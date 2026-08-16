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
ALTER TABLE conversation ADD COLUMN harness_internal INTEGER NOT NULL DEFAULT 0;
ALTER TABLE conversation ADD COLUMN codeg_owned INTEGER NOT NULL DEFAULT 0;
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
ALTER TABLE conversation DROP COLUMN harness_internal;
ALTER TABLE conversation DROP COLUMN codeg_owned;
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
    async fn up_adds_projection_flags_defaulting_false() {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "CREATE TABLE conversation (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT)",
        )
        .await
        .expect("create stub table");
        conn.execute_unprepared("INSERT INTO conversation (title) VALUES ('x')")
            .await
            .expect("insert row");

        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");

        let rows = conn
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT harness_internal, codeg_owned FROM conversation".to_owned(),
            ))
            .await
            .expect("query rows");
        let harness_internal: i64 = rows[0]
            .try_get("", "harness_internal")
            .expect("harness_internal");
        let codeg_owned: i64 = rows[0].try_get("", "codeg_owned").expect("codeg_owned");
        assert_eq!(harness_internal, 0);
        assert_eq!(codeg_owned, 0);
    }
}
