use sea_orm_migration::{prelude::*, schema::*};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260814_070053_create_cookies_table"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("cookies")
                    .if_not_exists()
                    .col(big_integer("id").primary_key().auto_increment())
                    .col(big_integer("key_id").unique_key())
                    .col(text("value"))
                    .col(timestamp_with_time_zone_null("expires_at"))
                    .col(timestamp_with_time_zone("created_at"))
                    .col(timestamp_with_time_zone("last_access_at"))
                    .col(boolean("secure"))
                    .col(boolean("http_only"))
                    .col(unsigned_null("same_site"))
                    .foreign_key(
                        ForeignKey::create()
                            .name("cookies_to_cookie_keys")
                            .from("cookies", "key_id")
                            .to("cookie_keys", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("cookies").if_exists().to_owned())
            .await
    }
}
