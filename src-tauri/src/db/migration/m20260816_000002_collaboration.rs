use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Collaboration is an application-owned communication fact, not a
        // Harness transcript turn. The author id deliberately has no
        // conversation FK: deleting the author must not erase mail that
        // already arrived in another Session. Frozen metadata keeps the event
        // renderable after a rename, archive, import, or hard deletion.
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE TABLE IF NOT EXISTS collaboration_event (
    id TEXT PRIMARY KEY NOT NULL,
    source_conversation_id INTEGER NOT NULL,
    source_title_snapshot TEXT NULL,
    source_agent_type_snapshot TEXT NOT NULL,
    source_folder_path_snapshot TEXT NULL,
    source_backend_snapshot TEXT NOT NULL DEFAULT 'current',
    body TEXT NOT NULL,
    reply_to_event_id TEXT NULL,
    expects_reply INTEGER NOT NULL DEFAULT 0 CHECK (expects_reply IN (0, 1)),
    urgency TEXT NOT NULL DEFAULT 'normal' CHECK (urgency IN ('normal', 'urgent')),
    client_dedupe_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (source_conversation_id, client_dedupe_id),
    FOREIGN KEY (reply_to_event_id) REFERENCES collaboration_event(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS collaboration_delivery (
    id TEXT PRIMARY KEY NOT NULL,
    event_id TEXT NOT NULL,
    target_conversation_id INTEGER NOT NULL,
    target_title_snapshot TEXT NULL,
    target_agent_type_snapshot TEXT NULL,
    target_folder_path_snapshot TEXT NULL,
    invocation_policy TEXT NOT NULL DEFAULT 'store_only'
        CHECK (invocation_policy IN ('store_only', 'invoke_when_idle')),
    delivery_hint TEXT NOT NULL DEFAULT 'default'
        CHECK (delivery_hint IN ('default', 'steer_if_supported')),
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'queued', 'embedding', 'embedded', 'dismissed', 'failed')),
    ui_seen_at TEXT NULL,
    embedded_turn_ref TEXT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    error TEXT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (event_id) REFERENCES collaboration_event(id) ON DELETE CASCADE,
    UNIQUE (event_id, target_conversation_id)
);

CREATE TABLE IF NOT EXISTS conversation_collaboration_state (
    conversation_id INTEGER PRIMARY KEY NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (conversation_id) REFERENCES conversation(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_collaboration_event_source_created
    ON collaboration_event(source_conversation_id, created_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_collaboration_delivery_target_created
    ON collaboration_delivery(target_conversation_id, created_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_collaboration_delivery_event
    ON collaboration_delivery(event_id, target_conversation_id);
CREATE INDEX IF NOT EXISTS idx_collaboration_delivery_pending
    ON collaboration_delivery(target_conversation_id, state, ui_seen_at);
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
DROP INDEX IF EXISTS idx_collaboration_delivery_pending;
DROP INDEX IF EXISTS idx_collaboration_delivery_event;
DROP INDEX IF EXISTS idx_collaboration_delivery_target_created;
DROP INDEX IF EXISTS idx_collaboration_event_source_created;
DROP TABLE IF EXISTS conversation_collaboration_state;
DROP TABLE IF EXISTS collaboration_delivery;
DROP TABLE IF EXISTS collaboration_event;
"#,
            )
            .await?;
        Ok(())
    }
}
