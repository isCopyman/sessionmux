//! Per-member Room read cursor.
//!
//! Channel unread is "posts after this member last read", not a room-global
//! `last_seen_at`. New members start caught up (`last_read_at = joined_at`)
//! so joining does not inherit the whole ledger as unread. `last_read_event_id`
//! breaks same-second ties the way the timeline already uses `rowid`.

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
ALTER TABLE collaboration_room_member
    ADD COLUMN last_read_event_id TEXT NULL;

UPDATE collaboration_room_member
SET last_read_at = COALESCE(last_read_at, joined_at)
WHERE last_read_at IS NULL;
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
ALTER TABLE collaboration_room_member DROP COLUMN last_read_event_id;
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
    async fn up_adds_last_read_event_id_and_catches_up_existing_members() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "CREATE TABLE collaboration_room_member (
                 room_id TEXT NOT NULL,
                 conversation_id INTEGER NOT NULL,
                 role TEXT NOT NULL,
                 joined_at TEXT NOT NULL,
                 last_read_at TEXT NULL,
                 PRIMARY KEY (room_id, conversation_id)
             );
             INSERT INTO collaboration_room_member
                 (room_id, conversation_id, role, joined_at, last_read_at)
             VALUES ('rm', 1, 'member', '2026-08-01 00:00:00', NULL);",
        )
        .await
        .expect("seed");
        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");
        let row = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_read_at, last_read_event_id FROM collaboration_room_member".to_owned(),
            ))
            .await
            .expect("query")
            .expect("row");
        let last_read_at: String = row.try_get("", "last_read_at").expect("last_read_at");
        let last_read_event_id: Option<String> = row
            .try_get("", "last_read_event_id")
            .expect("last_read_event_id");
        assert_eq!(last_read_at, "2026-08-01 00:00:00");
        assert!(last_read_event_id.is_none());
    }
}
