//! Add lightweight business priority to task-board cards.
//!
//! Priority is planning metadata only. The dispatcher and PromptQueue keep
//! their existing ordering and interrupt rules.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE work_task ADD COLUMN priority TEXT NOT NULL DEFAULT 'none' \
                 CHECK (priority IN ('none', 'low', 'medium', 'high', 'urgent'));",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE work_task DROP COLUMN priority;")
            .await?;
        Ok(())
    }
}
