use std::collections::HashMap;

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait,
};

use crate::db::entities::conversation::{ConversationKind, ConversationStatus, CREATED_BY_USER};
use crate::db::entities::{collection_conversation, conversation, folder};
use crate::db::error::DbError;
use crate::models::{AgentType, DbConversationSummary};

pub async fn create(
    conn: &DatabaseConnection,
    folder_id: i32,
    agent_type: AgentType,
    title: Option<String>,
    git_branch: Option<String>,
) -> Result<conversation::Model, DbError> {
    create_with_source(
        conn,
        folder_id,
        agent_type,
        title,
        git_branch,
        CREATED_BY_USER,
    )
    .await
}

/// Mirror of [`create`] for rows a machine spawned: the caller names the
/// provenance (`created_by`) instead of defaulting to the user. Regular kind,
/// like [`create`].
pub async fn create_with_source(
    conn: &DatabaseConnection,
    folder_id: i32,
    agent_type: AgentType,
    title: Option<String>,
    git_branch: Option<String>,
    created_by: &str,
) -> Result<conversation::Model, DbError> {
    create_inner(
        conn,
        folder_id,
        agent_type,
        title,
        git_branch,
        ConversationKind::Regular,
        created_by,
    )
    .await
}

/// Mirror of [`create`] for folderless chat-mode conversations: identical row
/// shape but `kind = 'chat'`, so the sidebar routes the row to its flat "Chat"
/// section. Callers must pair it with the hidden chat folder created in the
/// same flow (`create_chat_conversation_core`).
pub async fn create_chat(
    conn: &DatabaseConnection,
    folder_id: i32,
    agent_type: AgentType,
    title: Option<String>,
    git_branch: Option<String>,
) -> Result<conversation::Model, DbError> {
    create_inner(
        conn,
        folder_id,
        agent_type,
        title,
        git_branch,
        ConversationKind::Chat,
        CREATED_BY_USER,
    )
    .await
}

async fn create_inner(
    conn: &DatabaseConnection,
    folder_id: i32,
    agent_type: AgentType,
    title: Option<String>,
    git_branch: Option<String>,
    kind: ConversationKind,
    created_by: &str,
) -> Result<conversation::Model, DbError> {
    let at_str = serde_json::to_value(agent_type)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    let now = Utc::now();
    let model = conversation::ActiveModel {
        id: NotSet,
        folder_id: Set(folder_id),
        title: Set(title),
        title_locked: Set(false),
        agent_type: Set(at_str),
        status: Set(conversation::ConversationStatus::InProgress),
        kind: Set(kind),
        model: Set(None),
        git_branch: Set(git_branch),
        external_id: Set(None),
        parent_id: Set(None),
        parent_tool_use_id: Set(None),
        delegation_call_id: Set(None),
        message_count: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
        deleted_at: Set(None),
        archived_at: Set(None),
        pinned_at: Set(None),
        origin_cwd: Set(None),
        harness_internal: Set(false),
        codeg_owned: Set(true),
        created_by: Set(created_by.to_string()),
        preferred_mode_id: Set(None),
        preferred_config_values: Set(None),
    };
    Ok(model.insert(conn).await?)
}

pub async fn update_status(
    conn: &DatabaseConnection,
    conversation_id: i32,
    status: conversation::ConversationStatus,
) -> Result<(), DbError> {
    let conv = conversation::Entity::find_by_id(conversation_id)
        .one(conn)
        .await?
        .ok_or_else(|| DbError::Migration(format!("Conversation not found: {conversation_id}")))?;
    let mut active: conversation::ActiveModel = conv.into();
    active.status = Set(status);
    active.updated_at = Set(Utc::now());
    active.update(conn).await?;
    Ok(())
}

/// Conditional status transition (CAS): write `new_status` only if the row's
/// current `status` equals `expected`. Returns `true` when the row was
/// updated. Used by the lifecycle subscriber on disconnect/error so a
/// concurrent user-driven `completed` (or a prior `pending_review` from
/// `TurnComplete`) cannot be silently overwritten.
pub async fn update_status_if(
    conn: &DatabaseConnection,
    conversation_id: i32,
    expected: conversation::ConversationStatus,
    new_status: conversation::ConversationStatus,
) -> Result<bool, DbError> {
    use sea_orm::sea_query::Expr;
    let result = conversation::Entity::update_many()
        .col_expr(conversation::Column::Status, Expr::value(new_status))
        .col_expr(conversation::Column::UpdatedAt, Expr::value(Utc::now()))
        .filter(conversation::Column::Id.eq(conversation_id))
        .filter(conversation::Column::Status.eq(expected))
        .exec(conn)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Manual rename: set the title AND lock it. Once locked, the per-turn
/// auto-title backfill ([`refresh_auto_title`]) leaves this row alone, so the
/// user's hand-picked name survives every subsequent session-file parse.
pub async fn update_title(
    conn: &DatabaseConnection,
    conversation_id: i32,
    title: String,
) -> Result<(), DbError> {
    let conv = conversation::Entity::find_by_id(conversation_id)
        .one(conn)
        .await?
        .ok_or_else(|| DbError::Migration(format!("Conversation not found: {conversation_id}")))?;
    let mut active: conversation::ActiveModel = conv.into();
    active.title = Set(Some(title));
    active.title_locked = Set(true);
    active.updated_at = Set(Utc::now());
    active.update(conn).await?;
    Ok(())
}

/// Atomically apply a Host-Control-style manual rename within one execution
/// folder. The predicate is evaluated by the database together with the write,
/// so a concurrent delete or move cannot produce a successful but invisible
/// rename. Internal loop/delegate rows are intentionally excluded.
pub async fn update_title_if_live_in_folder(
    conn: &DatabaseConnection,
    conversation_id: i32,
    folder_id: i32,
    title: String,
) -> Result<bool, DbError> {
    use sea_orm::sea_query::Expr;
    let result = conversation::Entity::update_many()
        .col_expr(conversation::Column::Title, Expr::value(Some(title)))
        .col_expr(conversation::Column::TitleLocked, Expr::value(true))
        .col_expr(conversation::Column::UpdatedAt, Expr::value(Utc::now()))
        .filter(conversation::Column::Id.eq(conversation_id))
        .filter(conversation::Column::FolderId.eq(folder_id))
        .filter(conversation::Column::DeletedAt.is_null())
        .filter(
            sea_orm::Condition::any()
                .add(conversation::Column::Kind.eq(ConversationKind::Regular))
                .add(conversation::Column::Kind.eq(ConversationKind::Chat)),
        )
        .filter(conversation::Column::ParentId.is_null())
        .exec(conn)
        .await?;
    Ok(result.rows_affected == 1)
}

/// Persist the ordinary-projection hide flag without treating it as activity.
/// Returns `true` when the stored value changed.
pub async fn set_harness_internal(
    conn: &DatabaseConnection,
    conversation_id: i32,
    harness_internal: bool,
) -> Result<bool, DbError> {
    use sea_orm::sea_query::Expr;
    let result = conversation::Entity::update_many()
        .col_expr(
            conversation::Column::HarnessInternal,
            Expr::value(harness_internal),
        )
        .filter(conversation::Column::Id.eq(conversation_id))
        .filter(conversation::Column::HarnessInternal.ne(harness_internal))
        .exec(conn)
        .await?;
    Ok(result.rows_affected == 1)
}

/// Auto-derive counterpart to [`update_title`]: write `title` ONLY when the row
/// is not user-locked and the value actually changed. Never sets `title_locked`
/// (the title stays eligible for future auto-refreshes, e.g. when an agent like
/// OpenCode regenerates its own session title) and deliberately does NOT bump
/// `updated_at` — a title backfill is metadata, not user activity, so it must
/// not float the row to the top of a recency-sorted sidebar. Returns `true`
/// when a row was written so the caller can broadcast a sidebar upsert.
///
/// Implemented as a single conditional UPDATE (`... WHERE id = ? AND
/// title_locked = false AND (title IS NULL OR title <> ?)`) so the lock/equality
/// checks and the write are atomic: a manual rename ([`update_title`], which
/// sets `title_locked = true`) that lands between a would-be read and the write
/// can never be clobbered, because the lock predicate is re-evaluated at write
/// time by the database. A non-existent row simply matches nothing (`false`).
pub async fn refresh_auto_title(
    conn: &DatabaseConnection,
    conversation_id: i32,
    title: String,
) -> Result<bool, DbError> {
    use sea_orm::sea_query::Expr;
    let title = title.trim();
    if title.is_empty() {
        return Ok(false);
    }
    let res = conversation::Entity::update_many()
        .col_expr(conversation::Column::Title, Expr::value(title))
        .filter(conversation::Column::Id.eq(conversation_id))
        .filter(conversation::Column::TitleLocked.eq(false))
        .filter(
            sea_orm::Condition::any()
                .add(conversation::Column::Title.is_null())
                .add(conversation::Column::Title.ne(title)),
        )
        .exec(conn)
        .await?;
    Ok(res.rows_affected > 0)
}

/// Conditionally adopt one title Codex's `session_index.jsonl` reported for a
/// conversation, as part of [`refresh_codex_auto_titles`].
///
/// Every field used to select `candidate` is re-checked in the UPDATE, plus the
/// title `candidate` was observed to have. This prevents a delayed write from
/// landing after the row was re-pointed to a different Codex session, deleted,
/// manually renamed, moved into a deleted folder, or flipped to a kind/harness
/// state `list_all` would no longer surface — every field mutable outside this
/// function is covered, mirroring [`refresh_auto_title`]'s single-conditional-
/// UPDATE approach so the whole identity check and the write are atomic.
async fn refresh_codex_auto_title_candidate(
    conn: &DatabaseConnection,
    candidate: &conversation::Model,
    title: &str,
) -> Result<bool, DbError> {
    use sea_orm::sea_query::{Expr, Query};

    let title = title.trim();
    let Some(external_id) = candidate.external_id.as_deref() else {
        return Ok(false);
    };
    if title.is_empty() || candidate.title.as_deref() == Some(title) {
        return Ok(false);
    }

    let old_title = match candidate.title.as_deref() {
        Some(old) => conversation::Column::Title.eq(old),
        None => conversation::Column::Title.is_null(),
    };
    let res = conversation::Entity::update_many()
        .col_expr(conversation::Column::Title, Expr::value(title))
        .filter(conversation::Column::Id.eq(candidate.id))
        .filter(conversation::Column::AgentType.eq(AgentType::Codex.as_wire().into_owned()))
        .filter(conversation::Column::ExternalId.eq(external_id))
        .filter(conversation::Column::DeletedAt.is_null())
        .filter(conversation::Column::TitleLocked.eq(false))
        .filter(conversation::Column::Kind.ne(ConversationKind::Loop))
        .filter(conversation::Column::HarnessInternal.eq(false))
        .filter(
            conversation::Column::FolderId.in_subquery(
                Query::select()
                    .column(folder::Column::Id)
                    .from(folder::Entity)
                    .and_where(folder::Column::DeletedAt.is_null())
                    .to_owned(),
            ),
        )
        .filter(old_title)
        .exec(conn)
        .await?;
    Ok(res.rows_affected > 0)
}

/// Refresh every live, unlocked Codex conversation whose external session id
/// has a title in `titles` (Codex's `session_index.jsonl`, read by
/// [`crate::parsers::codex::CodexParser::session_index_titles`]). Called from
/// `list_all_conversations_core` so a rename made in another Codex client
/// reaches the sidebar/@-panel/workspace list without the user first opening
/// that conversation's detail view (the only other path that currently adopts
/// it, via the per-turn auto-title backfill).
///
/// Candidate selection is chunked below SQLite's bound-variable limit and
/// scoped to what [`list_all`] would actually return (live, unlocked, not a
/// loop run, not harness-internal, in a non-deleted folder) — every refreshed
/// id is broadcast as a sidebar upsert, and refreshing a row this query would
/// never return would push a row `list_all` deliberately hides into every
/// client's sidebar until its next refetch. Each candidate's write re-checks
/// the same scope via [`refresh_codex_auto_title_candidate`], so a change
/// landing between selection and write can never be clobbered or leaked.
///
/// This is a best-effort reconciliation: a failed chunk or row is logged and
/// skipped rather than failing the caller's list read. Converged rows issue no
/// UPDATE and, like [`refresh_auto_title`], a title refresh never bumps
/// `updated_at`. Returns the ids that were actually rewritten, so the caller
/// can broadcast and propagate exactly those.
pub(crate) async fn refresh_codex_auto_titles(
    conn: &DatabaseConnection,
    titles: &HashMap<String, String>,
) -> Vec<i32> {
    const SQLITE_TITLE_QUERY_CHUNK_SIZE: usize = 500;

    let external_ids: Vec<String> = titles
        .iter()
        .filter(|(_, title)| !title.trim().is_empty())
        .map(|(external_id, _)| external_id.clone())
        .collect();
    if external_ids.is_empty() {
        return Vec::new();
    }

    let mut refreshed = Vec::new();
    for external_id_chunk in external_ids.chunks(SQLITE_TITLE_QUERY_CHUNK_SIZE) {
        use sea_orm::sea_query::Query;

        let candidates = match conversation::Entity::find()
            .filter(conversation::Column::AgentType.eq(AgentType::Codex.as_wire().into_owned()))
            .filter(conversation::Column::ExternalId.is_in(external_id_chunk.iter().cloned()))
            .filter(conversation::Column::DeletedAt.is_null())
            .filter(conversation::Column::TitleLocked.eq(false))
            .filter(conversation::Column::Kind.ne(ConversationKind::Loop))
            .filter(conversation::Column::HarnessInternal.eq(false))
            .filter(
                conversation::Column::FolderId.in_subquery(
                    Query::select()
                        .column(folder::Column::Id)
                        .from(folder::Entity)
                        .and_where(folder::Column::DeletedAt.is_null())
                        .to_owned(),
                ),
            )
            .order_by_asc(conversation::Column::Id)
            .all(conn)
            .await
        {
            Ok(candidates) => candidates,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    session_count = external_id_chunk.len(),
                    "failed to select Codex title refresh candidates; skipping chunk"
                );
                continue;
            }
        };

        for candidate in candidates {
            let Some(external_id) = candidate.external_id.as_deref() else {
                continue;
            };
            let Some(title) = titles.get(external_id) else {
                continue;
            };
            match refresh_codex_auto_title_candidate(conn, &candidate, title).await {
                Ok(true) => refreshed.push(candidate.id),
                Ok(false) => {}
                Err(error) => tracing::warn!(
                    error = %error,
                    conversation_id = candidate.id,
                    external_id,
                    "failed to refresh Codex title candidate; skipping row"
                ),
            }
        }
    }

    refreshed
}

