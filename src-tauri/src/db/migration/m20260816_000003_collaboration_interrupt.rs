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
CREATE TABLE IF NOT EXISTS collaboration_interrupt_operation (
    id TEXT PRIMARY KEY NOT NULL,
    event_id TEXT NOT NULL,
    target_conversation_id INTEGER NOT NULL,
    client_dedupe_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'requested'
        CHECK (state IN (
            'requested', 'cancelling', 'terminal_observed',
            'waiting_for_terminal', 'ready', 'dispatching',
            'completed', 'failed'
        )),
    connection_id_snapshot TEXT NULL,
    error TEXT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (event_id) REFERENCES collaboration_event(id) ON DELETE CASCADE,
    UNIQUE (target_conversation_id, client_dedupe_id),
    UNIQUE (event_id, target_conversation_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_collaboration_interrupt_active_target
    ON collaboration_interrupt_operation(target_conversation_id)
    WHERE state IN (
        'requested', 'cancelling', 'terminal_observed',
        'waiting_for_terminal', 'ready', 'dispatching'
    );
CREATE INDEX IF NOT EXISTS idx_collaboration_interrupt_target_state
    ON collaboration_interrupt_operation(target_conversation_id, state, updated_at);
"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS collaboration_interrupt_operation;")
            .await?;
        Ok(())
    }
}
