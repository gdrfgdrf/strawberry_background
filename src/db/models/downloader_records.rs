use sea_orm::entity::prelude::*;
use chrono::FixedOffset;
use sea_orm::sqlx::types::chrono::DateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Hash)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum DownloaderPriority {
    Normal = 0,
    Privileged = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Hash)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum DownloaderStatus {
    Enqueued = 0,
    Running = 1,
    Paused = 2,
    WaitForRetry = 3,
    Error = 4,
}

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "downloader_records")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(unique, column_type = "Text")]
    pub request_id: String,
    pub channel_id: i64,
    #[sea_orm(column_type = "Text")]
    pub url: String,
    #[sea_orm(column_type = "Text")]
    pub path: String,
    pub priority: DownloaderPriority,
    pub downloaded: i64,
    pub expected_length: Option<i64>,
    pub status: DownloaderStatus,
    pub retried_count: i32,
    #[sea_orm(column_type = "Text", nullable)]
    pub error_message: Option<String>,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
}

impl ActiveModelBehavior for ActiveModel {}
