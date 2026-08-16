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
ALTER TABLE conversation_collaboration_state
    ADD COLUMN reminder_last_at TEXT NULL;
ALTER TABLE conversation_collaboration_state
    ADD COLUMN reminder_repeat_count INTEGER NOT NULL DEFAULT 0;
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
ALTER TABLE conversation_collaboration_state DROP COLUMN reminder_repeat_count;
ALTER TABLE conversation_collaboration_state DROP COLUMN reminder_last_at;
"#,
            )
            .await?;
        Ok(())
    }
}
