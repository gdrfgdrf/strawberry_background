use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "cache_channels")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(unique, column_type = "Text")]
    pub name: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub extension: Option<String>,

    #[sea_orm(has_many)]
    pub records: HasMany<super::cache_records::Entity>
}

impl ActiveModelBehavior for ActiveModel {}