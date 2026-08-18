//! Rooms are a shared space. Members are equal — there is no owner role.
//! `created_by_conversation_id` stays as an audit / human-ledger placeholder
//! until Human Inbox lands; it is not a rank.

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
UPDATE collaboration_room_member
SET role = 'member'
WHERE role != 'member';
"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

    #[tokio::test]
    async fn flattens_owner_rows_to_member() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "CREATE TABLE collaboration_room_member (
                 room_id TEXT NOT NULL,
                 conversation_id INTEGER NOT NULL,
                 role TEXT NOT NULL
             );
             INSERT INTO collaboration_room_member VALUES ('rm_1', 1, 'owner');
             INSERT INTO collaboration_room_member VALUES ('rm_1', 2, 'member');",
        )
        .await
        .expect("seed");
        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");
        let owners: i64 = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM collaboration_room_member WHERE role = 'owner'"
                    .to_owned(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "count")
            .expect("column");
        assert_eq!(owners, 0);
    }
}