/// Lock a row's title WITHOUT rewriting it. For a conversation whose name was
/// typed by the user somewhere else — a work task's title, an automation's name
/// — the seed passed to [`create`] already IS the name; all that's missing is
/// the promise that [`refresh_auto_title`] will keep its hands off it, exactly
/// as it does after a manual rename ([`update_title`]).
///
/// Without this, a task-launched session drifts to whatever the agent's session
/// file parses to — for agents with no title of their own, the first line of the
/// composed prompt — and the board's own name for the work is lost (issue #495).
///
/// Deliberately does NOT touch `title` or bump `updated_at`: like
/// [`refresh_auto_title`], flipping this flag is metadata, not user activity,
/// and must not float the row to the top of a recency-sorted sidebar. A
/// non-existent row simply matches nothing.
///
/// Callers must lock BEFORE broadcasting the row's first sidebar upsert: the
/// conversation id is not knowable to any client until that broadcast, so
/// locking first makes an auto-title backfill on this row impossible rather
/// than merely unlikely.
pub async fn lock_title(conn: &DatabaseConnection, conversation_id: i32) -> Result<(), DbError> {
    use sea_orm::sea_query::Expr;
    conversation::Entity::update_many()
        .col_expr(conversation::Column::TitleLocked, Expr::value(true))
        .filter(conversation::Column::Id.eq(conversation_id))
        .exec(conn)
        .await?;
    Ok(())
}

/// Rename a locked title when its OWNER was renamed: write `new_title` only if
/// the row still carries `expected`. Used when a work task is retitled — the
/// session it produced should follow the card it came from, but only while the
/// two are still in sync.
///
/// The equality guard is the whole point. Both a task-derived title and a hand
/// picked one leave `title_locked = true`, so the flag alone cannot tell them
/// apart; `expected` (the task's PREVIOUS title) can. If the user renamed the
/// conversation themselves, nothing matches and not a byte is written. Same
/// optimistic shape as [`refresh_auto_title`]: one conditional UPDATE, so the
/// comparison happens at write time in the database and a rename landing in
/// between can never be clobbered. Returns `true` when a row was written.
///
/// Leaves `title_locked` alone (already true for every row this can match) and,
/// like its siblings, does not bump `updated_at`.
pub async fn retitle_if_unchanged(
    conn: &DatabaseConnection,
    conversation_id: i32,
    expected: &str,
    new_title: &str,
) -> Result<bool, DbError> {
    use sea_orm::sea_query::Expr;
    let new_title = new_title.trim();
    if new_title.is_empty() || new_title == expected {
        return Ok(false);
    }
    let res = conversation::Entity::update_many()
        .col_expr(conversation::Column::Title, Expr::value(new_title))
        .filter(conversation::Column::Id.eq(conversation_id))
        .filter(conversation::Column::Title.eq(expected))
        .exec(conn)
        .await?;
    Ok(res.rows_affected > 0)
}

/// Adopt an imported conversation's newest activity from its agent-side
/// transcript: stamp `updated_at` with the session file's own last-activity
/// time (never `now()` — the scan is not the activity) and re-sync
/// `message_count`. Returns `true` when a row was written, so the caller can
/// broadcast a sidebar upsert.
///
/// This is the counterpart to [`refresh_auto_title`] for the OTHER half of a
/// re-import: a session the user kept working on in the agent's own CLI after
/// importing it into codeg. Its title may be unchanged while its activity is
/// hours newer, and `updated_at` is what the sidebar's "recently updated"
/// ordering (and the relative timestamp on each row) reads.
///
/// One conditional UPDATE, guarded so it can never do harm:
/// * `updated_at < activity_at` — strictly forward. A re-import can never move
///   a conversation backwards or re-order an unchanged one, re-running is a
///   no-op, and a turn running live in codeg (which stamps `updated_at =
///   now()`) wins over a transcript tail parsed moments earlier.
/// * `deleted_at IS NULL` — a soft-deleted conversation stays deleted; agent
///   activity must not half-resurrect an invisible row.
/// * `parent_id IS NULL` — delegation children are not sidebar rows and are
///   maintained by the delegation flow.
///
/// Everything else the user owns is left alone: `created_at`, `title` (and its
/// lock), `pinned_at`, `status`, and folder placement.
pub async fn refresh_external_activity(
    conn: &DatabaseConnection,
    conversation_id: i32,
    activity_at: chrono::DateTime<Utc>,
    message_count: u32,
) -> Result<bool, DbError> {
    use sea_orm::sea_query::Expr;
    let res = conversation::Entity::update_many()
        .col_expr(conversation::Column::UpdatedAt, Expr::value(activity_at))
        .col_expr(
            conversation::Column::MessageCount,
            Expr::value(message_count as i32),
        )
        .filter(conversation::Column::Id.eq(conversation_id))
        .filter(conversation::Column::DeletedAt.is_null())
        .filter(conversation::Column::ParentId.is_null())
        .filter(conversation::Column::UpdatedAt.lt(activity_at))
        .exec(conn)
        .await?;
    Ok(res.rows_affected > 0)
}

/// Pin or unpin a conversation. Sets `pinned_at = now()` when pinning, `NULL`
/// when unpinning. Only the `pinned_at` column is written — `updated_at` is
/// deliberately left untouched (SeaORM updates only the `Set` field), because
/// pinning is a view preference, not conversation activity, and must not float
/// the row to the top of a recency-sorted sidebar (same reasoning as
/// [`refresh_auto_title`]). The sidebar's "Pinned" section orders by `pinned_at`
/// descending, so a freshly pinned conversation jumps to the top.
pub async fn update_pin(
    conn: &DatabaseConnection,
    conversation_id: i32,
    pinned: bool,
) -> Result<(), DbError> {
    let conv = conversation::Entity::find_by_id(conversation_id)
        .one(conn)
        .await?
        .ok_or_else(|| DbError::Migration(format!("Conversation not found: {conversation_id}")))?;
    let mut active: conversation::ActiveModel = conv.into();
    active.pinned_at = Set(pinned.then(Utc::now));
    active.update(conn).await?;
    Ok(())
}

/// Archive or restore a conversation without changing its progress status,
/// activity timestamp, open workbench references, or underlying transcript.
pub async fn update_archive(
    conn: &DatabaseConnection,
    conversation_id: i32,
    archived: bool,
) -> Result<(), DbError> {
    let conv = conversation::Entity::find_by_id(conversation_id)
        .filter(conversation::Column::DeletedAt.is_null())
        .one(conn)
        .await?
        .ok_or_else(|| DbError::Migration(format!("Conversation not found: {conversation_id}")))?;
    let mut active: conversation::ActiveModel = conv.into();
    active.archived_at = Set(archived.then(Utc::now));
    active.update(conn).await?;
    Ok(())
}

/// Unconditionally overwrite `external_id`, guarded only on `deleted_at IS
/// NULL`. This is NOT the ACP session-binding write path — see
/// [`bind_external_id`] for that — and must not be reached for a
/// `SessionStarted`-driven rebind.
///
/// Its one remaining caller
/// (`commands::conversations::get_folder_conversation_core`) uses it for a
/// narrower, different operation: an OpenClaw/Cline/Gemini row whose stored
/// `external_id` no longer resolves to any parseable session file gets
/// re-matched by folder + start time, and the id the parser actually
/// recognizes is backfilled here. That is a correction of a stale label for
/// the SAME conversation, not a takeover of a live one — running it through
/// `bind_external_id`'s split logic would be actively wrong: these are all
/// built-in agent types (no `continues_from` chain can ever justify the
/// change), so every correction would be misjudged as an A1 takeover and
/// spawn a needless, permanent ghost row carrying the old, already-dead id.
///
/// Kept `pub(crate)` rather than `pub` so this stays a deliberate, narrow
/// exception rather than something a future ACP-binding call site reaches
/// for by habit.
pub(crate) async fn update_external_id(
    conn: &DatabaseConnection,
    conversation_id: i32,
    external_id: String,
) -> Result<(), DbError> {
    use sea_orm::sea_query::Expr;
    conversation::Entity::update_many()
        .col_expr(conversation::Column::ExternalId, Expr::value(external_id))
        .col_expr(conversation::Column::UpdatedAt, Expr::value(Utc::now()))
        .filter(conversation::Column::Id.eq(conversation_id))
        .filter(conversation::Column::DeletedAt.is_null())
        .exec(conn)
        .await?;
    Ok(())
}

/// Outcome of a successful [`bind_external_id`] call.
pub struct BindOutcome {
    /// `Some(row_id)` when the row's PREVIOUS `external_id` had to be
    /// preserved onto a different (possibly pre-existing) row before this one
    /// could be overwritten — an unrelated, still-referenced session that
    /// would otherwise become permanently unreachable (ACP binding audit,
    /// scenario A1). The caller must broadcast an upsert for `row_id` too, or
    /// the preserved conversation stays invisible until an unrelated refresh.
    ///
    /// `None` when the write needed no split: the row already carried this
    /// exact value, had none yet, or its previous value is part of the new
    /// value's OWN continuation chain (a memory-less custom agent's restart,
    /// not a takeover — scenario A2).
    pub preserved_conversation_id: Option<i32>,
}

