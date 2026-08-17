use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Compatibility-shaped Timer storage. The first product path exposes
        // only `idle_for`: after a real TurnComplete boundary, the backend
        // enqueues the Timer text through the authoritative PromptQueue.
        // The broader columns remain reserved so early development databases
        // do not require a destructive schema rewrite; they are not public
        // `at` or `interval` capabilities in this release.
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE TABLE IF NOT EXISTS conversation_timer (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id INTEGER NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('at', 'interval', 'idle_for')),
    at_time TEXT NULL,
    interval_secs INTEGER NULL,
    idle_secs INTEGER NULL,
    prompt_text TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    repeat_idle INTEGER NOT NULL DEFAULT 0,
    last_fired_at TEXT NULL,
    next_fire_at TEXT NULL,
    fire_count INTEGER NOT NULL DEFAULT 0,
    client_dedupe_id TEXT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (conversation_id) REFERENCES conversation(id) ON DELETE CASCADE,
    CHECK (
        (mode = 'at' AND at_time IS NOT NULL) OR
        (mode = 'interval' AND interval_secs IS NOT NULL AND interval_secs > 0) OR
        (mode = 'idle_for' AND idle_secs IS NOT NULL AND idle_secs > 0)
    )
);

CREATE INDEX IF NOT EXISTS idx_conversation_timer_conversation
    ON conversation_timer(conversation_id, enabled);
CREATE INDEX IF NOT EXISTS idx_conversation_timer_due
    ON conversation_timer(enabled, next_fire_at);
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
DROP INDEX IF EXISTS idx_conversation_timer_due;
DROP INDEX IF EXISTS idx_conversation_timer_conversation;
DROP TABLE IF EXISTS conversation_timer;
"#,
            )
            .await?;
        Ok(())
    }
}
