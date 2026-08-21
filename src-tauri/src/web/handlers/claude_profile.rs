use std::sync::Arc;

use axum::{extract::Extension, Json};

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::claude_profile as profile_commands;
use crate::models::claude_profile::{
    ClaudeProfileInfo, ClaudeProfileUpsert, ClaudeSettingsReadResult,
    ConversationClaudeProfileResult,
};

pub async fn claude_profile_list(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<Vec<ClaudeProfileInfo>>, AppCommandError> {
    let result = profile_commands::claude_profile_list_core(&state.data_dir)?;
    Ok(Json(result))
}

pub async fn claude_settings_read(
    Json(params): Json<profile_commands::ClaudeSettingsReadParams>,
) -> Result<Json<ClaudeSettingsReadResult>, AppCommandError> {
    Ok(Json(profile_commands::claude_settings_read_core(
        params.path,
    )?))
}

pub async fn claude_profile_upsert(
    Extension(state): Extension<Arc<AppState>>,
    Json(payload): Json<ClaudeProfileUpsert>,
) -> Result<Json<ClaudeProfileInfo>, AppCommandError> {
    let result = profile_commands::claude_profile_upsert_core(&state.data_dir, payload)?;
    Ok(Json(result))
}

pub async fn claude_profile_delete(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<profile_commands::ClaudeProfileDeleteParams>,
) -> Result<Json<()>, AppCommandError> {
    profile_commands::claude_profile_delete_core(&state.data_dir, &params.id)?;
    Ok(Json(()))
}

pub async fn conversation_get_claude_profile(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<profile_commands::ConversationGetClaudeProfileParams>,
) -> Result<Json<ConversationClaudeProfileResult>, AppCommandError> {
    let result = profile_commands::conversation_get_claude_profile_core(
        &state.db,
        &state.data_dir,
        params.conversation_id,
    )
    .await?;
    Ok(Json(result))
}

pub async fn conversation_set_claude_profile(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<profile_commands::ConversationSetClaudeProfileParams>,
) -> Result<Json<ConversationClaudeProfileResult>, AppCommandError> {
    let result = profile_commands::conversation_set_claude_profile_core(
        &state.db,
        &state.connection_manager,
        &state.data_dir,
        params.conversation_id,
        params.profile_id,
    )
    .await?;
    Ok(Json(result))
}