/// Bind `external_id` (an agent's native session id) onto `conversation_id` —
/// the write path for every ACP `SessionStarted`-driven rebind, and the ONLY
/// one that may see a genuine session takeover. Supersedes the old
/// `update_external_id` for that purpose: `update_external_id` was a blind
/// conditional UPDATE that never read the row's previous value, so a
/// reconnect that lost its session id, or a `session/load` fallback that
/// handed a DIFFERENT session id to an already-bound row, silently
/// overwrote the old value — and because the sidebar is a pure DB-row
/// projection that never scans an agent's own history store, the old
/// session then belonged to no row at all and simply vanished from the
/// user's view (see `docs/session-workbench/ACP-BINDING-AUDIT-2026-08-19.zh-CN.md`,
/// scenario A1). `update_external_id` itself still exists, narrowed to
/// `pub(crate)`, for exactly one unrelated caller that is NOT a takeover —
/// see its own doc for why that one must NOT go through the split logic
/// below.
///
/// Shaped like `persist_fork_outcome`'s "claim → read under lock → decide →
/// write" transaction (`acp::manager::ConnectionManager`): the opening
/// statement is a value-preserving self-assignment on `conversation_id` so
/// SQLite grants the writer lock immediately instead of taking a deferred read
/// snapshot it might later fail to promote (see that function's doc for the
/// full `SQLITE_BUSY_SNAPSHOT` rationale). Every read this function does
/// afterward — the conflict check and the continuation check below — is then
/// guaranteed not to be superseded by a concurrent writer before this
/// transaction commits, so checking-then-acting on them is race-free without
/// needing a second round-trip.
///
/// Three outcomes:
/// * **No-op** (`Ok(BindOutcome { preserved_conversation_id: None })`) — the
///   row already carries this exact `external_id`, has none yet, or its
///   previous value is in `external_id`'s own continuation chain (A2: a
///   memory-less custom agent forgot its session across a restart; codeg
///   opened a fresh agent session for the SAME conversation, and
///   [`crate::acp_transcript::continued_session_ids`] links the two ids).
///   Also the outcome for a soft-deleted or missing row — no live row to
///   bind, and every caller already treats that as nothing to do, mirroring
///   the old function's guard.
/// * **Split** (`Ok(BindOutcome { preserved_conversation_id: Some(id) })`) —
///   the row's previous `external_id` is unrelated to the new one. It is
///   preserved onto its own row FIRST (reusing one that already carries it,
///   if an earlier call or a fork already split it out — see
///   `already_preserved` below), and only then is `conversation_id` written.
/// * **Refused** (`Err(DbError::ExternalIdTaken)`) — `external_id` is already
///   held by a DIFFERENT row. Checked across BOTH live and soft-deleted rows:
///   the unique `(external_id, agent_type)` index carries no `deleted_at`
///   predicate, so a soft-deleted row still occupies the slot. This is
///   permanent, not a transient lock conflict — retrying does not help,
///   matching the naming rationale documented on [`DbError::ExternalIdTaken`].
pub async fn bind_external_id(
    conn: &DatabaseConnection,
    conversation_id: i32,
    external_id: String,
) -> Result<BindOutcome, DbError> {
    use sea_orm::sea_query::Expr;

    /// Transaction-local result, kept distinct from [`DbError`] so a genuine
    /// unique-index takeover can never be collapsed into the same
    /// `TransactionError` arm as a `sea_orm::DbErr` surfaced by `?` inside the
    /// closure (a real transient/IO failure) — the two must stay
    /// distinguishable, or a permanent refusal would look retryable.
    enum BindAttempt {
        Bound(Option<i32>),
        Refused,
    }

    let external_id_for_error = external_id.clone();
    let attempt = conn
        .transaction::<_, BindAttempt, sea_orm::DbErr>(move |txn| {
            let external_id = external_id.clone();
            Box::pin(async move {
                let now = chrono::Utc::now();

                // WRITE FIRST — claims the writer lock before anything below
                // is read. See the fn doc / `persist_fork_outcome`.
                let claimed = conversation::Entity::update_many()
                    .col_expr(
                        conversation::Column::UpdatedAt,
                        Expr::col(conversation::Column::UpdatedAt).into(),
                    )
                    .filter(conversation::Column::Id.eq(conversation_id))
                    .filter(conversation::Column::DeletedAt.is_null())
                    .exec(txn)
                    .await?;
                if claimed.rows_affected == 0 {
                    // Soft-deleted or missing row: mirror the old
                    // `update_external_id` no-op-on-deleted guard.
                    return Ok(BindAttempt::Bound(None));
                }

                // Read UNDER the write lock — see the fn doc.
                let current = conversation::Entity::find_by_id(conversation_id)
                    .one(txn)
                    .await?
                    .ok_or_else(|| {
                        sea_orm::DbErr::Custom(format!("conversation {conversation_id} not found"))
                    })?;

                if current.external_id.as_deref() == Some(external_id.as_str()) {
                    return Ok(BindAttempt::Bound(None)); // already correct
                }

                // Refuse BEFORE touching anything else: some OTHER row (live
                // or soft-deleted — no `deleted_at` filter, matching the real
                // unique index) already holds this exact (external_id,
                // agent_type) pair.
                let conflict = conversation::Entity::find()
                    .filter(conversation::Column::ExternalId.eq(external_id.clone()))
                    .filter(conversation::Column::AgentType.eq(current.agent_type.clone()))
                    .filter(conversation::Column::Id.ne(conversation_id))
                    .one(txn)
                    .await?;
                if conflict.is_some() {
                    return Ok(BindAttempt::Refused);
                }

                let previous_external_id = current.external_id.clone();
                let is_continuation = match previous_external_id.as_deref() {
                    None => true,
                    Some(prev) => {
                        let agent_type = parse_agent_type(&current.agent_type);
                        crate::acp_transcript::continued_session_ids(agent_type, &external_id)
                            .contains(prev)
                    }
                };

                if is_continuation {
                    conversation::Entity::update_many()
                        .col_expr(
                            conversation::Column::ExternalId,
                            Expr::value(Some(external_id.clone())),
                        )
                        .col_expr(conversation::Column::UpdatedAt, Expr::value(now))
                        .filter(conversation::Column::Id.eq(conversation_id))
                        .filter(conversation::Column::DeletedAt.is_null())
                        .exec(txn)
                        .await?;
                    return Ok(BindAttempt::Bound(None));
                }

                // Split required: `prev` is unrelated to `external_id` and
                // would otherwise become unreachable the instant this row's
                // column moves on. Preserve it onto its own row FIRST —
                // reusing one that already carries it (an earlier, possibly
                // interrupted split, or an unrelated fork already claimed
                // it) rather than inserting a duplicate that would collide
                // with the same unique index.
                let prev =
                    previous_external_id.expect("is_continuation's None arm returned above");
                let already_preserved = conversation::Entity::find()
                    .filter(conversation::Column::ExternalId.eq(prev.clone()))
                    .filter(conversation::Column::AgentType.eq(current.agent_type.clone()))
                    .filter(conversation::Column::Id.ne(conversation_id))
                    .one(txn)
                    .await?;

                let preserved_id = match already_preserved {
                    Some(existing) => existing.id,
                    None => {
                        // A row that was InProgress had a live turn running
                        // under `prev`; that turn will never complete now
                        // that the connection has moved on to `external_id`,
                        // so it is Cancelled — exactly the mapping
                        // `handle_terminal_event` (`acp::lifecycle`) already
                        // applies on an ordinary disconnect. Any other status
                        // already reflects a settled conversation (a normal
                        // end, a refusal, an explicit cancel) and is carried
                        // over unchanged rather than relabeled.
                        let preserved_status = if current.status == ConversationStatus::InProgress
                        {
                            ConversationStatus::Cancelled
                        } else {
                            current.status.clone()
                        };
                        let preserved = conversation::ActiveModel {
                            id: NotSet,
                            folder_id: Set(current.folder_id),
                            title: Set(current.title.clone()),
                            title_locked: Set(current.title_locked),
                            agent_type: Set(current.agent_type.clone()),
                            status: Set(preserved_status),
                            kind: Set(current.kind.clone()),
                            model: Set(current.model.clone()),
                            git_branch: Set(current.git_branch.clone()),
                            external_id: Set(Some(prev.clone())),
                            parent_id: Set(None),
                            parent_tool_use_id: Set(None),
                            delegation_call_id: Set(None),
                            message_count: Set(current.message_count),
                            created_at: Set(current.created_at),
                            updated_at: Set(now),
                            deleted_at: Set(None),
                            archived_at: Set(None),
                            pinned_at: Set(None),
                            origin_cwd: Set(current.origin_cwd.clone()),
                            harness_internal: Set(current.harness_internal),
                            codeg_owned: Set(current.codeg_owned),
                            created_by: Set(current.created_by.clone()),
                            preferred_mode_id: Set(current.preferred_mode_id.clone()),
                            preferred_config_values: Set(current.preferred_config_values.clone()),
                        };
                        let inserted = preserved.insert(txn).await?;

                        // Carry Collection membership across too — this
                        // preserved row IS the same conversation the user was
                        // already working with, only re-homed onto a new row
                        // id; it must not silently fall out of a saved
                        // Workbench. Mirrors `persist_fork_outcome`.
                        if let Some(membership) =
                            collection_conversation::Entity::find_by_id(conversation_id)
                                .one(txn)
                                .await?
                        {
                            collection_conversation::ActiveModel {
                                conversation_id: Set(inserted.id),
                                collection_id: Set(membership.collection_id),
                                created_at: Set(now),
                                updated_at: Set(now),
                            }
                            .insert(txn)
                            .await?;
                        }

                        inserted.id
                    }
                };

                // Only now, with the old session safely reachable elsewhere,
                // overwrite this row's binding.
                conversation::Entity::update_many()
                    .col_expr(
                        conversation::Column::ExternalId,
                        Expr::value(Some(external_id.clone())),
                    )
                    .col_expr(conversation::Column::UpdatedAt, Expr::value(now))
                    .filter(conversation::Column::Id.eq(conversation_id))
                    .filter(conversation::Column::DeletedAt.is_null())
                    .exec(txn)
                    .await?;

                Ok(BindAttempt::Bound(Some(preserved_id)))
            })
        })
        .await
        .map_err(|err| match err {
            sea_orm::TransactionError::Connection(e) => DbError::Database(e),
            sea_orm::TransactionError::Transaction(e) => DbError::Database(e),
        })?;

    match attempt {
        BindAttempt::Bound(preserved) => Ok(BindOutcome {
            preserved_conversation_id: preserved,
        }),
        BindAttempt::Refused => Err(DbError::ExternalIdTaken(format!(
            "external_id {external_id_for_error} is already bound to a different conversation"
        ))),
    }
}

/// Re-parent every live conversation of `from_folder_id` (a task worktree
/// folder about to be removed) onto the project folder, stamping `origin_cwd`
/// with the worktree's original path. History loading is external_id-driven and
/// unaffected; the Gemini/Cline/OpenClaw stale-id fallback matches on
/// `origin_cwd ?? folder.path`, which this preserves.
pub async fn reparent_folder_conversations(
    conn: &DatabaseConnection,
    from_folder_id: i32,
    to_folder_id: i32,
    origin_cwd: &str,
) -> Result<u64, DbError> {
    use sea_orm::sea_query::Expr;
    let res = conversation::Entity::update_many()
        .col_expr(conversation::Column::FolderId, Expr::value(to_folder_id))
        .col_expr(
            conversation::Column::OriginCwd,
            Expr::value(Some(origin_cwd.to_string())),
        )
        .col_expr(conversation::Column::UpdatedAt, Expr::value(Utc::now()))
        .filter(conversation::Column::FolderId.eq(from_folder_id))
        .filter(conversation::Column::DeletedAt.is_null())
        .exec(conn)
        .await?;
    Ok(res.rows_affected)
}

pub async fn soft_delete(conn: &DatabaseConnection, conversation_id: i32) -> Result<(), DbError> {
    let conv = conversation::Entity::find_by_id(conversation_id)
        .filter(conversation::Column::DeletedAt.is_null())
        .one(conn)
        .await?
        .ok_or_else(|| DbError::Migration(format!("Conversation not found: {conversation_id}")))?;
    let mut active: conversation::ActiveModel = conv.into();
    active.deleted_at = Set(Some(Utc::now()));
    active.update(conn).await?;
    Ok(())
}

