//! Fork lineage as its own fact table.
//!
//! A fork produces a NEW conversation row (C2) bound to the agent's new native
//! Session; C1 stays untouched. Nothing in the schema recorded that C2 came
//! from C1 — only the `[Fork] ` title prefix hinted at it, and a title is not a
//! queryable relation.
//!
//! `conversation.parent_id` is deliberately NOT reused: it means "delegation
//! child" (invariant `kind == Delegate` ⟺ `parent_id IS NOT NULL`), and a fork
//! writes `parent_id = NULL` on purpose. Widening that column would silently
//! change what every existing delegation query and test means.
//!
//! `relation_kind` starts at `fork_head` — the only relation the code produces
//! today. The two other values are the ones the Session History RFC §6 already
//! names (`HistoryOperationResult.relationKind`), pre-authorized here so adding
//! historical fork or handoff needs no schema change. `anchor` is the nullable
//! JSON slot for the provider-native message anchor a `fork_at_message`
//! relation will carry; head forks leave it NULL.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ForkRelation::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ForkRelation::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ForkRelation::SourceConversationId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ForkRelation::TargetConversationId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ForkRelation::RelationKind)
                            .string()
                            .not_null()
                            .default("fork_head")
                            .check(Expr::col(ForkRelation::RelationKind).is_in([
                                "fork_head",
                                "fork_at_message",
                                "handoff",
                            ])),
                    )
                    .col(ColumnDef::new(ForkRelation::Anchor).text().null())
                    .col(
                        ColumnDef::new(ForkRelation::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Both sides cascade. A lineage edge is only meaningful
                    // while both endpoints exist: a hard-deleted endpoint
                    // leaves nothing to render or jump to, and keeping the row
                    // would strand a dangling id the UI must special-case.
                    // Soft deletion (`deleted_at`) does NOT fire this — the
                    // edge survives, and the lineage query filters it out.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_fork_relation_source")
                            .from(ForkRelation::Table, ForkRelation::SourceConversationId)
                            .to(Conversation::Table, Conversation::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_fork_relation_target")
                            .from(ForkRelation::Table, ForkRelation::TargetConversationId)
                            .to(Conversation::Table, Conversation::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // One edge per (source, target, kind): a retried write can't double the
        // "forked from" badge. Kind is part of the key so a future
        // `fork_at_message` between the same two rows is still expressible.
        // This index's leading column also serves the "what was forked out of
        // me?" direction, so no separate source-only index is created.
        manager
            .create_index(
                Index::create()
                    .name("uq_fork_relation_edge")
                    .table(ForkRelation::Table)
                    .col(ForkRelation::SourceConversationId)
                    .col(ForkRelation::TargetConversationId)
                    .col(ForkRelation::RelationKind)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // The "who was I forked from?" direction.
        manager
            .create_index(
                Index::create()
                    .name("idx_fork_relation_target")
                    .table(ForkRelation::Table)
                    .col(ForkRelation::TargetConversationId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(ForkRelation::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum ForkRelation {
    Table,
    Id,
    SourceConversationId,
    TargetConversationId,
    RelationKind,
    Anchor,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Conversation {
    Table,
    Id,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{ConnectionTrait, Database, DbBackend, Statement};

    async fn stub_db() -> sea_orm_migration::sea_orm::DatabaseConnection {
        let conn = Database::connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        conn.execute_unprepared(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE conversation (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 title TEXT NULL
             );
             INSERT INTO conversation (id, title) VALUES (1, 'C1'), (2, 'C2'), (3, 'C3');",
        )
        .await
        .expect("stub conversation table");
        Migration
            .up(&SchemaManager::new(&conn))
            .await
            .expect("run migration up");
        conn
    }

    async fn count(conn: &sea_orm_migration::sea_orm::DatabaseConnection, sql: &str) -> i64 {
        conn.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_owned()))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "count")
            .expect("column")
    }

    #[tokio::test]
    async fn up_creates_fork_relation_with_default_kind_and_null_anchor() {
        let conn = stub_db().await;
        conn.execute_unprepared(
            "INSERT INTO fork_relation (source_conversation_id, target_conversation_id)
             VALUES (1, 2);",
        )
        .await
        .expect("insert edge");

        let kind: String = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT relation_kind AS kind FROM fork_relation WHERE source_conversation_id = 1"
                    .to_owned(),
            ))
            .await
            .expect("query")
            .expect("row")
            .try_get("", "kind")
            .expect("column");
        assert_eq!(kind, "fork_head", "relation_kind defaults to fork_head");

        let null_anchors = count(
            &conn,
            "SELECT COUNT(*) AS count FROM fork_relation WHERE anchor IS NULL",
        )
        .await;
        assert_eq!(null_anchors, 1, "a head fork stores no anchor");
    }

    #[tokio::test]
    async fn relation_kind_check_rejects_an_unknown_value() {
        let conn = stub_db().await;
        let bad = conn
            .execute_unprepared(
                "INSERT INTO fork_relation
                     (source_conversation_id, target_conversation_id, relation_kind)
                 VALUES (1, 2, 'truncate_in_place');",
            )
            .await;
        assert!(
            bad.is_err(),
            "an unmodelled relation_kind must be rejected by the CHECK constraint"
        );

        // The three modelled kinds all pass.
        for (target, kind) in [(2, "fork_head"), (2, "fork_at_message"), (3, "handoff")] {
            conn.execute_unprepared(&format!(
                "INSERT INTO fork_relation
                     (source_conversation_id, target_conversation_id, relation_kind)
                 VALUES (1, {target}, '{kind}');"
            ))
            .await
            .unwrap_or_else(|e| panic!("{kind} must be accepted: {e}"));
        }
    }

    #[tokio::test]
    async fn duplicate_edge_of_the_same_kind_is_rejected() {
        let conn = stub_db().await;
        conn.execute_unprepared(
            "INSERT INTO fork_relation (source_conversation_id, target_conversation_id)
             VALUES (1, 2);",
        )
        .await
        .expect("insert edge");

        let dup = conn
            .execute_unprepared(
                "INSERT INTO fork_relation (source_conversation_id, target_conversation_id)
                 VALUES (1, 2);",
            )
            .await;
        assert!(dup.is_err(), "duplicate (source, target, kind) is rejected");

        // Same pair, different kind, is a distinct edge.
        conn.execute_unprepared(
            "INSERT INTO fork_relation
                 (source_conversation_id, target_conversation_id, relation_kind)
             VALUES (1, 2, 'fork_at_message');",
        )
        .await
        .expect("a different kind between the same pair is allowed");
    }

    #[tokio::test]
    async fn hard_deleting_either_endpoint_cascades_the_edge() {
        let conn = stub_db().await;
        conn.execute_unprepared(
            "INSERT INTO fork_relation (source_conversation_id, target_conversation_id)
             VALUES (1, 2), (1, 3);",
        )
        .await
        .expect("insert edges");

        conn.execute_unprepared("DELETE FROM conversation WHERE id = 3;")
            .await
            .expect("delete target");
        assert_eq!(
            count(&conn, "SELECT COUNT(*) AS count FROM fork_relation").await,
            1,
            "deleting the target removes its edge"
        );

        conn.execute_unprepared("DELETE FROM conversation WHERE id = 1;")
            .await
            .expect("delete source");
        assert_eq!(
            count(&conn, "SELECT COUNT(*) AS count FROM fork_relation").await,
            0,
            "deleting the source removes its edges too"
        );
    }

    #[tokio::test]
    async fn edge_must_point_at_existing_conversations() {
        let conn = stub_db().await;
        let orphan = conn
            .execute_unprepared(
                "INSERT INTO fork_relation (source_conversation_id, target_conversation_id)
                 VALUES (1, 999);",
            )
            .await;
        assert!(orphan.is_err(), "the target FK must reject an unknown row");
    }
}
