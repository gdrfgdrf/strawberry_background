use crate::db::migration::{
    m20260807_154717_create_cache_channels_table, m20260807_154726_create_cache_records_table,
    m20260814_070046_create_cookie_keys_table, m20260814_070053_create_cookies_table,
};
use async_trait::async_trait;
use sea_orm_migration::{MigrationTrait, MigratorTrait};

pub struct Migrator;

#[async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260807_154717_create_cache_channels_table::Migration),
            Box::new(m20260807_154726_create_cache_records_table::Migration),
            Box::new(m20260814_070046_create_cookie_keys_table::Migration),
            Box::new(m20260814_070053_create_cookies_table::Migration),
        ]
    }
}
