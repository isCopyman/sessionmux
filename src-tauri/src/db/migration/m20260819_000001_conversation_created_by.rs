//! Sessions gain a `created_by` provenance column (`user` / `agent` /
//! `automation`) so the workspace can filter Sessions by who spawned them.
//! Existing rows predate tracking and backfill to 'user' via the column
//! default.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
ALTER TABLE conversation
    ADD COLUMN created_by TEXT NOT NULL DEFAULT 'user'
    CHECK (created_by IN ('user', 'agent', 'automation'));
"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
ALTER TABLE conversation DROP COLUMN created_by;
"#,
            )
            .await?;
        Ok(())
    }
}
