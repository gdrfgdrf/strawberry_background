use crate::db::migration::migrator::Migrator;
use cpal::Data;
use lazy_static::lazy_static;
use parking_lot::RwLock;
use sea_orm::{
    ColumnTrait, ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbErr, EntityTrait,
};
use sea_orm_migration::MigratorTrait;
use std::ops::Deref;
use std::time::Duration;

#[macro_export]
macro_rules! all_columns {
    ($entity:path) => {
        <$entity>::iter()
            .map(|col| col.into_iden())
            .collect::<Vec<_>>()
    };
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum DatabaseError {
    #[error("Error Forward: {0}")]
    ErrorForward(String),
    #[error("Not Initialized")]
    NotInitialized,
}

impl From<DatabaseError> for String {
    fn from(value: DatabaseError) -> Self {
        value.to_string()
    }
}

impl From<String> for DatabaseError {
    fn from(value: String) -> Self {
        DatabaseError::ErrorForward(value)
    }
}

impl From<DbErr> for DatabaseError {
    fn from(value: DbErr) -> Self {
        DatabaseError::ErrorForward(value.to_string())
    }
}

lazy_static! {
    pub static ref DB: RwLock<Option<DatabaseConnection>> = RwLock::new(None);
}

pub async fn initialize_db(db_path: &str) -> Result<DatabaseConnection, DatabaseError> {
    let mut options = ConnectOptions::new(format!("sqlite://{}?mode=rwc", db_path));
    options
        .max_connections(100)
        .min_connections(5)
        .connect_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(30));

    let db = Database::connect(options).await?;
    db.execute_unprepared("PRAGMA journal_mode=WAL;").await?;
    db.execute_unprepared("PRAGMA synchronous=NORMAL;").await?;
    db.execute_unprepared("PRAGMA busy_timeout=5000;").await?;
    db.execute_unprepared("PRAGMA foreign_keys=ON;").await?;

    Ok(db)
}

pub async fn initialize_strawberry_background_db(db_path: &str) -> Result<(), DatabaseError> {
    let db = DB.read();
    if db.is_some() {
        return Ok(());
    }
    drop(db);

    let db = initialize_db(db_path).await?;
    Migrator::up(&db, None).await?;
    *DB.write() = Some(db);

    Ok(())
}
