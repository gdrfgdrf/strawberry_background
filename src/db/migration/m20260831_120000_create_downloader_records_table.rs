use sea_orm_migration::{prelude::*, schema::*};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260831_120000_create_downloader_records_table"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("downloader_records")
                    .if_not_exists()
                    .col(big_integer("id").primary_key().auto_increment())
                    .col(text("request_id").unique_key())
                    .col(big_integer("channel_id"))
                    .col(text("url"))
                    .col(text("path"))
                    .col(integer("priority"))
                    .col(big_integer("downloaded"))
                    .col(big_integer_null("expected_length"))
                    .col(integer("status"))
                    .col(integer("retried_count"))
                    .col(text_null("error_message"))
                    .col(timestamp_with_time_zone("created_at"))
                    .col(timestamp_with_time_zone("updated_at"))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("downloader_records").if_exists().to_owned())
            .await
    }
}
