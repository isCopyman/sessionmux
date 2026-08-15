use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::workbenches;
use crate::models::WorkbenchInfo;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkbenchParams {
    pub name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameWorkbenchParams {
    pub id: i32,
    pub name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteWorkbenchParams {
    pub id: i32,
}

pub async fn list_workbenches(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<Vec<WorkbenchInfo>>, AppCommandError> {
    Ok(Json(
        workbenches::list_workbenches_core(&state.db.conn).await?,
    ))
}

pub async fn create_workbench(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<CreateWorkbenchParams>,
) -> Result<Json<WorkbenchInfo>, AppCommandError> {
    Ok(Json(
        workbenches::create_workbench_core(&state.db.conn, params.name).await?,
    ))
}

pub async fn rename_workbench(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<RenameWorkbenchParams>,
) -> Result<Json<WorkbenchInfo>, AppCommandError> {
    Ok(Json(
        workbenches::rename_workbench_core(&state.db.conn, params.id, params.name).await?,
    ))
}

pub async fn delete_workbench(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<DeleteWorkbenchParams>,
) -> Result<Json<()>, AppCommandError> {
    workbenches::delete_workbench_core(&state.db.conn, params.id).await?;
    Ok(Json(()))
}
