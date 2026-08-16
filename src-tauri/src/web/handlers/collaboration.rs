use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::collaboration;
use crate::commands::collaboration::SessionCollaborationSettings;
use crate::models::{
    CollaborationFeed, CollaborationInterruptResult, CollaborationSendResult,
    CollaborationUnreadOverview, InterruptCollaborationInput,
    SendAndInterruptCollaborationInput, SendAndInterruptCollaborationResult,
    SendCollaborationMessageInput,
};

#[derive(Deserialize)]
pub struct SendParams {
    pub input: SendCollaborationMessageInput,
}

#[derive(Deserialize)]
pub struct InterruptParams {
    pub input: InterruptCollaborationInput,
}

#[derive(Deserialize)]
pub struct SendInterruptParams {
    pub input: SendAndInterruptCollaborationInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedParams {
    pub conversation_id: i32,
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkSeenParams {
    pub conversation_id: i32,
    pub delivery_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DismissParams {
    pub conversation_id: i32,
    pub delivery_id: String,
}

pub async fn get_settings(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<SessionCollaborationSettings>, AppCommandError> {
    Ok(Json(
        collaboration::load_session_collaboration_settings(&state.db.conn).await,
    ))
}

#[derive(Deserialize)]
pub struct SetSettingsParams {
    pub settings: SessionCollaborationSettings,
}

pub async fn set_settings(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<SetSettingsParams>,
) -> Result<Json<SessionCollaborationSettings>, AppCommandError> {
    Ok(Json(
        collaboration::set_session_collaboration_settings_core(
            &state.db.conn,
            &state.session_collaboration_config,
            &state.emitter,
            &state.prompt_queue,
            params.settings,
        )
        .await?,
    ))
}

pub async fn send(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<SendParams>,
) -> Result<Json<CollaborationSendResult>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_send_core(
            &state.db.conn,
            &state.emitter,
            &state.prompt_queue,
            params.input,
        )
        .await?,
    ))
}

pub async fn interrupt(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<InterruptParams>,
) -> Result<Json<CollaborationInterruptResult>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_interrupt_core(
            &state.db.conn,
            &state.connection_manager,
            &state.emitter,
            &state.prompt_queue,
            params.input,
        )
        .await?,
    ))
}

pub async fn send_interrupt(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<SendInterruptParams>,
) -> Result<Json<SendAndInterruptCollaborationResult>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_send_interrupt_core(
            &state.db.conn,
            &state.connection_manager,
            &state.emitter,
            &state.prompt_queue,
            params.input,
        )
        .await?,
    ))
}

pub async fn feed(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<FeedParams>,
) -> Result<Json<CollaborationFeed>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_feed_core(
            &state.db.conn,
            params.conversation_id,
            params.limit,
        )
        .await?,
    ))
}

pub async fn unread_overview(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<CollaborationUnreadOverview>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_unread_overview_core(&state.db.conn).await?,
    ))
}

pub async fn mark_seen(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<MarkSeenParams>,
) -> Result<Json<CollaborationFeed>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_mark_seen_core(
            &state.db.conn,
            &state.emitter,
            params.conversation_id,
            params.delivery_ids,
        )
        .await?,
    ))
}

pub async fn dismiss(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DismissParams>,
) -> Result<Json<CollaborationFeed>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_dismiss_core(
            &state.db.conn,
            &state.emitter,
            params.conversation_id,
            params.delivery_id,
        )
        .await?,
    ))
}

pub async fn restore(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DismissParams>,
) -> Result<Json<CollaborationFeed>, AppCommandError> {
    Ok(Json(
        collaboration::collaboration_restore_core(
            &state.db.conn,
            &state.emitter,
            params.conversation_id,
            params.delivery_id,
        )
        .await?,
    ))
}
