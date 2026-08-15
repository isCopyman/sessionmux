use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The queue is deliberately separate from `conversation`: a Session's
        // native transcript remains the conversation authority, while these
        // rows are host-owned follow-ups that have not yet been accepted by the
        // Harness. `draft_json XOR origin_event_id` reserves the same durable
        // queue for future cross-Session deliveries without copying an event's
        // body into a mutable local draft.
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE TABLE IF NOT EXISTS conversation_prompt_queue_state (
    conversation_id INTEGER PRIMARY KEY NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    paused_reason TEXT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (conversation_id) REFERENCES conversation(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS conversation_prompt_queue_item (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id INTEGER NOT NULL,
    position INTEGER NOT NULL,
    draft_json TEXT NULL,
    origin_event_id TEXT NULL,
    mode_id TEXT NULL,
    state TEXT NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'claimed', 'paused')),
    client_dedupe_id TEXT NOT NULL,
    claimed_by TEXT NULL,
    claim_expires_at TEXT NULL,
    dispatch_started_at TEXT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    paused_reason TEXT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (conversation_id) REFERENCES conversation(id) ON DELETE CASCADE,
    CHECK ((draft_json IS NOT NULL) <> (origin_event_id IS NOT NULL)),
    UNIQUE (conversation_id, client_dedupe_id)
);

CREATE INDEX IF NOT EXISTS idx_conversation_prompt_queue_order
    ON conversation_prompt_queue_item(conversation_id, position, created_at, id);
CREATE INDEX IF NOT EXISTS idx_conversation_prompt_queue_claim
    ON conversation_prompt_queue_item(state, claim_expires_at);
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
DROP INDEX IF EXISTS idx_conversation_prompt_queue_claim;
DROP INDEX IF EXISTS idx_conversation_prompt_queue_order;
DROP TABLE IF EXISTS conversation_prompt_queue_item;
DROP TABLE IF EXISTS conversation_prompt_queue_state;
"#,
            )
            .await?;
        Ok(())
    }
}
