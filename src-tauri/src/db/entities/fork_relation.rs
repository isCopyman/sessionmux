use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// How a target Session was derived from a source Session. Mirrors the CHECK
/// constraint in migration `m20260820_000002`, and the `relationKind` of the
/// Session History RFC's `HistoryOperationResult`.
///
/// [`ForkRelationKind::ForkHead`] and [`ForkRelationKind::ForkAtMessage`]
/// both have write paths (ACP `session/fork` with or without an anchor).
/// [`ForkRelationKind::Handoff`] is reserved for a later slice — do not treat
/// a handoff row as reachable until its producer lands.
#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
#[serde(rename_all = "snake_case")]
pub enum ForkRelationKind {
    /// Whole-Session fork: the target continues the source's complete history.
    #[sea_orm(string_value = "fork_head")]
    ForkHead,
    /// Fork from a historical message; `anchor` carries the provider-native
    /// position.
    #[sea_orm(string_value = "fork_at_message")]
    ForkAtMessage,
    /// Not a native fork — a new Session in another Harness carrying material
    /// copied from the source.
    #[sea_orm(string_value = "handoff")]
    Handoff,
}

/// One directed lineage edge: `target` was produced from `source`.
///
/// Deliberately separate from `conversation.parent_id`, which means delegation
/// and stays NULL on a fork. See the migration's module doc.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "fork_relation")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// The Session that was forked FROM. Untouched by the fork itself.
    pub source_conversation_id: i32,
    /// The Session the fork produced.
    pub target_conversation_id: i32,
    pub relation_kind: ForkRelationKind,
    /// Provider-native message anchor as JSON, for kinds that fork at a
    /// position. NULL for `fork_head` — there is no position to record.
    pub anchor: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
