use sea_orm::Schema;
use sea_orm_migration::{prelude::*, schema::*};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260814_070046_create_cookie_keys_table"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("cookie_keys")
                    .if_not_exists()
                    .col(big_integer("id").primary_key().auto_increment())
                    .col(text("path"))
                    .col(text("name"))
                    .col(text("domain"))
                    .index(
                        Index::create()
                            .name("keys")
                            .col("path")
                            .col("name")
                            .col("domain")
                            .unique(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("cookie_keys").if_exists().to_owned())
            .await
    }
}
