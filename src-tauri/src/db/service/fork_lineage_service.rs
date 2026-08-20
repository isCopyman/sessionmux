//! Reads and writes the `fork_relation` lineage table.
//!
//! This is the only place that knows a fork edge exists. `conversation.parent_id`
//! is NOT involved: it means delegation, and a fork writes NULL to it on
//! purpose (`acp/manager.rs`, `persist_fork_outcome`).

use std::collections::HashSet;

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, Condition, ConnectionTrait, EntityTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};

use crate::db::entities::fork_relation::ForkRelationKind;
use crate::db::entities::{conversation, fork_relation};
use crate::db::error::DbError;
use crate::models::{ForkLineage, ForkRelationRef};

/// Record that `target` was produced by a whole-Session fork of `source`.
///
/// Generic over the connection so the ACP fork can call this inside the very
/// transaction that INSERTs the target row — the lineage edge and the Session
/// it describes must become visible together or not at all.
pub async fn record_fork_head<C: ConnectionTrait>(
    conn: &C,
    source_conversation_id: i32,
    target_conversation_id: i32,
) -> Result<fork_relation::Model, DbError> {
    if source_conversation_id == target_conversation_id {
        return Err(DbError::Validation(format!(
            "fork lineage: a conversation cannot fork from itself ({source_conversation_id})"
        )));
    }
    let row = fork_relation::ActiveModel {
        id: NotSet,
        source_conversation_id: Set(source_conversation_id),
        target_conversation_id: Set(target_conversation_id),
        relation_kind: Set(ForkRelationKind::ForkHead),
        anchor: Set(None),
        created_at: Set(Utc::now()),
    }
    .insert(conn)
    .await?;
    Ok(row)
}

/// Both directions of one conversation's lineage: what it was forked from, and
/// what was forked out of it.
///
/// Edges whose OTHER endpoint is soft-deleted are dropped. A soft-deleted row
/// is invisible everywhere else in the app, so returning it would hand the UI a
/// badge that resolves to nothing. Hard deletion is handled by the FKs, which
/// cascade the edge away.
pub async fn lineage_for<C: ConnectionTrait>(
    conn: &C,
    conversation_id: i32,
) -> Result<ForkLineage, DbError> {
    let rows = fork_relation::Entity::find()
        .filter(
            Condition::any()
                .add(fork_relation::Column::SourceConversationId.eq(conversation_id))
                .add(fork_relation::Column::TargetConversationId.eq(conversation_id)),
        )
        // Oldest first, id as the tie-break so same-timestamp edges are stable.
        .order_by_asc(fork_relation::Column::CreatedAt)
        .order_by_asc(fork_relation::Column::Id)
        .all(conn)
        .await?;
    if rows.is_empty() {
        return Ok(ForkLineage::default());
    }

    let counterparts: Vec<i32> = rows
        .iter()
        .map(|row| {
            if row.source_conversation_id == conversation_id {
                row.target_conversation_id
            } else {
                row.source_conversation_id
            }
        })
        .collect();
    let live: HashSet<i32> = conversation::Entity::find()
        .select_only()
        .column(conversation::Column::Id)
        .filter(conversation::Column::Id.is_in(counterparts))
        .filter(conversation::Column::DeletedAt.is_null())
        .into_tuple::<i32>()
        .all(conn)
        .await?
        .into_iter()
        .collect();

    let mut lineage = ForkLineage::default();
    for row in rows {
        let is_target = row.target_conversation_id == conversation_id;
        let counterpart = if is_target {
            row.source_conversation_id
        } else {
            row.target_conversation_id
        };
        if !live.contains(&counterpart) {
            continue;
        }
        let edge = ForkRelationRef {
            id: row.id,
            source_conversation_id: row.source_conversation_id,
            target_conversation_id: row.target_conversation_id,
            relation_kind: row.relation_kind,
            anchor: row.anchor,
            created_at: row.created_at,
        };
        if is_target {
            lineage.forked_from.push(edge);
        } else {
            lineage.forks.push(edge);
        }
    }
    Ok(lineage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::service::conversation_service;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::db::AppDatabase;
    use crate::models::AgentType;

    async fn seed(count: usize) -> (AppDatabase, Vec<i32>) {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-fork-lineage").await;
        let mut ids = Vec::with_capacity(count);
        for _ in 0..count {
            ids.push(seed_conversation(&db, folder_id, AgentType::ClaudeCode).await);
        }
        (db, ids)
    }

    #[tokio::test]
    async fn lineage_of_an_unforked_conversation_is_empty() {
        let (db, ids) = seed(1).await;
        let lineage = lineage_for(&db.conn, ids[0]).await.expect("lineage");
        assert_eq!(lineage, ForkLineage::default());
    }

    #[tokio::test]
    async fn a_source_can_have_several_forks_and_they_come_back_in_insertion_order() {
        let (db, ids) = seed(3).await;
        record_fork_head(&db.conn, ids[0], ids[1])
            .await
            .expect("first fork");
        record_fork_head(&db.conn, ids[0], ids[2])
            .await
            .expect("second fork");

        let lineage = lineage_for(&db.conn, ids[0]).await.expect("lineage");
        assert!(lineage.forked_from.is_empty());
        let targets: Vec<i32> = lineage
            .forks
            .iter()
            .map(|edge| edge.target_conversation_id)
            .collect();
        assert_eq!(targets, vec![ids[1], ids[2]]);
    }

    #[tokio::test]
    async fn an_edge_is_hidden_once_its_other_endpoint_is_soft_deleted() {
        let (db, ids) = seed(2).await;
        record_fork_head(&db.conn, ids[0], ids[1])
            .await
            .expect("fork");
        conversation_service::soft_delete(&db.conn, ids[1])
            .await
            .expect("soft delete the fork");

        // The source no longer advertises a fork that is out of the library…
        let source = lineage_for(&db.conn, ids[0]).await.expect("source lineage");
        assert!(
            source.forks.is_empty(),
            "a soft-deleted target would be a badge that jumps nowhere"
        );
        // …but the row itself survives, so the edge is only filtered, not gone.
        assert_eq!(
            fork_relation::Entity::find()
                .all(&db.conn)
                .await
                .expect("read edges")
                .len(),
            1,
            "soft deletion must not cascade the edge away"
        );

        // Symmetrically, a deleted SOURCE hides the "forked from" direction.
        let (db, ids) = seed(2).await;
        record_fork_head(&db.conn, ids[0], ids[1])
            .await
            .expect("fork");
        conversation_service::soft_delete(&db.conn, ids[0])
            .await
            .expect("soft delete the source");
        let target = lineage_for(&db.conn, ids[1]).await.expect("target lineage");
        assert!(target.forked_from.is_empty());
    }

    #[tokio::test]
    async fn a_conversation_cannot_fork_from_itself() {
        let (db, ids) = seed(1).await;
        let err = record_fork_head(&db.conn, ids[0], ids[0])
            .await
            .expect_err("self-fork must be rejected");
        assert!(matches!(err, DbError::Validation(_)), "got {err:?}");
    }
}
