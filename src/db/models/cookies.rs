use sea_orm::entity::prelude::*;
use chrono::{FixedOffset, TimeZone, Utc};
use sea_orm::sqlx::types::chrono::DateTime;

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum, Hash)]
#[sea_orm(rs_type = "u32", db_type = "Integer")]
pub enum SameSite {
    Strict = 0,
    Lax = 1,
    None = 2,
}

#[sea_orm::model]
#[derive(Clone, Debug, Eq, Hash, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "cookies")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(unique)]
    pub key_id: i64,
    #[sea_orm(column_type = "Text")]
    pub value: String,
    pub expires_at: Option<DateTime<FixedOffset>>,
    pub created_at: DateTime<FixedOffset>,
    pub last_access_at: DateTime<FixedOffset>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<SameSite>,

    #[sea_orm(belongs_to, from = "key_id", to = "id")]
    pub key: HasOne<super::cookie_keys::Entity>
}

impl ActiveModelBehavior for ActiveModel {}