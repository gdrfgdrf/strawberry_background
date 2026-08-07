use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "cache_records")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(primary_key, column_type = "Text")]
    pub tag: String,
    #[sea_orm(primary_key, column_type = "Text")]
    pub filename: String,
    pub sentence: String,
    pub channel_id: i64,
    #[sea_orm(belongs_to, from = "channel_id", to = "id")]
    pub channel: HasOne<super::cache_channels::Entity>
}

impl ActiveModelBehavior for ActiveModel {}