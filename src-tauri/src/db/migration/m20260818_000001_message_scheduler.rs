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
ALTER TABLE conversation_prompt_queue_item
    ADD COLUMN source TEXT NOT NULL DEFAULT 'user'
    CHECK (source IN ('user', 'collaboration', 'reminder', 'timer'));
UPDATE conversation_prompt_queue_item
    SET source = 'collaboration'
    WHERE origin_event_id IS NOT NULL;
UPDATE conversation_prompt_queue_item
    SET source = 'reminder'
    WHERE client_dedupe_id LIKE 'mailbox-attention:%';
UPDATE conversation_prompt_queue_item
    SET source = 'timer'
    WHERE client_dedupe_id LIKE 'timer-fire-%';
CREATE INDEX IF NOT EXISTS idx_prompt_queue_claim_order
    ON conversation_prompt_queue_item(conversation_id, state, source, position);
ALTER TABLE conversation_timer
    ADD COLUMN strike_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE conversation_timer
    ADD COLUMN auto_paused_at TEXT NULL;
ALTER TABLE conversation_timer
    ADD COLUMN auto_pause_reason TEXT NULL;
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
ALTER TABLE conversation_timer DROP COLUMN auto_pause_reason;
ALTER TABLE conversation_timer DROP COLUMN auto_paused_at;
ALTER TABLE conversation_timer DROP COLUMN strike_count;
DROP INDEX IF EXISTS idx_prompt_queue_claim_order;
ALTER TABLE conversation_prompt_queue_item DROP COLUMN source;
"#,
            )
            .await?;
        Ok(())
    }
}
