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
ALTER TABLE collaboration_delivery ADD COLUMN attention_state TEXT NOT NULL DEFAULT 'unread'
    CHECK (attention_state IN ('unread', 'opened'));
ALTER TABLE collaboration_delivery ADD COLUMN opened_at TEXT NULL;
ALTER TABLE collaboration_delivery ADD COLUMN agent_received_at TEXT NULL;
ALTER TABLE collaboration_delivery ADD COLUMN agent_receipt_kind TEXT NULL
    CHECK (agent_receipt_kind IS NULL OR agent_receipt_kind IN ('managed_acp', 'legacy_embedded'));
ALTER TABLE collaboration_delivery ADD COLUMN agent_receipt_ref TEXT NULL;
ALTER TABLE collaboration_delivery ADD COLUMN obligation_state TEXT NOT NULL DEFAULT 'none'
    CHECK (obligation_state IN ('none', 'awaiting_reply', 'resolved'));
ALTER TABLE collaboration_delivery ADD COLUMN obligation_created_at TEXT NULL;
ALTER TABLE collaboration_delivery ADD COLUMN obligation_resolved_at TEXT NULL;

UPDATE collaboration_delivery
SET attention_state = CASE WHEN ui_seen_at IS NULL THEN 'unread' ELSE 'opened' END,
    opened_at = ui_seen_at;

UPDATE collaboration_delivery
SET agent_receipt_kind = 'legacy_embedded',
    agent_receipt_ref = embedded_turn_ref
WHERE state = 'embedded' AND embedded_turn_ref IS NOT NULL;

UPDATE collaboration_delivery
SET obligation_state = CASE
        WHEN COALESCE((
            SELECT e.expects_reply FROM collaboration_event e
            WHERE e.id = collaboration_delivery.event_id
        ), 0) = 0 THEN 'none'
        WHEN EXISTS (
            SELECT 1 FROM collaboration_event reply
            WHERE reply.reply_to_event_id = collaboration_delivery.event_id
              AND reply.source_conversation_id = collaboration_delivery.target_conversation_id
        ) THEN 'resolved'
        ELSE 'awaiting_reply'
    END,
    obligation_created_at = CASE
        WHEN COALESCE((
            SELECT e.expects_reply FROM collaboration_event e
            WHERE e.id = collaboration_delivery.event_id
        ), 0) = 1 THEN (
            SELECT e.created_at FROM collaboration_event e
            WHERE e.id = collaboration_delivery.event_id
        )
        ELSE NULL
    END,
    obligation_resolved_at = CASE
        WHEN COALESCE((
            SELECT e.expects_reply FROM collaboration_event e
            WHERE e.id = collaboration_delivery.event_id
        ), 0) = 1 THEN (
            SELECT MIN(reply.created_at) FROM collaboration_event reply
            WHERE reply.reply_to_event_id = collaboration_delivery.event_id
              AND reply.source_conversation_id = collaboration_delivery.target_conversation_id
        )
        ELSE NULL
    END;

