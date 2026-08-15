use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::collaboration;
use crate::models::{CollaborationFeed, CollaborationSendResult, SendCollaborationMessageInput};

#[derive(Deserialize)]
pub struct SendParams {
    pub input: SendCollaborationMessageInput,
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
