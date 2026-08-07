use sea_orm_migration::{prelude::*, schema::*};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260807_154726_create_cache_records_table"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("cache_records")
                    .if_not_exists()
                    .col(big_integer("id"))
                    .col(text("tag"))
                    .col(text("filename"))
                    .col(text("sentence"))
                    .col(big_integer("channel_id"))
                    .foreign_key(
                        ForeignKey::create()
                            .name("cache_records_to_cache_channels")
                            .from("cache_records", "channel_id")
                            .to("cache_channels", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .primary_key(Index::create().col("id").col("tag").col("filename"))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("cache_records").if_exists().to_owned())
            .await
    }
}