CREATE INDEX IF NOT EXISTS idx_collaboration_delivery_attention
    ON collaboration_delivery(target_conversation_id, attention_state, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_collaboration_delivery_obligation
    ON collaboration_delivery(target_conversation_id, obligation_state, created_at DESC);
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
DROP INDEX IF EXISTS idx_collaboration_delivery_obligation;
DROP INDEX IF EXISTS idx_collaboration_delivery_attention;
ALTER TABLE collaboration_delivery DROP COLUMN obligation_resolved_at;
ALTER TABLE collaboration_delivery DROP COLUMN obligation_created_at;
ALTER TABLE collaboration_delivery DROP COLUMN obligation_state;
ALTER TABLE collaboration_delivery DROP COLUMN agent_receipt_ref;
ALTER TABLE collaboration_delivery DROP COLUMN agent_receipt_kind;
ALTER TABLE collaboration_delivery DROP COLUMN agent_received_at;
ALTER TABLE collaboration_delivery DROP COLUMN opened_at;
ALTER TABLE collaboration_delivery DROP COLUMN attention_state;
"#,
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

    #[tokio::test]
    async fn up_backfills_attention_receipt_and_per_target_obligation_without_inventing_time() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        conn.execute_unprepared(
            "CREATE TABLE collaboration_event (\
                id TEXT PRIMARY KEY NOT NULL, \
                source_conversation_id INTEGER NOT NULL, \
                expects_reply INTEGER NOT NULL, \
                reply_to_event_id TEXT NULL, \
                created_at TEXT NOT NULL\
            ); \
            CREATE TABLE collaboration_delivery (\
                id TEXT PRIMARY KEY NOT NULL, \
                event_id TEXT NOT NULL, \
                target_conversation_id INTEGER NOT NULL, \
                state TEXT NOT NULL, \
                ui_seen_at TEXT NULL, \
                embedded_turn_ref TEXT NULL, \
                created_at TEXT NOT NULL\
            ); \
            INSERT INTO collaboration_event VALUES \
                ('question', 1, 1, NULL, '2026-08-16 01:00:00'), \
                ('answer-a', 2, 0, 'question', '2026-08-16 02:00:00'), \
                ('notice', 1, 0, NULL, '2026-08-16 03:00:00'), \
                ('answer-notice', 2, 0, 'notice', '2026-08-16 04:00:00'); \
            INSERT INTO collaboration_delivery VALUES \
                ('delivery-a', 'question', 2, 'embedded', '2026-08-16 01:30:00', 'turn-a', '2026-08-16 01:00:00'), \
                ('delivery-b', 'question', 3, 'pending', NULL, NULL, '2026-08-16 01:00:00'), \
                ('delivery-notice', 'notice', 2, 'pending', NULL, NULL, '2026-08-16 03:00:00');",
        )
        .await
        .unwrap();

        Migration.up(&SchemaManager::new(&conn)).await.unwrap();

        let rows = conn
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id, attention_state, opened_at, agent_received_at, \
                        agent_receipt_kind, agent_receipt_ref, obligation_state, \
                        obligation_created_at, obligation_resolved_at \
                 FROM collaboration_delivery ORDER BY id"
                    .to_string(),
            ))
            .await
            .unwrap();

        let a = &rows[0];
        assert_eq!(
            a.try_get::<String>("", "attention_state").unwrap(),
            "opened"
        );
        assert_eq!(
            a.try_get::<Option<String>>("", "opened_at")
                .unwrap()
                .as_deref(),
            Some("2026-08-16 01:30:00")
        );
        assert_eq!(
            a.try_get::<Option<String>>("", "agent_receipt_kind")
                .unwrap()
                .as_deref(),
            Some("legacy_embedded")
        );
        assert_eq!(
            a.try_get::<Option<String>>("", "agent_receipt_ref")
                .unwrap()
                .as_deref(),
            Some("turn-a")
        );
        assert!(a
            .try_get::<Option<String>>("", "agent_received_at")
            .unwrap()
            .is_none());
        assert_eq!(
            a.try_get::<String>("", "obligation_state").unwrap(),
            "resolved"
        );
        assert_eq!(
            a.try_get::<Option<String>>("", "obligation_resolved_at")
                .unwrap()
                .as_deref(),
            Some("2026-08-16 02:00:00")
        );

        let b = &rows[1];
        assert_eq!(
            b.try_get::<String>("", "attention_state").unwrap(),
            "unread"
        );
        assert_eq!(
            b.try_get::<String>("", "obligation_state").unwrap(),
            "awaiting_reply"
        );
        assert_eq!(
            b.try_get::<Option<String>>("", "obligation_created_at")
                .unwrap()
                .as_deref(),
            Some("2026-08-16 01:00:00")
        );

        let notice = &rows[2];
        assert_eq!(
            notice.try_get::<String>("", "obligation_state").unwrap(),
            "none"
        );
        assert!(notice
            .try_get::<Option<String>>("", "obligation_resolved_at")
            .unwrap()
            .is_none());
    }
}
