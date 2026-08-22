//! ACP connection orchestration.
//!
//! This module owns the boundary between a stable Codeg conversation and an
//! ephemeral ACP process. Transports provide a request; this coordinator
//! resolves the persisted identity and launch configuration before publishing
//! the connection through [`ConnectionManager`].

use std::collections::BTreeMap;
use std::path::Path;

use crate::acp::error::AcpError;
use crate::acp::manager::ConnectionManager;
use crate::commands::acp::{
    build_session_runtime_env, resolve_connect_selector_prefs, verify_agent_installed,
};
use crate::db::service::conversation_service;
use crate::db::AppDatabase;
use crate::models::agent::AgentType;
use crate::web::event_bridge::EventEmitter;

pub(crate) struct AcpConnectRequest {
    pub agent_type: AgentType,
    pub working_dir: Option<String>,
    pub session_id: Option<String>,
    pub preferred_mode_id: Option<String>,
    pub preferred_config_values: Option<BTreeMap<String, String>>,
    pub conversation_id: Option<i32>,
    pub wait_until_ready: bool,
}

pub(crate) async fn resolve_connect_binding(
    db: &AppDatabase,
    conversation_id: Option<i32>,
    agent_type: AgentType,
) -> Result<Option<(i32, i32)>, AcpError> {
    let Some(conversation_id) = conversation_id else {
        return Ok(None);
    };
    let conversation = conversation_service::get_by_id(&db.conn, conversation_id)
        .await
        .map_err(|error| {
            AcpError::protocol(format!(
                "cannot attach ACP runtime to conversation {conversation_id}: {error}"
            ))
        })?;
    if conversation.agent_type != agent_type {
        return Err(AcpError::protocol(format!(
            "conversation {conversation_id} belongs to agent '{:?}', not '{:?}'",
            conversation.agent_type, agent_type
        )));
    }
    Ok(Some((conversation_id, conversation.folder_id)))
}

/// The one ACP connect path shared by Tauri and Web clients. Stable Codeg
/// identity is resolved from SQLite once and installed in the runtime before
/// the connection is published; transports do not patch `SessionState` after
/// spawn or maintain their own profile/selector lifecycle.
pub(crate) async fn acp_connect_core(
    manager: &ConnectionManager,
    db: &AppDatabase,
    data_dir: &Path,
    owner_window_label: String,
    emitter: EventEmitter,
    request: AcpConnectRequest,
) -> Result<String, AcpError> {
    let AcpConnectRequest {
        agent_type,
        working_dir,
        session_id,
        preferred_mode_id,
        preferred_config_values,
        conversation_id,
        wait_until_ready,
    } = request;

    // A profile picked in the composer travels on THIS request, not through a
    // conversation row: a tab whose conversation does not exist yet still has
    // to relaunch on the profile the user just chose.
    let explicit_profile_id = preferred_config_values
        .as_ref()
        .and_then(|values| values.get(crate::acp::connection::PREFERRED_PROFILE_CONFIG_KEY))
        .cloned();
    let runtime_env = build_session_runtime_env(
        db,
        agent_type,
        session_id.as_deref(),
        data_dir,
        conversation_id,
        explicit_profile_id.as_deref(),
    )
    .await?;

    // Connecting never downloads an agent. Installation remains an explicit
    // settings action so a Session cannot trigger a surprise package change.
    verify_agent_installed(agent_type).await?;

    let (preferred_mode_id, preferred_config_values) = resolve_connect_selector_prefs(
        db,
        conversation_id,
        preferred_mode_id,
        preferred_config_values.unwrap_or_default(),
    )
    .await;

    let conversation_binding = resolve_connect_binding(db, conversation_id, agent_type).await?;
    let wait_for_fresh_session = wait_until_ready && session_id.is_none();

    if let Some((conversation_id, folder_id)) = conversation_binding {
        manager
            .spawn_agent_for_conversation(
                agent_type,
                working_dir,
                session_id,
                runtime_env,
                owner_window_label,
                emitter,
                preferred_mode_id,
                preferred_config_values,
                conversation_id,
                folder_id,
                wait_for_fresh_session,
            )
            .await
    } else if wait_for_fresh_session {
        manager
            .spawn_agent_wait_ready(
                agent_type,
                working_dir,
                runtime_env,
                owner_window_label,
                emitter,
                preferred_mode_id,
                preferred_config_values,
            )
            .await
    } else {
        manager
            .spawn_agent(
                agent_type,
                working_dir,
                session_id,
                runtime_env,
                owner_window_label,
                emitter,
                preferred_mode_id,
                preferred_config_values,
            )
            .await
    }
}
