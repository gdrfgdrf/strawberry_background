use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, Eq, Hash, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "cookie_keys")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(column_type = "Text", unique_key = "keys")]
    pub path: String,
    #[sea_orm(column_type = "Text", unique_key = "keys")]
    pub name: String,
    #[sea_orm(column_type = "Text", unique_key = "keys")]
    pub domain: String,
}

impl ActiveModelBehavior for ActiveModel {}