fn parse_agent_type(s: &str) -> AgentType {
    match serde_json::from_value(serde_json::Value::String(s.to_string())) {
        Ok(at) => at,
        Err(_) => {
            // DB has a value the enum does not recognise (manual edit or removed variant).
            // Fall back to ClaudeCode so the row stays readable, but log so resume-as-wrong-agent
            // regressions are traceable.
            tracing::warn!(
                "[conversation_service] unknown agent_type {s:?} in DB, falling back to ClaudeCode"
            );
            AgentType::ClaudeCode
        }
    }
}

fn conv_to_summary(r: conversation::Model) -> DbConversationSummary {
    let status = serde_json::to_value(&r.status)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| format!("{:?}", r.status));
    DbConversationSummary {
        id: r.id,
        folder_id: r.folder_id,
        title: r.title,
        title_locked: r.title_locked,
        agent_type: parse_agent_type(&r.agent_type),
        status,
        kind: r.kind.clone(),
        model: r.model,
        git_branch: r.git_branch,
        external_id: r.external_id,
        message_count: r.message_count as u32,
        // Pure mapper: `child_count` is backfilled by `fill_child_counts` over
        // the returned set, never queried per-row here.
        child_count: 0,
        created_at: r.created_at,
        updated_at: r.updated_at,
        archived_at: r.archived_at,
        pinned_at: r.pinned_at,
        parent_id: r.parent_id,
        parent_tool_use_id: r.parent_tool_use_id,
        delegation_call_id: r.delegation_call_id,
        origin_cwd: r.origin_cwd,
        harness_internal: r.harness_internal,
        created_by: r.created_by,
    }
}

/// Backfill each summary's `child_count` with its number of direct, non-deleted
/// delegation children using ONE `GROUP BY` aggregate over the whole set (never
/// per-row — no N+1). `child_count > 0` iff `list_children` would return rows
/// (same `parent_id == id AND deleted_at IS NULL` predicate), so the sidebar
/// chevron neither expands to nothing nor hides a real subtree. No-op on an
/// empty slice (avoids an `IN ()`).
async fn fill_child_counts(
    conn: &DatabaseConnection,
    summaries: &mut [DbConversationSummary],
) -> Result<(), DbError> {
    if summaries.is_empty() {
        return Ok(());
    }
    let ids: Vec<i32> = summaries.iter().map(|s| s.id).collect();
    let pairs: Vec<(Option<i32>, i64)> = conversation::Entity::find()
        .select_only()
        .column(conversation::Column::ParentId)
        .column_as(conversation::Column::Id.count(), "cnt")
        .filter(conversation::Column::ParentId.is_in(ids))
        .filter(conversation::Column::DeletedAt.is_null())
        .group_by(conversation::Column::ParentId)
        .into_tuple()
        .all(conn)
        .await?;
    let mut counts: std::collections::HashMap<i32, u32> =
        std::collections::HashMap::with_capacity(pairs.len());
    for (parent_id, cnt) in pairs {
        if let Some(pid) = parent_id {
            counts.insert(pid, cnt.max(0) as u32);
        }
    }
    for s in summaries.iter_mut() {
        s.child_count = counts.get(&s.id).copied().unwrap_or(0);
    }
    Ok(())
}

pub async fn get_by_id(
    conn: &DatabaseConnection,
    conversation_id: i32,
) -> Result<DbConversationSummary, DbError> {
    let conv = conversation::Entity::find_by_id(conversation_id)
        .filter(conversation::Column::DeletedAt.is_null())
        .one(conn)
        .await?
        .ok_or_else(|| DbError::Migration(format!("Conversation not found: {conversation_id}")))?;

    let mut summary = conv_to_summary(conv);
    fill_child_counts(conn, std::slice::from_mut(&mut summary)).await?;
    Ok(summary)
}

/// Look up a child conversation by its `delegation_call_id` (the broker's
/// `task_id`). Returns `Ok(None)` when no row matches — used by the broker's
/// `ChildStatusLookup` DB fallback to recover a delegation task's terminal
/// status after its in-memory result was evicted from the completed-cache.
/// Unlike [`get_by_id`] this never errors hard on "not found": a missing row
/// is a legitimate "unknown task" answer.
pub async fn get_by_delegation_call_id(
    conn: &DatabaseConnection,
    delegation_call_id: &str,
) -> Result<Option<DbConversationSummary>, DbError> {
    let conv = conversation::Entity::find()
        .filter(conversation::Column::DelegationCallId.eq(delegation_call_id))
        .filter(conversation::Column::DeletedAt.is_null())
        .one(conn)
        .await?;
    Ok(conv.map(conv_to_summary))
}

pub async fn list_by_folder(
    conn: &DatabaseConnection,
    folder_id: i32,
    agent_type: Option<AgentType>,
    search: Option<String>,
    sort_by: Option<String>,
    status: Option<String>,
) -> Result<Vec<DbConversationSummary>, DbError> {
    let mut query = conversation::Entity::find()
        .filter(conversation::Column::FolderId.eq(folder_id))
        .filter(conversation::Column::DeletedAt.is_null())
        .filter(conversation::Column::HarnessInternal.eq(false));

    // Filter by agent_type
    if let Some(ref at) = agent_type {
        let at_str = serde_json::to_value(at)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        query = query.filter(conversation::Column::AgentType.eq(at_str));
    }

    // Search by title
    if let Some(ref s) = search {
        if !s.is_empty() {
            query = query.filter(conversation::Column::Title.contains(s));
        }
    }

    // Filter by status
    if let Some(ref st) = status {
        if let Ok(status_enum) = serde_json::from_value::<conversation::ConversationStatus>(
            serde_json::Value::String(st.clone()),
        ) {
            query = query.filter(conversation::Column::Status.eq(status_enum));
        }
    }

    // Sort
    query = match sort_by.as_deref() {
        Some("oldest") => query.order_by_asc(conversation::Column::CreatedAt),
        _ => query.order_by_desc(conversation::Column::CreatedAt),
    };

    let rows = query.all(conn).await?;

    let mut summaries: Vec<DbConversationSummary> = rows.into_iter().map(conv_to_summary).collect();
    fill_child_counts(conn, &mut summaries).await?;

    Ok(summaries)
}

/// List conversations across folders. When `folder_ids` is `None`, queries all
/// When `folder_ids` is provided, results are scoped to that set. Otherwise
/// returns conversations across every non-deleted folder (open or not).
///
/// `include_children` controls visibility of delegation sub-sessions. When
/// `false` (the default for the top-level list), rows whose `parent_id` is
/// non-null are filtered out — they belong to their parent's tool-call view,
/// not the workspace conversation list. Rows with `kind = 'loop'` are always
/// excluded — they belong to the loops workbench. Harness-internal subagents
/// (`harness_internal = true`) are also excluded from this ordinary
/// projection; `get_by_id` still returns them.
#[allow(clippy::too_many_arguments)]
pub async fn list_all(
    conn: &DatabaseConnection,
    folder_ids: Option<Vec<i32>>,
    agent_type: Option<AgentType>,
    search: Option<String>,
    sort_by: Option<String>,
    status: Option<String>,
    archived: bool,
    include_children: bool,
) -> Result<Vec<DbConversationSummary>, DbError> {
    let mut query = conversation::Entity::find().filter(conversation::Column::DeletedAt.is_null());

    query = if archived {
        query.filter(conversation::Column::ArchivedAt.is_not_null())
    } else {
        query.filter(conversation::Column::ArchivedAt.is_null())
    };

    // Loop-engineering runs never surface in the workspace conversation list —
    // their entry point is the loops workbench.
    query = query.filter(conversation::Column::Kind.ne(ConversationKind::Loop));
    // Harness-internal subagents stay out of ordinary Session projections.
    query = query.filter(conversation::Column::HarnessInternal.eq(false));

    if !include_children {
        query = query.filter(conversation::Column::ParentId.is_null());
    }

    match folder_ids {
        Some(ids) if !ids.is_empty() => {
            query = query.filter(conversation::Column::FolderId.is_in(ids));
        }
        _ => {
            // Exclude conversations whose folder was soft-deleted.
            let active_folder_ids: Vec<i32> = folder::Entity::find()
                .filter(folder::Column::DeletedAt.is_null())
                .all(conn)
                .await?
                .into_iter()
                .map(|m| m.id)
                .collect();
            if active_folder_ids.is_empty() {
                return Ok(Vec::new());
            }
            query = query.filter(conversation::Column::FolderId.is_in(active_folder_ids));
        }
    }

    if let Some(ref at) = agent_type {
        let at_str = serde_json::to_value(at)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        query = query.filter(conversation::Column::AgentType.eq(at_str));
    }

    if let Some(ref s) = search {
        if !s.is_empty() {
            query = query.filter(conversation::Column::Title.contains(s));
        }
    }

    if let Some(ref st) = status {
        if let Ok(status_enum) = serde_json::from_value::<conversation::ConversationStatus>(
            serde_json::Value::String(st.clone()),
        ) {
            query = query.filter(conversation::Column::Status.eq(status_enum));
        }
    }

    query = match sort_by.as_deref() {
        Some("oldest") => query.order_by_asc(conversation::Column::UpdatedAt),
        _ => query.order_by_desc(conversation::Column::UpdatedAt),
    };

    let rows = query.all(conn).await?;
    let mut summaries: Vec<DbConversationSummary> = rows.into_iter().map(conv_to_summary).collect();
    fill_child_counts(conn, &mut summaries).await?;
    Ok(summaries)
}

/// List delegation children of a single parent conversation, newest first
/// (`created_at` DESC), matching the sidebar's newest-on-top ordering so a
/// freshly-spawned sub-agent surfaces right under its parent. The only other
/// consumer (`inject_delegation_meta`) keys these by `parent_tool_use_id` and
/// is order-agnostic. Returns rows where `parent_id == parent_conversation_id`.
/// Soft-deleted children are filtered out so a removed sub-session stays hidden
/// in the parent's tool-call view too.
pub async fn list_children(
    conn: &DatabaseConnection,
    parent_conversation_id: i32,
) -> Result<Vec<DbConversationSummary>, DbError> {
    let rows = conversation::Entity::find()
        .filter(conversation::Column::ParentId.eq(parent_conversation_id))
        .filter(conversation::Column::DeletedAt.is_null())
        .order_by_desc(conversation::Column::CreatedAt)
        // Explicit id tie-break so same-timestamp siblings are deterministic and
        // match the frontend re-sort (which tie-breaks id DESC): the raw fetch
        // snapshot and a live-inserted child then land in the same order.
        .order_by_desc(conversation::Column::Id)
        .all(conn)
        .await?;
    let mut summaries: Vec<DbConversationSummary> = rows.into_iter().map(conv_to_summary).collect();
    fill_child_counts(conn, &mut summaries).await?;
    Ok(summaries)
}

/// Per-Session selector preferences: the pinned ACP mode and configId →
/// valueId choices (model, thinking effort, …) this conversation should
/// reconnect with. A missing row or unparsable JSON degrades to "no pins".
pub async fn selector_prefs(
    conn: &DatabaseConnection,
    conversation_id: i32,
) -> Result<(Option<String>, std::collections::BTreeMap<String, String>), DbError> {
    let Some(row) = conversation::Entity::find_by_id(conversation_id)
        .one(conn)
        .await?
    else {
        return Ok((None, Default::default()));
    };
    let values = row
        .preferred_config_values
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default();
    Ok((row.preferred_mode_id, values))
}

/// Pin the Session's ACP mode. Like pinning, this is a view/launch preference:
/// it never bumps `updated_at`.
pub async fn set_selector_mode(
    conn: &DatabaseConnection,
    conversation_id: i32,
    mode_id: &str,
) -> Result<(), DbError> {
    let Some(row) = conversation::Entity::find_by_id(conversation_id)
        .one(conn)
        .await?
    else {
        return Ok(());
    };
    let mut active: conversation::ActiveModel = row.into();
    active.preferred_mode_id = Set(Some(mode_id.to_string()));
    active.updated_at = NotSet;
    active.update(conn).await?;
    Ok(())
}

