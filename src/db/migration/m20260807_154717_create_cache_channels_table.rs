use sea_orm_migration::{prelude::*, schema::*};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260807_154717_create_cache_channels_table"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("cache_channels")
                    .if_not_exists()
                    .col(big_integer("id").primary_key().auto_increment())
                    .col(text("name").unique_key())
                    .col(text_null("extension"))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("cache_channels").if_exists().to_owned())
            .await
    }
}
