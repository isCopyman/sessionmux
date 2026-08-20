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