/// Merge one selector choice into the Session's pinned set without disturbing
/// the others (changing the model must not drop a pinned thinking effort).
pub async fn merge_selector_config_value(
    conn: &DatabaseConnection,
    conversation_id: i32,
    config_id: &str,
    value_id: &str,
) -> Result<(), DbError> {
    let Some(row) = conversation::Entity::find_by_id(conversation_id)
        .one(conn)
        .await?
    else {
        return Ok(());
    };
    let mut values: std::collections::BTreeMap<String, String> = row
        .preferred_config_values
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default();
    values.insert(config_id.to_string(), value_id.to_string());
    let serialized = serde_json::to_string(&values)
        .map_err(|err| DbError::Validation(format!("selector prefs serialize: {err}")))?;
    let mut active: conversation::ActiveModel = row.into();
    active.preferred_config_values = Set(Some(serialized));
    active.updated_at = NotSet;
    active.update(conn).await?;
    Ok(())
}

/// Insert a historical child row for compatibility tests. Production releases
/// after delegation removal never call this path.
#[cfg(test)]
pub(crate) async fn create_historical_child_fixture(
    conn: &DatabaseConnection,
    folder_id: i32,
    agent_type: AgentType,
    title: Option<String>,
    parent_conversation_id: i32,
    parent_tool_use_id: String,
    delegation_call_id: String,
) -> Result<conversation::Model, DbError> {
    use sea_orm::IntoActiveModel;

    let row = create(conn, folder_id, agent_type, title, None).await?;
    let mut active = row.into_active_model();
    active.kind = Set(ConversationKind::Delegate);
    active.parent_id = Set(Some(parent_conversation_id));
    active.parent_tool_use_id = Set(Some(parent_tool_use_id));
    active.delegation_call_id = Set(Some(delegation_call_id));
    Ok(active.update(conn).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_folder};

    struct HistoricalChildLink {
        parent_conversation_id: i32,
        parent_tool_use_id: String,
        delegation_call_id: String,
    }

    /// Test fixture for rows written by pre-removal releases. Production no
    /// longer creates these children, but list/import compatibility remains.
    async fn create_historical_child(
        conn: &DatabaseConnection,
        folder_id: i32,
        agent_type: AgentType,
        title: Option<String>,
        git_branch: Option<String>,
        link: Option<HistoricalChildLink>,
    ) -> Result<conversation::Model, DbError> {
        let _ = git_branch;
        let link = link.expect("historical child link");
        create_historical_child_fixture(
            conn,
            folder_id,
            agent_type,
            title,
            link.parent_conversation_id,
            link.parent_tool_use_id,
            link.delegation_call_id,
        )
        .await
    }

    /// Build a parent + a delegation child for filter assertions.
    async fn seed_parent_with_child(conn: &DatabaseConnection, folder_id: i32) -> (i32, i32) {
        let parent = create(
            conn,
            folder_id,
            AgentType::ClaudeCode,
            Some("P".into()),
            None,
        )
        .await
        .expect("parent");
        let link = HistoricalChildLink {
            parent_conversation_id: parent.id,
            parent_tool_use_id: "tu-1".into(),
            delegation_call_id: "call-1".into(),
        };
        let child = create_historical_child(
            conn,
            folder_id,
            AgentType::Codex,
            Some("C".into()),
            None,
            Some(link),
        )
        .await
        .expect("child");
        (parent.id, child.id)
    }

    #[tokio::test]
    async fn created_by_defaults_to_user_and_honors_explicit_provenance() {
        use crate::db::entities::conversation::CREATED_BY_AGENT;

        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-created-by").await;

        let user_row = create(&db.conn, folder_id, AgentType::Codex, None, None)
            .await
            .expect("user-created row");
        assert_eq!(user_row.created_by, CREATED_BY_USER);

        let agent_row = create_with_source(
            &db.conn,
            folder_id,
            AgentType::Codex,
            None,
            None,
            CREATED_BY_AGENT,
        )
        .await
        .expect("agent-created row");
        assert_eq!(agent_row.created_by, CREATED_BY_AGENT);

        // The summary the list APIs ship carries the provenance through.
        let summary = get_by_id(&db.conn, agent_row.id).await.expect("summary");
        assert_eq!(summary.created_by, CREATED_BY_AGENT);
        let summary = get_by_id(&db.conn, user_row.id).await.expect("summary");
        assert_eq!(summary.created_by, CREATED_BY_USER);
    }

    #[tokio::test]
    async fn list_all_excludes_children_by_default() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-list-children-default").await;
        let (parent, _child) = seed_parent_with_child(&db.conn, folder).await;

        let rows = list_all(&db.conn, None, None, None, None, None, false, false)
            .await
            .expect("list");
        let ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
        assert!(ids.contains(&parent), "parent must remain visible: {ids:?}");
        assert_eq!(
            rows.len(),
            1,
            "expected only the parent, got {} rows: {ids:?}",
            rows.len()
        );
    }

    #[tokio::test]
    async fn list_all_includes_children_when_requested() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-list-children-on").await;
        let (parent, child) = seed_parent_with_child(&db.conn, folder).await;

        let rows = list_all(&db.conn, None, None, None, None, None, false, true)
            .await
            .expect("list");
        let ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
        assert!(
            ids.contains(&parent) && ids.contains(&child),
            "both parent + child must appear when include_children=true, got: {ids:?}",
        );
    }

    #[tokio::test]
    async fn archive_is_independent_and_listable_for_restore() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-archive-session").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("archive me".into()),
            None,
        )
        .await
        .expect("create");

        update_archive(&db.conn, row.id, true)
            .await
            .expect("archive");
        let active = list_all(&db.conn, None, None, None, None, None, false, false)
            .await
            .expect("active");
        assert!(active.iter().all(|item| item.id != row.id));

        let archived = list_all(&db.conn, None, None, None, None, None, true, false)
            .await
            .expect("archived");
        let archived_row = archived.iter().find(|item| item.id == row.id).expect("row");
        assert!(archived_row.archived_at.is_some());
        assert_eq!(archived_row.status, "in_progress");

        update_archive(&db.conn, row.id, false)
            .await
            .expect("restore");
        let restored = list_all(&db.conn, None, None, None, None, None, false, false)
            .await
            .expect("restored");
        assert!(restored.iter().any(|item| item.id == row.id));
    }

    #[tokio::test]
    async fn list_children_returns_only_matching_parent() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-list-children-only").await;
        let (parent_a, child_a) = seed_parent_with_child(&db.conn, folder).await;
        let (_parent_b, _child_b) = seed_parent_with_child(&db.conn, folder).await;

        let rows = list_children(&db.conn, parent_a).await.expect("list");
        assert_eq!(
            rows.len(),
            1,
            "expected 1 child of parent_a, got {}",
            rows.len()
        );
        assert_eq!(rows[0].id, child_a);
        assert_eq!(rows[0].parent_id, Some(parent_a));
    }

    #[tokio::test]
    async fn list_children_orders_newest_first() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-list-children-order").await;
        let parent = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("P".into()),
            None,
        )
        .await
        .expect("parent");
        // Two children created oldest → newest under the same parent.
        let first = create_historical_child(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("first".into()),
            None,
            Some(HistoricalChildLink {
                parent_conversation_id: parent.id,
                parent_tool_use_id: "tu-1".into(),
                delegation_call_id: "call-1".into(),
            }),
        )
        .await
        .expect("first child");
        let second = create_historical_child(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("second".into()),
            None,
            Some(HistoricalChildLink {
                parent_conversation_id: parent.id,
                parent_tool_use_id: "tu-2".into(),
                delegation_call_id: "call-2".into(),
            }),
        )
        .await
        .expect("second child");

        // The sidebar shows sub-sessions newest-first so a freshly-spawned
        // sub-agent surfaces right under its parent.
        let rows = list_children(&db.conn, parent.id).await.expect("list");
        let ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
        assert_eq!(
            ids,
            vec![second.id, first.id],
            "newest child must come first"
        );
    }

    #[tokio::test]
    async fn child_count_reflects_direct_children() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-child-count-direct").await;
        let (parent, child) = seed_parent_with_child(&db.conn, folder).await;

        // The root listing carries the parent's direct-child count so the
        // sidebar knows to show a chevron; the leaf child carries 0.
        let roots = list_all(&db.conn, None, None, None, None, None, false, false)
            .await
            .expect("list");
        let parent_row = roots.iter().find(|r| r.id == parent).expect("parent row");
        assert_eq!(parent_row.child_count, 1, "parent has one delegation child");

        let children = list_children(&db.conn, parent).await.expect("children");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child);
        assert_eq!(children[0].child_count, 0, "leaf child has no children");
    }

    #[tokio::test]
    async fn child_count_counts_grandchildren_for_nested_chevron() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-child-count-nested").await;
        let (parent, child) = seed_parent_with_child(&db.conn, folder).await;

        // Delegate a grandchild from the child so the child itself becomes
        // expandable one level down.
        let link = HistoricalChildLink {
            parent_conversation_id: child,
            parent_tool_use_id: "tu-2".into(),
            delegation_call_id: "call-2".into(),
        };
        create_historical_child(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("G".into()),
            None,
            Some(link),
        )
        .await
        .expect("grandchild");

        // list_children(parent) must report the child's OWN child_count (1) so
        // the recursive chevron appears on the nested row.
        let children = list_children(&db.conn, parent).await.expect("children");
        let child_row = children.iter().find(|r| r.id == child).expect("child row");
        assert_eq!(child_row.child_count, 1, "child has one grandchild");
    }

    #[tokio::test]
    async fn child_count_excludes_soft_deleted_children() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-child-count-deleted").await;
        let (parent, child) = seed_parent_with_child(&db.conn, folder).await;

        soft_delete(&db.conn, child)
            .await
            .expect("soft delete child");

        // A removed sub-session must not keep the parent's chevron alive: the
        // aggregate filters deleted_at IS NULL, matching list_children.
        let roots = list_all(&db.conn, None, None, None, None, None, false, false)
            .await
            .expect("list");
        let parent_row = roots.iter().find(|r| r.id == parent).expect("parent row");
        assert_eq!(
            parent_row.child_count, 0,
            "soft-deleted child must not be counted"
        );
    }

    #[tokio::test]
    async fn update_pin_sets_and_clears_without_bumping_updated_at() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-update-pin").await;
        let conv = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("c".into()),
            None,
        )
        .await
        .expect("create");

        // Freshly created rows are unpinned, and the summary projection carries
        // the field through (conv_to_summary mapping).
        let before = get_by_id(&db.conn, conv.id).await.expect("get before");
        assert!(
            before.pinned_at.is_none(),
            "new conversation must be unpinned"
        );
        let updated_at_before = before.updated_at;

        // Pin → pinned_at populated; updated_at must NOT move (pin is a view
        // preference, not activity).
        update_pin(&db.conn, conv.id, true).await.expect("pin");
        let pinned = get_by_id(&db.conn, conv.id).await.expect("get pinned");
        assert!(
            pinned.pinned_at.is_some(),
            "pinned_at must be set after pin"
        );
        assert_eq!(
            pinned.updated_at, updated_at_before,
            "pinning must not bump updated_at"
        );

        // Unpin → pinned_at cleared back to NULL; updated_at still unchanged.
        update_pin(&db.conn, conv.id, false).await.expect("unpin");
        let unpinned = get_by_id(&db.conn, conv.id).await.expect("get unpinned");
        assert!(
            unpinned.pinned_at.is_none(),
            "pinned_at must clear after unpin"
        );
        assert_eq!(
            unpinned.updated_at, updated_at_before,
            "unpinning must not bump updated_at"
        );
    }

    #[tokio::test]
    async fn list_children_excludes_soft_deleted() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-list-children-soft-del").await;
        let (parent, child) = seed_parent_with_child(&db.conn, folder).await;

        soft_delete(&db.conn, child).await.expect("soft delete");

        let rows = list_children(&db.conn, parent).await.expect("list");
        assert!(
            rows.is_empty(),
            "soft-deleted child must not appear: {rows:?}"
        );
    }

    #[tokio::test]
    async fn create_leaves_title_unlocked() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-title-unlocked").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("hi".into()),
            None,
        )
        .await
        .expect("create");
        let summary = get_by_id(&db.conn, row.id).await.expect("get");
        assert!(
            !summary.title_locked,
            "new conversation must start unlocked"
        );
    }

    #[tokio::test]
    async fn update_title_locks_the_title() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-title-lock").await;
        let row = create(&db.conn, folder, AgentType::ClaudeCode, None, None)
            .await
            .expect("create");
        update_title(&db.conn, row.id, "My name".into())
            .await
            .expect("rename");
        let summary = get_by_id(&db.conn, row.id).await.expect("get");
        assert_eq!(summary.title.as_deref(), Some("My name"));
        assert!(summary.title_locked, "manual rename must lock the title");
    }

    #[tokio::test]
    async fn bind_external_id_skips_soft_deleted_row() {
        // A late/stale `SessionStarted` write — e.g. a fork's SessionStarted{S2}
        // landing after the user deleted the conversation — must NOT mutate a
        // soft-deleted row. The initial claim is guarded on `deleted_at IS
        // NULL`, so it is a silent no-op: the deleted row keeps its old
        // external_id and is never half-resurrected.
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-extid-deleted").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("Doomed".into()),
            None,
        )
        .await
        .expect("create");
        bind_external_id(&db.conn, row.id, "session-S1".into())
            .await
            .expect("seed external_id");
        soft_delete(&db.conn, row.id).await.expect("soft delete");

        // The guarded write must no-op (Ok, no split) without touching the
        // deleted row.
        let outcome = bind_external_id(&db.conn, row.id, "session-S2".into())
            .await
            .expect("a stale SessionStarted write must be a no-op, not an error");
        assert!(outcome.preserved_conversation_id.is_none());

        // Inspect the raw row directly — `get_by_id` filters deleted rows out.
        let raw = conversation::Entity::find_by_id(row.id)
            .one(&db.conn)
            .await
            .expect("query")
            .expect("row still exists (soft-deleted)");
        assert!(raw.deleted_at.is_some(), "row must remain soft-deleted");
        assert_eq!(
            raw.external_id.as_deref(),
            Some("session-S1"),
            "a stale external_id write must not re-point a soft-deleted row"
        );

        // Sanity: the guard is not over-broad — a LIVE row still updates.
        let live = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("Live".into()),
            None,
        )
        .await
        .expect("create live");
        bind_external_id(&db.conn, live.id, "session-S9".into())
            .await
            .expect("live update");
        let live_raw = conversation::Entity::find_by_id(live.id)
            .one(&db.conn)
            .await
            .expect("query live")
            .expect("live row");
        assert_eq!(
            live_raw.external_id.as_deref(),
            Some("session-S9"),
            "a live row must still receive its external_id"
        );
    }

    #[tokio::test]
    async fn bind_external_id_is_idempotent_for_the_same_value() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-bind-idempotent").await;
        let row = create(&db.conn, folder, AgentType::ClaudeCode, None, None)
            .await
            .expect("create");
        bind_external_id(&db.conn, row.id, "session-S1".into())
            .await
            .expect("first bind");
        let before = get_by_id(&db.conn, row.id).await.expect("get");

        let outcome = bind_external_id(&db.conn, row.id, "session-S1".into())
            .await
            .expect("rebinding the same value must not error");
        assert!(
            outcome.preserved_conversation_id.is_none(),
            "no split needed when the value is unchanged"
        );
        let after = get_by_id(&db.conn, row.id).await.expect("get");
        assert_eq!(after.external_id.as_deref(), Some("session-S1"));
        assert_eq!(
            after.updated_at, before.updated_at,
            "a true no-op must not even bump updated_at a second time"
        );
    }

    #[tokio::test]
    async fn bind_external_id_refuses_a_value_already_held_by_another_row() {
        // B1: the target external_id already belongs to a DIFFERENT live row —
        // this must be a permanent, distinguishable refusal, never a silent
        // "success" and never the retryable `DbError::Conflict` CAS variant.
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-bind-refuse").await;
        let holder = create(&db.conn, folder, AgentType::ClaudeCode, None, None)
            .await
            .expect("create holder");
        bind_external_id(&db.conn, holder.id, "session-taken".into())
            .await
            .expect("seed holder");
        let challenger = create(&db.conn, folder, AgentType::ClaudeCode, None, None)
            .await
            .expect("create challenger");

        let err = bind_external_id(&db.conn, challenger.id, "session-taken".into())
            .await
            .expect_err("a value already held by another row must be refused");
        assert!(
            matches!(err, DbError::ExternalIdTaken(_)),
            "must be the permanent, non-retryable variant, got: {err:?}"
        );

        // The challenger's row must be untouched — a refused bind writes
        // nothing.
        let challenger_row = get_by_id(&db.conn, challenger.id).await.expect("get");
        assert!(challenger_row.external_id.is_none());
    }

    #[tokio::test]
    async fn bind_external_id_refuses_against_a_soft_deleted_holder() {
        // The unique (external_id, agent_type) index has no `deleted_at`
        // predicate, so a soft-deleted row still occupies the slot — the
        // conflict check must see it too, not just live rows.
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-bind-refuse-deleted").await;
        let holder = create(&db.conn, folder, AgentType::ClaudeCode, None, None)
            .await
            .expect("create holder");
        bind_external_id(&db.conn, holder.id, "session-ghost".into())
            .await
            .expect("seed holder");
        soft_delete(&db.conn, holder.id).await.expect("soft delete holder");

        let challenger = create(&db.conn, folder, AgentType::ClaudeCode, None, None)
            .await
            .expect("create challenger");
        let err = bind_external_id(&db.conn, challenger.id, "session-ghost".into())
            .await
            .expect_err("a soft-deleted holder must still refuse the bind");
        assert!(matches!(err, DbError::ExternalIdTaken(_)));
    }

    #[tokio::test]
    async fn bind_external_id_preserves_an_unrelated_previous_session() {
        // A1 for a built-in (no transcript, no continuation chain possible):
        // a row bound to S1 is asked to adopt an unrelated S2 — S1 must be
        // split off onto its own row rather than silently overwritten.
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-bind-split").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("Original".into()),
            None,
        )
        .await
        .expect("create");
        bind_external_id(&db.conn, row.id, "session-S1".into())
            .await
            .expect("initial bind");
        update_status(&db.conn, row.id, ConversationStatus::InProgress)
            .await
            .expect("mark in progress");

        let outcome = bind_external_id(&db.conn, row.id, "session-S2".into())
            .await
            .expect("takeover bind");
        let preserved_id = outcome
            .preserved_conversation_id
            .expect("an unrelated previous session must be preserved");

        let live = get_by_id(&db.conn, row.id).await.expect("get live");
        assert_eq!(live.external_id.as_deref(), Some("session-S2"));

        let preserved = get_by_id(&db.conn, preserved_id).await.expect("get preserved");
        assert_eq!(preserved.external_id.as_deref(), Some("session-S1"));
        assert_eq!(
            preserved.title.as_deref(),
            Some("Original"),
            "the preserved row keeps the original display title verbatim"
        );
        assert_eq!(
            preserved.status, "cancelled",
            "an in-flight turn under the abandoned session can never complete now"
        );
    }

    #[tokio::test]
    async fn bind_external_id_preserves_settled_status_verbatim() {
        // A row whose previous session had already reached a terminal state
        // on its own (not InProgress) must keep that status on the preserved
        // row, not be relabeled Cancelled.
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-bind-split-settled").await;
        let row = create(&db.conn, folder, AgentType::ClaudeCode, None, None)
            .await
            .expect("create");
        bind_external_id(&db.conn, row.id, "session-S1".into())
            .await
            .expect("initial bind");
        update_status(&db.conn, row.id, ConversationStatus::Completed)
            .await
            .expect("mark completed");

        let outcome = bind_external_id(&db.conn, row.id, "session-S2".into())
            .await
            .expect("takeover bind");
        let preserved_id = outcome.preserved_conversation_id.expect("must split");
        let preserved = get_by_id(&db.conn, preserved_id).await.expect("get preserved");
        assert_eq!(
            preserved.status, "completed",
            "a settled status is carried over, not forced to cancelled"
        );
    }

    #[tokio::test]
    async fn bind_external_id_sequential_takeovers_each_get_their_own_preserved_row() {
        // Two takeovers in a row on the same connection (a builtin agent
        // whose session id keeps changing) must each split off their OWN
        // abandoned session onto its OWN row, with no cross-contamination —
        // the second split must not disturb or merge with the first.
        //
        // NOTE: this does not exercise `already_preserved`'s REUSE branch
        // (finding an existing row already carrying the value about to be
        // preserved). That branch is defense-in-depth: reaching it would
        // require two rows to simultaneously hold the same (external_id,
        // agent_type), which the unique index this function relies on
        // already forbids. No sequence reachable through this function's own
        // API can construct that precondition without first violating the
        // index, so it is intentionally left unverified by a test.
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-bind-already-preserved").await;
        let row = create(&db.conn, folder, AgentType::ClaudeCode, None, None)
            .await
            .expect("create");
        bind_external_id(&db.conn, row.id, "session-S1".into())
            .await
            .expect("initial bind");
        let first = bind_external_id(&db.conn, row.id, "session-S2".into())
            .await
            .expect("first takeover");
        let preserved_id = first.preserved_conversation_id.expect("must split");

        // Now row.id (bound to S2) is asked to take over S3, while S1 already
        // lives on `preserved_id`. Nothing here should touch S1's row again.
        let second = bind_external_id(&db.conn, row.id, "session-S3".into())
            .await
            .expect("second takeover");
        let preserved_id_2 = second
            .preserved_conversation_id
            .expect("S2 is also unrelated to S3 and must itself be preserved");
        assert_ne!(
            preserved_id_2, preserved_id,
            "S2's preserved row must be a NEW row, distinct from S1's"
        );

        // S1's row is untouched by the second call.
        let s1_row = get_by_id(&db.conn, preserved_id).await.expect("get s1");
        assert_eq!(s1_row.external_id.as_deref(), Some("session-S1"));
    }

    #[tokio::test]
    async fn bind_external_id_exempts_a_custom_agent_continuation_chain() {
        // A2: a memory-less custom agent restarts, codeg opens a fresh
        // session (S2) continuing S1 via the ACP transcript's
        // `continues_from` — this must NOT be treated as a takeover, or every
        // restart would split the sidebar entry in two.
        let root = std::env::temp_dir().join(format!(
            "codeg-bind-continuation-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let custom = AgentType::custom("goosetest").expect("valid slug");
        let dir = crate::acp::registry::registry_id_for(custom);
        let header_s1 = crate::acp_transcript::TranscriptHeader::new(
            &custom.as_wire(),
            "session-S1",
            "/repo",
            1,
        );
        crate::acp_transcript::append_line_in(
            &root,
            dir,
            "session-S1",
            &serde_json::to_string(&header_s1).unwrap(),
        );
        let header_s2 = crate::acp_transcript::TranscriptHeader::new(
            &custom.as_wire(),
            "session-S2",
            "/repo",
            2,
        )
        .continuing("session-S1");
        crate::acp_transcript::append_line_in(
            &root,
            dir,
            "session-S2",
            &serde_json::to_string(&header_s2).unwrap(),
        );

        // This test exercises `continued_session_ids` directly against the
        // fixture root — `bind_external_id` itself always reads the process
        // default root (`paths::codeg_acp_transcripts_root`), which a unit
        // test cannot redirect. The chain-membership check this proves is
        // the exact predicate `bind_external_id` evaluates inline; see its
        // `is_continuation` computation.
        let ancestors = crate::acp_transcript::continued_session_ids_in(&root, custom, "session-S2");
        assert!(
            ancestors.contains("session-S1"),
            "S1 must be recognized as S2's continuation ancestor"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn refresh_auto_title_writes_when_unlocked_and_changed() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-title-auto").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("old".into()),
            None,
        )
        .await
        .expect("create");

        let wrote = refresh_auto_title(&db.conn, row.id, "fresh".into())
            .await
            .expect("auto");
        assert!(wrote, "an unlocked, changed title must be written");

        let summary = get_by_id(&db.conn, row.id).await.expect("get");
        assert_eq!(summary.title.as_deref(), Some("fresh"));
        assert!(
            !summary.title_locked,
            "auto refresh must NOT lock — the title stays eligible for future refreshes"
        );
    }

    #[tokio::test]
    async fn refresh_auto_title_skips_when_unchanged_or_empty() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-title-auto-skip").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("same".into()),
            None,
        )
        .await
        .expect("create");

        assert!(
            !refresh_auto_title(&db.conn, row.id, "same".into())
                .await
                .expect("auto-same"),
            "identical title must be a no-op"
        );
        assert!(
            !refresh_auto_title(&db.conn, row.id, String::new())
                .await
                .expect("auto-empty"),
            "empty title must be a no-op"
        );
    }

    #[tokio::test]
    async fn refresh_auto_title_never_clobbers_a_locked_title() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-title-auto-locked").await;
        let row = create(&db.conn, folder, AgentType::ClaudeCode, None, None)
            .await
            .expect("create");
        update_title(&db.conn, row.id, "User pick".into())
            .await
            .expect("rename");

        let wrote = refresh_auto_title(&db.conn, row.id, "parser title".into())
            .await
            .expect("auto");
        assert!(!wrote, "a locked title must never be auto-overwritten");
        let summary = get_by_id(&db.conn, row.id).await.expect("get");
        assert_eq!(summary.title.as_deref(), Some("User pick"));
        assert!(summary.title_locked);
    }

    #[tokio::test]
    async fn refresh_auto_title_does_not_bump_updated_at() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-title-no-bump").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("old".into()),
            None,
        )
        .await
        .expect("create");
        let before = row.updated_at;

        let wrote = refresh_auto_title(&db.conn, row.id, "fresh".into())
            .await
            .expect("auto");
        assert!(wrote);

        let summary = get_by_id(&db.conn, row.id).await.expect("get");
        assert_eq!(summary.title.as_deref(), Some("fresh"));
        assert_eq!(
            summary.updated_at, before,
            "auto-title backfill is metadata, not activity — it must not bump updated_at"
        );
    }

    #[tokio::test]
    async fn refresh_codex_auto_titles_converges_without_bumping_updated_at() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-codex-index-title").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("old title".into()),
            None,
        )
        .await
        .expect("create");
        update_external_id(&db.conn, row.id, "codex-session-1".into())
            .await
            .expect("set external id");
        let before = get_by_id(&db.conn, row.id).await.expect("get before");
        let titles = HashMap::from([("codex-session-1".to_string(), "新标题".to_string())]);

        assert_eq!(
            refresh_codex_auto_titles(&db.conn, &titles).await,
            vec![row.id]
        );
        let refreshed = get_by_id(&db.conn, row.id).await.expect("get refreshed");
        assert_eq!(refreshed.title.as_deref(), Some("新标题"));
        assert_eq!(
            refreshed.updated_at, before.updated_at,
            "an index title refresh must not count as conversation activity"
        );

        assert_eq!(
            refresh_codex_auto_titles(&db.conn, &titles).await,
            Vec::<i32>::new(),
            "a converged title map must issue no UPDATE"
        );
    }

    #[tokio::test]
    async fn refresh_codex_auto_titles_keeps_partial_successes_after_row_failure() {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};

        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-codex-index-partial-failure").await;
        let successful = create(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("old success".into()),
            None,
        )
        .await
        .expect("create successful candidate");
        update_external_id(&db.conn, successful.id, "session-success".into())
            .await
            .expect("set successful external id");
        let failing = create(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("old failure".into()),
            None,
        )
        .await
        .expect("create failing candidate");
        update_external_id(&db.conn, failing.id, "session-failure".into())
            .await
            .expect("set failing external id");
        db.conn
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    r#"CREATE TRIGGER fail_codex_title_sync
                       BEFORE UPDATE OF title ON conversation
                       WHEN OLD.id = {}
                       BEGIN
                         SELECT RAISE(FAIL, 'injected title sync failure');
                       END"#,
                    failing.id
                ),
            ))
            .await
            .expect("install title failure trigger");
        let titles = HashMap::from([
            ("session-success".to_string(), "new success".to_string()),
            ("session-failure".to_string(), "new failure".to_string()),
        ]);

        let refreshed = refresh_codex_auto_titles(&db.conn, &titles).await;

        assert_eq!(refreshed, vec![successful.id]);
        assert_eq!(
            get_by_id(&db.conn, successful.id)
                .await
                .expect("get successful row")
                .title
                .as_deref(),
            Some("new success")
        );
        assert_eq!(
            get_by_id(&db.conn, failing.id)
                .await
                .expect("get failing row")
                .title
                .as_deref(),
            Some("old failure"),
            "a row whose UPDATE failed must keep its prior title, not block other rows in the chunk"
        );
    }

    #[tokio::test]
    async fn refresh_codex_auto_title_candidate_rechecks_external_id_at_write_time() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-codex-index-race").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("old title".into()),
            None,
        )
        .await
        .expect("create");
        update_external_id(&db.conn, row.id, "session-before".into())
            .await
            .expect("set initial external id");
        let stale_candidate = conversation::Entity::find_by_id(row.id)
            .one(&db.conn)
            .await
            .expect("query candidate")
            .expect("candidate exists");

        update_external_id(&db.conn, row.id, "session-after".into())
            .await
            .expect("re-point conversation to a different Codex session");
        let wrote = refresh_codex_auto_title_candidate(
            &db.conn,
            &stale_candidate,
            "title for session-before",
        )
        .await
        .expect("conditional refresh");

        assert!(!wrote, "a stale candidate must not update a re-pointed row");
        let current = get_by_id(&db.conn, row.id).await.expect("read current row");
        assert_eq!(current.external_id.as_deref(), Some("session-after"));
        assert_eq!(current.title.as_deref(), Some("old title"));
    }

    #[tokio::test]
    async fn refresh_codex_auto_title_candidate_rechecks_original_title_at_write_time() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-codex-index-title-race").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("candidate title".into()),
            None,
        )
        .await
        .expect("create");
        update_external_id(&db.conn, row.id, "session-title-race".into())
            .await
            .expect("set external id");
        let stale_candidate = conversation::Entity::find_by_id(row.id)
            .one(&db.conn)
            .await
            .expect("query candidate")
            .expect("candidate exists");

        refresh_auto_title(&db.conn, row.id, "newer automatic title".into())
            .await
            .expect("apply concurrent automatic title");
        let wrote = refresh_codex_auto_title_candidate(
            &db.conn,
            &stale_candidate,
            "stale session index title",
        )
        .await
        .expect("conditional refresh");

        assert!(!wrote, "a stale candidate must not overwrite a newer title");
        let current = get_by_id(&db.conn, row.id).await.expect("read current row");
        assert_eq!(current.title.as_deref(), Some("newer automatic title"));
    }

    #[tokio::test]
    async fn refresh_codex_auto_title_candidate_rechecks_deleted_at_at_write_time() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-codex-index-delete-race").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("candidate title".into()),
            None,
        )
        .await
        .expect("create");
        update_external_id(&db.conn, row.id, "session-delete-race".into())
            .await
            .expect("set external id");
        let stale_candidate = conversation::Entity::find_by_id(row.id)
            .one(&db.conn)
            .await
            .expect("query candidate")
            .expect("candidate exists");

        soft_delete(&db.conn, row.id)
            .await
            .expect("concurrently delete candidate");
        let wrote = refresh_codex_auto_title_candidate(
            &db.conn,
            &stale_candidate,
            "stale session index title",
        )
        .await
        .expect("conditional refresh");

        assert!(!wrote, "a stale candidate must not update a deleted row");
        let current = conversation::Entity::find_by_id(row.id)
            .one(&db.conn)
            .await
            .expect("read current row")
            .expect("soft-deleted row remains persisted");
        assert!(current.deleted_at.is_some());
        assert_eq!(current.title.as_deref(), Some("candidate title"));
    }

    #[tokio::test]
    async fn refresh_codex_auto_title_candidate_rechecks_title_lock_at_write_time() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-codex-index-lock-race").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("same manual title".into()),
            None,
        )
        .await
        .expect("create");
        update_external_id(&db.conn, row.id, "session-lock-race".into())
            .await
            .expect("set external id");
        let stale_candidate = conversation::Entity::find_by_id(row.id)
            .one(&db.conn)
            .await
            .expect("query candidate")
            .expect("candidate exists");

        // Same text, but going through `update_title` locks it.
        update_title(&db.conn, row.id, "same manual title".into())
            .await
            .expect("lock title without changing its value");
        let wrote = refresh_codex_auto_title_candidate(
            &db.conn,
            &stale_candidate,
            "stale session index title",
        )
        .await
        .expect("conditional refresh");

        assert!(
            !wrote,
            "a stale candidate must not overwrite a locked title"
        );
        let current = get_by_id(&db.conn, row.id).await.expect("read current row");
        assert!(current.title_locked);
        assert_eq!(current.title.as_deref(), Some("same manual title"));
    }

    #[tokio::test]
    async fn refresh_codex_auto_title_candidate_rechecks_folder_deletion_at_write_time() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-codex-index-folder-race").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::Codex,
            Some("old title".into()),
            None,
        )
        .await
        .expect("create");
        update_external_id(&db.conn, row.id, "session-folder-race".into())
            .await
            .expect("set external id");
        let stale_candidate = conversation::Entity::find_by_id(row.id)
            .one(&db.conn)
            .await
            .expect("query candidate")
            .expect("candidate exists");

        crate::db::service::folder_service::soft_delete_folder(&db.conn, folder)
            .await
            .expect("soft delete folder");
        let wrote =
            refresh_codex_auto_title_candidate(&db.conn, &stale_candidate, "index title").await;

        assert!(
            !wrote.expect("conditional refresh"),
            "a row whose folder was deleted mid-refresh must not be rewritten (and so must not be broadcast)"
        );
        let current = get_by_id(&db.conn, row.id).await.expect("read current row");
        assert_eq!(current.title.as_deref(), Some("old title"));
    }

    #[tokio::test]
    async fn refresh_codex_auto_title_candidate_adopts_a_title_over_null() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-codex-index-null-title").await;
        let row = create(&db.conn, folder, AgentType::Codex, None, None)
            .await
            .expect("create");
        update_external_id(&db.conn, row.id, "session-null-title".into())
            .await
            .expect("set external id");
        let candidate = conversation::Entity::find_by_id(row.id)
            .one(&db.conn)
            .await
            .expect("query candidate")
            .expect("candidate exists");
        assert!(candidate.title.is_none(), "fixture must start titleless");

        assert!(
            refresh_codex_auto_title_candidate(&db.conn, &candidate, "first Codex title")
                .await
                .expect("conditional refresh"),
            "the IS NULL branch of the observed-title CAS must still write"
        );
        assert_eq!(
            get_by_id(&db.conn, row.id)
                .await
                .expect("read current row")
                .title
                .as_deref(),
            Some("first Codex title")
        );

        // ...and once a title exists, the same stale (title = NULL) candidate
        // must no longer match.
        assert!(
            !refresh_codex_auto_title_candidate(&db.conn, &candidate, "second Codex title")
                .await
                .expect("conditional refresh"),
            "a stale titleless candidate must not clobber the title it just wrote"
        );
    }

    #[tokio::test]
    async fn refresh_codex_auto_titles_skips_rows_the_sidebar_list_hides() {
        let db = fresh_in_memory_db().await;
        let live_folder = seed_folder(&db, "/tmp/codeg-codex-index-visible").await;
        let dead_folder = seed_folder(&db, "/tmp/codeg-codex-index-hidden").await;

        let visible = create(
            &db.conn,
            live_folder,
            AgentType::Codex,
            Some("old visible".into()),
            None,
        )
        .await
        .expect("create visible");
        update_external_id(&db.conn, visible.id, "session-visible".into())
            .await
            .expect("set visible external id");

        let in_dead_folder = create(
            &db.conn,
            dead_folder,
            AgentType::Codex,
            Some("old hidden".into()),
            None,
        )
        .await
        .expect("create hidden");
        update_external_id(&db.conn, in_dead_folder.id, "session-hidden".into())
            .await
            .expect("set hidden external id");
        crate::db::service::folder_service::soft_delete_folder(&db.conn, dead_folder)
            .await
            .expect("soft delete folder");

        let loop_row = create(
            &db.conn,
            live_folder,
            AgentType::Codex,
            Some("old loop".into()),
            None,
        )
        .await
        .expect("create loop row");
        update_external_id(&db.conn, loop_row.id, "session-loop".into())
            .await
            .expect("set loop external id");
        // No public write path mints kind='loop' for a codex row, so flip it
        // directly — mirrors `create_historical_child_fixture` above.
        let mut active: conversation::ActiveModel = conversation::Entity::find_by_id(loop_row.id)
            .one(&db.conn)
            .await
            .expect("query loop row")
            .expect("loop row exists")
            .into();
        active.kind = Set(ConversationKind::Loop);
        active.update(&db.conn).await.expect("flip kind");

        let internal_row = create(
            &db.conn,
            live_folder,
            AgentType::Codex,
            Some("old internal".into()),
            None,
        )
        .await
        .expect("create harness-internal row");
        update_external_id(&db.conn, internal_row.id, "session-internal".into())
            .await
            .expect("set internal external id");
        set_harness_internal(&db.conn, internal_row.id, true)
            .await
            .expect("flip harness_internal");

        let titles = HashMap::from([
            ("session-visible".to_string(), "new visible".to_string()),
            ("session-hidden".to_string(), "new hidden".to_string()),
            ("session-loop".to_string(), "new loop".to_string()),
            ("session-internal".to_string(), "new internal".to_string()),
        ]);

        let refreshed = refresh_codex_auto_titles(&db.conn, &titles).await;

        assert_eq!(
            refreshed,
            vec![visible.id],
            "only rows `list_all` would return may be refreshed — every refreshed id is broadcast as a sidebar upsert"
        );
        assert_eq!(
            get_by_id(&db.conn, in_dead_folder.id)
                .await
                .expect("read hidden row")
                .title
                .as_deref(),
            Some("old hidden")
        );
        assert_eq!(
            get_by_id(&db.conn, loop_row.id)
                .await
                .expect("read loop row")
                .title
                .as_deref(),
            Some("old loop")
        );
        assert_eq!(
            get_by_id(&db.conn, internal_row.id)
                .await
                .expect("read internal row")
                .title
                .as_deref(),
            Some("old internal")
        );
    }

    /// The work-task / automation launch path: the seed IS the name, so locking
    /// must keep the title byte-identical, keep the row where it is in a
    /// recency-sorted sidebar, and make the next auto-title a no-op.
    #[tokio::test]
    async fn lock_title_freezes_the_seed_without_rewriting_or_bumping() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-title-lock-seed").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("Fix the login flow".into()),
            None,
        )
        .await
        .expect("create");
        let before = row.updated_at;
        assert!(!row.title_locked, "a fresh row starts unlocked");

        lock_title(&db.conn, row.id).await.expect("lock");

        let locked = get_by_id(&db.conn, row.id).await.expect("get");
        assert!(locked.title_locked, "lock_title must set the flag");
        assert_eq!(
            locked.title.as_deref(),
            Some("Fix the login flow"),
            "lock_title must not rewrite the title"
        );
        assert_eq!(
            locked.updated_at, before,
            "locking is metadata, not activity — it must not bump updated_at"
        );

        // The whole point: the parsed session title can no longer take over.
        assert!(
            !refresh_auto_title(&db.conn, row.id, "项目：/Users/me/app".into())
                .await
                .expect("auto"),
            "a seeded-and-locked title must survive the per-turn backfill"
        );
        let after = get_by_id(&db.conn, row.id).await.expect("get again");
        assert_eq!(after.title.as_deref(), Some("Fix the login flow"));
    }

    /// A missing row must not be an error — the caller locks on a best-effort
    /// path where a failure would be worse than the drift it prevents.
    #[tokio::test]
    async fn lock_title_on_a_missing_row_is_a_no_op() {
        let db = fresh_in_memory_db().await;
        lock_title(&db.conn, 999_999).await.expect("no-op lock");
    }

    #[tokio::test]
    async fn retitle_if_unchanged_follows_the_owner_rename() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-retitle-follow").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("old".into()),
            None,
        )
        .await
        .expect("create");
        let before = row.updated_at;
        lock_title(&db.conn, row.id).await.expect("lock");

        let wrote = retitle_if_unchanged(&db.conn, row.id, "old", "new")
            .await
            .expect("retitle");
        assert!(wrote, "a title still in sync with its owner must follow it");

        let summary = get_by_id(&db.conn, row.id).await.expect("get");
        assert_eq!(summary.title.as_deref(), Some("new"));
        assert!(summary.title_locked, "the row must stay locked");
        assert_eq!(
            summary.updated_at, before,
            "following an owner rename is metadata, not activity"
        );
    }

    /// The guard that makes the sync safe: once the user names the conversation
    /// themselves, the owner's rename must not reach it. `title_locked` is true
    /// in BOTH cases, so only the expected-value comparison can tell them apart.
    #[tokio::test]
    async fn retitle_if_unchanged_refuses_after_a_manual_rename() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-retitle-refuse").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("old".into()),
            None,
        )
        .await
        .expect("create");
        update_title(&db.conn, row.id, "User pick".into())
            .await
            .expect("rename");

        let wrote = retitle_if_unchanged(&db.conn, row.id, "old", "new")
            .await
            .expect("retitle");
        assert!(!wrote, "a hand-picked title must not follow the owner");
        let summary = get_by_id(&db.conn, row.id).await.expect("get");
        assert_eq!(summary.title.as_deref(), Some("User pick"));
    }

    #[tokio::test]
    async fn retitle_if_unchanged_skips_empty_and_identical() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-retitle-skip").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("same".into()),
            None,
        )
        .await
        .expect("create");

        assert!(
            !retitle_if_unchanged(&db.conn, row.id, "same", "same")
                .await
                .expect("identical"),
            "an unchanged owner name must be a no-op"
        );
        assert!(
            !retitle_if_unchanged(&db.conn, row.id, "same", "   ")
                .await
                .expect("blank"),
            "a blank new title must never erase the name"
        );
        let summary = get_by_id(&db.conn, row.id).await.expect("get");
        assert_eq!(summary.title.as_deref(), Some("same"));
    }

    #[tokio::test]
    async fn refresh_external_activity_moves_forward_only() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-activity").await;
        let row = create(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            Some("kept".into()),
            None,
        )
        .await
        .expect("create");
        let created_at = row.created_at;
        let before = row.updated_at;

        // The session kept running in the agent's own CLI after import.
        let later = before + chrono::Duration::hours(2);
        assert!(
            refresh_external_activity(&db.conn, row.id, later, 7)
                .await
                .expect("newer"),
            "newer transcript activity must be adopted"
        );
        let summary = get_by_id(&db.conn, row.id).await.expect("get");
        assert_eq!(summary.updated_at, later);
        assert_eq!(summary.message_count, 7);
        assert_eq!(
            summary.created_at, created_at,
            "created_at is the import/creation time and must not move"
        );
        assert_eq!(summary.title.as_deref(), Some("kept"));

        // Re-scanning the same (or an older) transcript must not move the row
        // back or re-order a sidebar sorted by recency.
        for (at, label) in [(later, "identical"), (before, "older")] {
            assert!(
                !refresh_external_activity(&db.conn, row.id, at, 1)
                    .await
                    .expect("no-op"),
                "{label} activity must be a no-op"
            );
        }
        let summary = get_by_id(&db.conn, row.id).await.expect("get");
        assert_eq!(summary.updated_at, later);
        assert_eq!(summary.message_count, 7, "a no-op must not resync counts");
    }

    #[tokio::test]
    async fn refresh_external_activity_skips_deleted_and_child_rows() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-activity-guards").await;
        let parent = create(&db.conn, folder, AgentType::ClaudeCode, None, None)
            .await
            .expect("parent");
        let child = create_historical_child(
            &db.conn,
            folder,
            AgentType::ClaudeCode,
            None,
            None,
            Some(HistoricalChildLink {
                parent_conversation_id: parent.id,
                parent_tool_use_id: "tu-activity".into(),
                delegation_call_id: "call-activity".into(),
            }),
        )
        .await
        .expect("child");
        let deleted = create(&db.conn, folder, AgentType::ClaudeCode, None, None)
            .await
            .expect("deleted");
        soft_delete(&db.conn, deleted.id)
            .await
            .expect("soft delete");

        let later = Utc::now() + chrono::Duration::hours(1);
        assert!(
            !refresh_external_activity(&db.conn, child.id, later, 9)
                .await
                .expect("child"),
            "a delegation child is not a sidebar row"
        );
        assert!(
            !refresh_external_activity(&db.conn, deleted.id, later, 9)
                .await
                .expect("deleted"),
            "a soft-deleted conversation must never be half-resurrected"
        );

        for id in [child.id, deleted.id] {
            let raw = conversation::Entity::find_by_id(id)
                .one(&db.conn)
                .await
                .expect("query")
                .expect("row present");
            assert_eq!(raw.message_count, 0, "row {id} untouched");
        }
    }

    #[tokio::test]
    async fn create_paths_write_expected_kinds() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/kinds").await;

        let regular = create(&db.conn, folder_id, AgentType::ClaudeCode, None, None)
            .await
            .expect("regular");
        assert_eq!(regular.kind, ConversationKind::Regular);

        let chat = create_chat(&db.conn, folder_id, AgentType::ClaudeCode, None, None)
            .await
            .expect("chat");
        assert_eq!(chat.kind, ConversationKind::Chat);

        let child = create_historical_child(
            &db.conn,
            folder_id,
            AgentType::Codex,
            None,
            None,
            Some(HistoricalChildLink {
                parent_conversation_id: regular.id,
                parent_tool_use_id: "tu-kind".into(),
                delegation_call_id: "call-kind".into(),
            }),
        )
        .await
        .expect("delegate");
        assert_eq!(child.kind, ConversationKind::Delegate);
        assert_eq!(child.parent_id, Some(regular.id));
    }

    #[tokio::test]
    async fn list_all_excludes_loop_kind_rows() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/loop-filter").await;
        let keep = create(
            &db.conn,
            folder_id,
            AgentType::ClaudeCode,
            Some("keep".into()),
            None,
        )
        .await
        .expect("keep");
        let hide = create(
            &db.conn,
            folder_id,
            AgentType::ClaudeCode,
            Some("hide".into()),
            None,
        )
        .await
        .expect("hide");
        // No public write path mints kind='loop' yet (reserved for the loop
        // engine), so flip the row directly to exercise the filter.
        let mut active: conversation::ActiveModel = hide.into();
        active.kind = Set(ConversationKind::Loop);
        active.update(&db.conn).await.expect("flip kind");

        let rows = list_all(&db.conn, None, None, None, None, None, false, false)
            .await
            .expect("list");
        assert!(rows.iter().any(|r| r.id == keep.id), "regular row stays");
        assert!(
            !rows.iter().any(|r| r.title.as_deref() == Some("hide")),
            "loop row must be excluded"
        );
    }

    #[tokio::test]
    async fn list_all_excludes_harness_internal_rows() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/harness-internal-filter").await;
        let keep = create(
            &db.conn,
            folder_id,
            AgentType::Codex,
            Some("user session".into()),
            None,
        )
        .await
        .expect("keep");
        let hide = create(
            &db.conn,
            folder_id,
            AgentType::Codex,
            Some("codex subagent".into()),
            None,
        )
        .await
        .expect("hide");
        let hide_id = hide.id;
        let mut active: conversation::ActiveModel = hide.into();
        active.harness_internal = Set(true);
        active.codeg_owned = Set(false);
        active.update(&db.conn).await.expect("mark internal");

        let rows = list_all(&db.conn, None, None, None, None, None, false, false)
            .await
            .expect("list");
        assert!(rows.iter().any(|r| r.id == keep.id), "user session stays");
        assert!(
            !rows
                .iter()
                .any(|r| r.title.as_deref() == Some("codex subagent")),
            "harness-internal row must be excluded"
        );
        let fetched = get_by_id(&db.conn, hide_id).await.expect("direct fetch");
        assert!(fetched.harness_internal);
    }
}
