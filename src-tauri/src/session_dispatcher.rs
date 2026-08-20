//! One Session Dispatcher for follow-ups and mailbox attention.
//!
//! Mailbox events stay in their own store. The PromptQueue remains the only
//! Turn entrance. This module decides whether a closed Session must be
//! started/resumed so a queued notice can actually reach the Agent.
//!
//! Owner window label is `session-dispatcher` so closing a Workbench window
//! does not tear down a mail-started runtime (`disconnect_by_owner_window`).

use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sea_orm::EntityTrait;

use crate::acp::manager::ConnectionManager;
use crate::acp::types::ConnectionStatus;
use crate::commands::acp::{build_session_runtime_env, verify_agent_installed};
use crate::db::entities::conversation::{self, ConversationKind};
use crate::db::entities::folder;
use crate::db::AppDatabase;
use crate::models::AgentType;
use crate::web::event_bridge::EventEmitter;

pub const SESSION_DISPATCH_OWNER: &str = "session-dispatcher";
const ENSURE_COOLDOWN: Duration = Duration::from_secs(45);

#[derive(Clone, Debug)]
pub struct SessionDispatchConfig {
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureRuntimeOutcome {
    AlreadyLive,
    Spawned,
    Reused,
    /// No native Session id to resume. `session/new` would orphan history.
    SkippedNoIdentity,
}

/// Per-Session gate so a missing connection does not spawn twice while the
/// first handshake is still in flight.
#[derive(Clone, Default)]
pub struct EnsureGate {
    inner: Arc<Mutex<std::collections::HashMap<i32, Instant>>>,
}

impl EnsureGate {
    pub fn try_begin(&self, conversation_id: i32) -> bool {
        let Ok(mut map) = self.inner.lock() else {
            return false;
        };
        if let Some(started) = map.get(&conversation_id) {
            if started.elapsed() < ENSURE_COOLDOWN {
                return false;
            }
        }
        map.insert(conversation_id, Instant::now());
        true
    }
}

pub fn connection_is_live(status: &ConnectionStatus) -> bool {
    !matches!(
        status,
        ConnectionStatus::Disconnected | ConnectionStatus::Error
    )
}

pub fn conversation_can_auto_start(row: &conversation::Model) -> bool {
    row.deleted_at.is_none()
        && !row.harness_internal
        && row.kind != ConversationKind::Delegate
        && AgentType::from_wire(&row.agent_type).is_some()
}

async fn connection_for_row(
    manager: &ConnectionManager,
    row: &conversation::Model,
) -> Option<(String, Arc<tokio::sync::RwLock<crate::acp::SessionState>>)> {
    let id = if let Some(id) = manager.find_connection_by_conversation_id(row.id).await {
        Some(id)
    } else if let (Some(external_id), Some(agent_type)) = (
        row.external_id.as_deref(),
        AgentType::from_wire(&row.agent_type),
    ) {
        manager
            .find_connection_by_external_id(external_id, agent_type)
            .await
    } else {
        None
    }?;
    let state = manager.get_state(&id).await?;
    Some((id, state))
}

async fn live_connection_for_row(
    manager: &ConnectionManager,
    row: &conversation::Model,
) -> Option<(String, Arc<tokio::sync::RwLock<crate::acp::SessionState>>)> {
    let (id, state) = connection_for_row(manager, row).await?;
    let status = state.read().await.status.clone();
    if connection_is_live(&status) {
        Some((id, state))
    } else {
        None
    }
}

async fn resolve_working_dir(
    db: &AppDatabase,
    row: &conversation::Model,
) -> Result<String, String> {
    if let Some(cwd) = row
        .origin_cwd
        .as_deref()
        .map(str::trim)
        .filter(|cwd| !cwd.is_empty())
    {
        return Ok(cwd.to_string());
    }
    let folder = folder::Entity::find_by_id(row.folder_id)
        .one(&db.conn)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Folder {} not found for Session {}", row.folder_id, row.id))?;
    let path = folder.path.trim();
    if path.is_empty() {
        return Err(format!("Session {} has no working directory", row.id));
    }
    Ok(path.to_string())
}

/// Resume the existing native Session. Never calls `session/new`: a missing
/// `external_id` would orphan history instead of delivering mail into it.
///
/// Enforced on BOTH sides of the spawn. Here: no `external_id`, no launch
/// ([`EnsureRuntimeOutcome::SkippedNoIdentity`]). Inside the connection:
/// `resume_only`, so an agent that cannot restore the id stops the connection
/// rather than falling back to `session/new` — that fallback is what turned an
/// @ mention of an idle Session into a phantom Session answering in its place.
pub async fn ensure_session_runtime(
    db: &AppDatabase,
    manager: &ConnectionManager,
    emitter: &EventEmitter,
    data_dir: &Path,
    row: &conversation::Model,
) -> Result<EnsureRuntimeOutcome, String> {
    if !conversation_can_auto_start(row) {
        return Err(format!("Session {} cannot be auto-started", row.id));
    }
    if live_connection_for_row(manager, row).await.is_some() {
        return Ok(EnsureRuntimeOutcome::AlreadyLive);
    }

    if let Some((dead_id, state)) = connection_for_row(manager, row).await {
        let status = state.read().await.status.clone();
        if !connection_is_live(&status) {
            let _ = manager.disconnect(&dead_id).await;
        }
    }

    let agent_type = AgentType::from_wire(&row.agent_type)
        .ok_or_else(|| format!("Unknown agent type {}", row.agent_type))?;
    if agent_type == AgentType::Cline {
        return Err(format!(
            "Session {} is Cline, which does not support resume",
            row.id
        ));
    }
    let Some(session_id) = row
        .external_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
    else {
        return Ok(EnsureRuntimeOutcome::SkippedNoIdentity);
    };
    verify_agent_installed(agent_type)
        .await
        .map_err(|error| error.to_string())?;
    let working_dir = resolve_working_dir(db, row).await?;
    let runtime_env =
        build_session_runtime_env(db, agent_type, Some(session_id.as_str()), data_dir)
            .await
            .map_err(|error| error.to_string())?;

    let connection_id = manager
        .resume_agent_for_conversation(
            agent_type,
            Some(working_dir),
            session_id,
            runtime_env,
            SESSION_DISPATCH_OWNER.to_string(),
            emitter.clone(),
            row.id,
            row.folder_id,
        )
        .await
        .map_err(|error| error.to_string())?;

    // `resume_agent_for_conversation` binds the row before the handshake, but a
    // deduped reuse returns someone else's connection, which may predate the
    // link. Cheap and idempotent either way.
    if let Some(state) = manager.get_state(&connection_id).await {
        let mut state = state.write().await;
        if state.conversation_id.is_none() {
            state.conversation_id = Some(row.id);
        }
        if state.folder_id.is_none() {
            state.folder_id = Some(row.folder_id);
        }
    }

    let reused = live_connection_for_row(manager, row)
        .await
        .is_some_and(|(id, _)| id == connection_id);
    Ok(if reused {
        EnsureRuntimeOutcome::Reused
    } else {
        EnsureRuntimeOutcome::Spawned
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};

    #[test]
    fn disconnected_and_error_are_not_live() {
        assert!(!connection_is_live(&ConnectionStatus::Disconnected));
        assert!(!connection_is_live(&ConnectionStatus::Error));
        assert!(connection_is_live(&ConnectionStatus::Connecting));
        assert!(connection_is_live(&ConnectionStatus::Connected));
        assert!(connection_is_live(&ConnectionStatus::Prompting));
    }

    #[test]
    fn ensure_gate_blocks_a_second_start_inside_cooldown() {
        let gate = EnsureGate::default();
        assert!(gate.try_begin(7));
        assert!(!gate.try_begin(7));
        assert!(gate.try_begin(8));
    }

    #[tokio::test]
    async fn resolve_cwd_prefers_origin_then_folder() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-dispatcher-cwd").await;
        let conversation_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let row = conversation::Entity::find_by_id(conversation_id)
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            resolve_working_dir(&db, &row).await.unwrap(),
            "/tmp/codeg-dispatcher-cwd"
        );
    }

    #[tokio::test]
    async fn missing_native_id_does_not_start_a_new_session() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-dispatcher-no-sid").await;
        let conversation_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let row = conversation::Entity::find_by_id(conversation_id)
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap();
        let outcome = ensure_session_runtime(
            &db,
            &ConnectionManager::new(),
            &EventEmitter::Noop,
            std::path::Path::new("/tmp/codeg-dispatcher-no-sid"),
            &row,
        )
        .await
        .expect("skip is not an error");
        assert_eq!(outcome, EnsureRuntimeOutcome::SkippedNoIdentity);
    }
}
