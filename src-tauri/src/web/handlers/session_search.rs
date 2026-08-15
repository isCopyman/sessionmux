use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::session_search;
use crate::models::{AgentType, SessionContentSearchResponse};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSessionContentParams {
    pub query: String,
    pub folder_ids: Option<Vec<i32>>,
    pub agent_type: Option<AgentType>,
    pub limit: Option<usize>,
}

pub async fn search_session_content(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<SearchSessionContentParams>,
) -> Result<Json<SessionContentSearchResponse>, AppCommandError> {
    Ok(Json(
        session_search::search_session_content_core(
            &state.db.conn,
            params.query,
            params.folder_ids,
            params.agent_type,
            params.limit,
        )
        .await?,
    ))
}
