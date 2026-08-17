use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::session_timer;
use crate::models::session_timer::{
    CreateSessionTimerInput, SessionTimerInfo, UpdateSessionTimerInput,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationParams {
    pub conversation_id: i32,
}

#[derive(Deserialize)]
pub struct CreateParams {
    pub input: CreateSessionTimerInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateParams {
    pub conversation_id: i32,
    pub id: String,
    pub input: UpdateSessionTimerInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteParams {
    pub conversation_id: i32,
    pub id: String,
}

pub async fn list(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<ConversationParams>,
) -> Result<Json<Vec<SessionTimerInfo>>, AppCommandError> {
    Ok(Json(
        session_timer::session_timer_list_core(&state.db.conn, params.conversation_id).await?,
    ))
}

pub async fn create(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<CreateParams>,
) -> Result<Json<SessionTimerInfo>, AppCommandError> {
    Ok(Json(
        session_timer::session_timer_create_core(
            &state.db.conn,
            &state.emitter,
            &state.session_timer,
            params.input,
        )
        .await?,
    ))
}

pub async fn update(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<UpdateParams>,
) -> Result<Json<SessionTimerInfo>, AppCommandError> {
    Ok(Json(
        session_timer::session_timer_update_core(
            &state.db.conn,
            &state.emitter,
            &state.session_timer,
            params.conversation_id,
            params.id,
            params.input,
        )
        .await?,
    ))
}

pub async fn delete(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DeleteParams>,
) -> Result<Json<()>, AppCommandError> {
    session_timer::session_timer_delete_core(
        &state.db.conn,
        &state.emitter,
        params.conversation_id,
        params.id,
    )
    .await?;
    Ok(Json(()))
}
