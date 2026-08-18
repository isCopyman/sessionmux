//! Rooms outlive Workbenches. Deleting a Workbench must not cascade-delete
//! the Rooms that happened to be listed there.

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
PRAGMA foreign_keys=OFF;
CREATE TABLE collaboration_room_new (
    id TEXT PRIMARY KEY NOT NULL,
    workbench_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    created_by_conversation_id INTEGER NOT NULL,
    last_seen_at TEXT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    collection_id INTEGER NULL,
    root_folder_id INTEGER NULL,
    FOREIGN KEY (workbench_id) REFERENCES workbench(id) ON DELETE RESTRICT,
    FOREIGN KEY (collection_id) REFERENCES collection(id) ON DELETE SET NULL
);
INSERT INTO collaboration_room_new (
    id, workbench_id, title, status, created_by_conversation_id, last_seen_at,
    created_at, updated_at, collection_id, root_folder_id
)
SELECT
    id, workbench_id, title, status, created_by_conversation_id, last_seen_at,
    created_at, updated_at, collection_id, root_folder_id
FROM collaboration_room;
DROP TABLE collaboration_room;
ALTER TABLE collaboration_room_new RENAME TO collaboration_room;
CREATE INDEX IF NOT EXISTS idx_collaboration_room_workbench
    ON collaboration_room(workbench_id, updated_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_collaboration_room_collection
    ON collaboration_room(collection_id, updated_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_collaboration_room_root_folder
    ON collaboration_room(root_folder_id, updated_at DESC, id);
PRAGMA foreign_keys=ON;
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
PRAGMA foreign_keys=OFF;
CREATE TABLE collaboration_room_old (
    id TEXT PRIMARY KEY NOT NULL,
    workbench_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    created_by_conversation_id INTEGER NOT NULL,
    last_seen_at TEXT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    collection_id INTEGER NULL,
    root_folder_id INTEGER NULL,
    FOREIGN KEY (workbench_id) REFERENCES workbench(id) ON DELETE CASCADE,
    FOREIGN KEY (collection_id) REFERENCES collection(id) ON DELETE SET NULL
);
INSERT INTO collaboration_room_old (
    id, workbench_id, title, status, created_by_conversation_id, last_seen_at,
    created_at, updated_at, collection_id, root_folder_id
)
SELECT
    id, workbench_id, title, status, created_by_conversation_id, last_seen_at,
    created_at, updated_at, collection_id, root_folder_id
FROM collaboration_room;
DROP TABLE collaboration_room;
ALTER TABLE collaboration_room_old RENAME TO collaboration_room;
CREATE INDEX IF NOT EXISTS idx_collaboration_room_workbench
    ON collaboration_room(workbench_id, updated_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_collaboration_room_collection
    ON collaboration_room(collection_id, updated_at DESC, id);
CREATE INDEX IF NOT EXISTS idx_collaboration_room_root_folder
    ON collaboration_room(root_folder_id, updated_at DESC, id);
PRAGMA foreign_keys=ON;
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
    async fn workbench_delete_does_not_cascade_rooms() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE workbench (
                 id INTEGER PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL
             );
             CREATE TABLE collection (
                 id INTEGER PRIMARY KEY NOT NULL
             );
             CREATE TABLE collaboration_room (
                 id TEXT PRIMARY KEY NOT NULL,
                 workbench_id INTEGER NOT NULL,
                 title TEXT NOT NULL,
                 status TEXT NOT NULL DEFAULT 'active',
                 created_by_conversation_id INTEGER NOT NULL,
                 last_seen_at TEXT NULL,
                 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                 updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                 collection_id INTEGER NULL,
                 root_folder_id INTEGER NULL,
                 FOREIGN KEY (workbench_id) REFERENCES workbench(id) ON DELETE CASCADE
             );
             INSERT INTO workbench VALUES (1, 'Main');
             INSERT INTO workbench VALUES (2, 'Review');
             INSERT INTO collaboration_room (
                 id, workbench_id, title, created_by_conversation_id
             ) VALUES ('rm_plan', 2, 'Plan', 1);",
        )
        .await
        .expect("seed");
        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");
        let blocked = conn
            .execute_unprepared("DELETE FROM workbench WHERE id = 2")
            .await;
        assert!(blocked.is_err(), "RESTRICT must keep the Room");
        let count: i64 = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM collaboration_room WHERE id = 'rm_plan'".to_owned(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "count")
            .expect("column");
        assert_eq!(count, 1);
    }
}
