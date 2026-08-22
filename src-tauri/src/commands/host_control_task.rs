//! Task-board actions for Codeg's progressive Host Control MCP.
//!
//! The caller is always the token-derived persistent Session. Tasks remain
//! project-scoped, and claiming uses the same atomic assignment + PromptQueue
//! path as the human UI.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::acp::host_control::{
    HostControlAccessLevel, HostControlCaller, HostControlCapability, HostControlUseOutcome,
};
use crate::acp::types::PromptInputBlock;
use crate::commands::work_task::work_task_assign_session_as_core;
use crate::db::service::{conversation_service, folder_service, work_task_service};
use crate::db::AppDatabase;
use crate::models::work_task::{WorkTaskConfig, WorkTaskDraft};
use crate::prompt_queue::PromptQueueHandle;
use crate::web::event_bridge::{emit_event, EventEmitter, WorkTaskChange, WORK_TASK_CHANGED_EVENT};

const MAX_TITLE_CHARS: usize = 200;
const MAX_DESCRIPTION_CHARS: usize = 50_000;

pub struct TaskHostControl {
    db: Arc<AppDatabase>,
    emitter: EventEmitter,
    prompt_queue: PromptQueueHandle,
}

impl TaskHostControl {
    pub fn new(
        db: Arc<AppDatabase>,
        emitter: EventEmitter,
        prompt_queue: PromptQueueHandle,
    ) -> Self {
        Self {
            db,
            emitter,
            prompt_queue,
        }
    }

    pub fn capabilities(writes_allowed: bool) -> Vec<HostControlCapability> {
        if !writes_allowed {
            return Vec::new();
        }
        vec![
            capability(
                "task.claim",
                "Claim an unowned board card for the current persistent Session. Its current column is not a gate: the task is moved to in-progress and atomically queued as this Session's next ordinary prompt.",
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["task_id"],
                    "properties": { "task_id": task_id_schema() }
                }),
            ),
            capability(
                "task.assign",
                "Assign an unowned board card to another persistent Session in the current project. Omit target_session_id to claim it for the caller. The task moves to in-progress and is atomically queued to the chosen Session.",
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["task_id"],
                    "properties": {
                        "task_id": task_id_schema(),
                        "target_session_id": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": i32::MAX,
                            "description": "Stable target Session id. Omit to claim for the token-derived current Session."
                        }
                    }
                }),
            ),
            capability(
                "task.update",
                "Edit the title and/or description of an unowned board card, or the calling Session's own active card, in the current project. A Session cannot rewrite another Session's task brief.",
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["task_id"],
                    "properties": {
                        "task_id": task_id_schema(),
                        "title": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": MAX_TITLE_CHARS
                        },
                        "description": {
                            "type": "string",
                            "maxLength": MAX_DESCRIPTION_CHARS
                        }
                    },
                    "anyOf": [
                        { "required": ["title"] },
                        { "required": ["description"] }
                    ]
                }),
            ),
        ]
    }

    pub fn access_for(action: &str) -> Option<HostControlAccessLevel> {
        match action {
            "task.claim" | "task.assign" | "task.update" => Some(HostControlAccessLevel::Write),
            _ => None,
        }
    }

    pub async fn use_action(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        match action.as_str() {
            "task.claim" | "task.assign" => {
                let params = match parse_input::<TaskAssignInput>(&action, input) {
                    Ok(params) => params,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                if let Err(note) = self.ensure_same_project(caller, params.task_id).await {
                    return HostControlUseOutcome::rejected(request_id, action, note);
                }
                let target_session_id = params
                    .target_session_id
                    .unwrap_or(caller.current_session_id);
                match work_task_assign_session_as_core(
                    &self.emitter,
                    &self.db,
                    &self.prompt_queue,
                    params.task_id,
                    target_session_id,
                    "agent",
                )
                .await
                {
                    Ok(task) => accepted(
                        request_id,
                        action,
                        "queued",
                        json!({
                            "task_id": task.id,
                            "session_id": target_session_id,
                            "task_status": task.task_status,
                            "execution_mode": task.execution_mode,
                        }),
                    ),
                    Err(error) => HostControlUseOutcome::rejected(
                        request_id,
                        action,
                        format!("Could not claim task: {error}"),
                    ),
                }
            }
            "task.update" => {
                let params = match parse_input::<TaskUpdateInput>(&action, input) {
                    Ok(params) => params,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                if params.title.is_none() && params.description.is_none() {
                    return HostControlUseOutcome::rejected(
                        request_id,
                        action,
                        "Provide title and/or description.",
                    );
                }
                let current = match self.ensure_same_project(caller, params.task_id).await {
                    Ok(task) => task,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                let is_unowned_card = current.execution_mode.is_none()
                    || current.execution_mode
                        == Some(crate::db::entities::work_task::WorkTaskExecutionMode::Manual);
                let is_owned_by_caller = current.execution_mode
                    == Some(crate::db::entities::work_task::WorkTaskExecutionMode::Session)
                    && current.conversation_id == Some(caller.current_session_id)
                    && matches!(
                        current.task_status,
                        crate::db::entities::work_task::WorkTaskBusinessStatus::InProgress
                            | crate::db::entities::work_task::WorkTaskBusinessStatus::Blocked
                    );
                if !is_unowned_card && !is_owned_by_caller {
                    return HostControlUseOutcome::rejected(
                        request_id,
                        action,
                        "Only an unowned board card or the calling Session's own active card can be edited.",
                    );
                }
                let title = match params.title {
                    Some(value) => match normalized(&value, MAX_TITLE_CHARS, "title", false) {
                        Ok(value) => value,
                        Err(note) => {
                            return HostControlUseOutcome::rejected(request_id, action, note)
                        }
                    },
                    None => current.title.clone(),
                };
                let mut config: WorkTaskConfig = match serde_json::from_value(current.config) {
                    Ok(config) => config,
                    Err(error) => {
                        return HostControlUseOutcome::rejected(
                            request_id,
                            action,
                            format!("Task brief is invalid: {error}"),
                        )
                    }
                };
                if let Some(description) = params.description {
                    let description = match normalized(
                        &description,
                        MAX_DESCRIPTION_CHARS,
                        "description",
                        true,
                    ) {
                        Ok(value) => value,
                        Err(note) => {
                            return HostControlUseOutcome::rejected(request_id, action, note)
                        }
                    };
                    config.display_text = description.clone();
                    config.prompt_blocks = if description.is_empty() {
                        Vec::new()
                    } else {
                        vec![
                            serde_json::to_value(PromptInputBlock::Text { text: description })
                                .expect("text prompt blocks serialize"),
                        ]
                    };
                }
                let draft = WorkTaskDraft {
                    folder_id: current.folder_id,
                    title,
                    config: match serde_json::to_value(config) {
                        Ok(value) => value,
                        Err(error) => {
                            return HostControlUseOutcome::rejected(
                                request_id,
                                action,
                                format!("Could not encode task brief: {error}"),
                            )
                        }
                    },
                    initial_status: None,
                };
                match work_task_service::update(&self.db.conn, params.task_id, draft).await {
                    Ok(task) => {
                        emit_event(
                            &self.emitter,
                            WORK_TASK_CHANGED_EVENT,
                            WorkTaskChange::Upsert { id: task.id },
                        );
                        accepted(
                            request_id,
                            action,
                            "persisted",
                            json!({
                                "task_id": task.id,
                                "title": task.title,
                                "task_status": task.task_status,
                                "updated_at": task.updated_at,
                            }),
                        )
                    }
                    Err(error) => HostControlUseOutcome::rejected(
                        request_id,
                        action,
                        format!("Could not update task: {error}"),
                    ),
                }
            }
            _ => HostControlUseOutcome::rejected(request_id, action, "Unknown task action."),
        }
    }

    async fn ensure_same_project(
        &self,
        caller: &HostControlCaller,
        task_id: i32,
    ) -> Result<crate::models::work_task::WorkTaskInfo, String> {
        if task_id <= 0 {
            return Err("task_id must be a positive integer".to_string());
        }
        let session = conversation_service::get_by_id(&self.db.conn, caller.current_session_id)
            .await
            .map_err(|_| "The calling Session is no longer available.".to_string())?;
        let folder = folder_service::get_folder_by_id(&self.db.conn, session.folder_id)
            .await
            .map_err(|_| "The calling Session project is no longer available.".to_string())?
            .ok_or_else(|| "The calling Session project is no longer available.".to_string())?;
        let root_folder_id = folder.parent_id.unwrap_or(folder.id);
        let task = work_task_service::get(&self.db.conn, task_id)
            .await
            .map_err(|_| format!("Task {task_id} was not found."))?;
        if task.folder_id != root_folder_id {
            return Err(format!(
                "Task {task_id} is outside the calling Session's current project."
            ));
        }
        Ok(task)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskAssignInput {
    task_id: i32,
    #[serde(default)]
    target_session_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskUpdateInput {
    task_id: i32,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

fn parse_input<T: DeserializeOwned>(action: &str, input: Value) -> Result<T, String> {
    serde_json::from_value(input).map_err(|error| format!("Invalid input for {action}: {error}"))
}

fn normalized(value: &str, max: usize, label: &str, allow_empty: bool) -> Result<String, String> {
    let value = value.trim();
    if !allow_empty && value.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if value.chars().count() > max {
        return Err(format!("{label} must be at most {max} characters"));
    }
    Ok(value.to_string())
}

fn capability(action: &str, description: &str, input_schema: Value) -> HostControlCapability {
    HostControlCapability {
        action: action.to_string(),
        description: description.to_string(),
        access: HostControlAccessLevel::Write,
        input_schema,
        result_stages: vec!["persisted".to_string(), "queued".to_string()],
    }
}

fn task_id_schema() -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": i32::MAX,
        "description": "Task id returned by list_tasks or create_work_task."
    })
}

fn accepted(request_id: String, action: String, stage: &str, data: Value) -> HostControlUseOutcome {
    HostControlUseOutcome {
        accepted: true,
        request_id,
        action,
        stage: stage.to_string(),
        replayed: false,
        data,
        note: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::service::prompt_queue_service;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::agent::AgentType;

    async fn fixture() -> (TaskHostControl, HostControlCaller, i32, Arc<AppDatabase>) {
        let db = Arc::new(fresh_in_memory_db().await);
        let folder_id = seed_folder(&db, "/tmp/task-host-control").await;
        let session_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let task = work_task_service::create(
            &db.conn,
            WorkTaskDraft {
                folder_id,
                title: "Initial title".to_string(),
                initial_status: None,
                config: serde_json::to_value(WorkTaskConfig {
                    display_text: "Initial description".to_string(),
                    prompt_blocks: vec![serde_json::to_value(PromptInputBlock::Text {
                        text: "Initial description".to_string(),
                    })
                    .unwrap()],
                    ..Default::default()
                })
                .unwrap(),
            },
        )
        .await
        .unwrap();
        let host = TaskHostControl::new(
            db.clone(),
            EventEmitter::Noop,
            PromptQueueHandle::disconnected_for_test(),
        );
        let caller = HostControlCaller {
            current_session_id: session_id,
            working_dir: "/tmp/task-host-control".into(),
            writes_allowed: true,
        };
        (host, caller, task.id, db)
    }

    #[tokio::test]
    async fn agent_can_refine_then_atomically_claim_an_unassigned_card() {
        let (host, caller, task_id, db) = fixture().await;
        let updated = host
            .use_action(
                &caller,
                "update-1".into(),
                "task.update".into(),
                json!({
                    "task_id": task_id,
                    "title": "Refined title",
                    "description": "Refined brief",
                }),
            )
            .await;
        assert!(updated.accepted, "{:?}", updated.note);
        let stored = work_task_service::get(&db.conn, task_id).await.unwrap();
        assert_eq!(stored.title, "Refined title");
        let config: WorkTaskConfig = serde_json::from_value(stored.config).unwrap();
        assert_eq!(config.display_text, "Refined brief");

        let claimed = host
            .use_action(
                &caller,
                "claim-1".into(),
                "task.claim".into(),
                json!({ "task_id": task_id }),
            )
            .await;
        assert!(claimed.accepted, "{:?}", claimed.note);
        assert_eq!(claimed.stage, "queued");
        let stored = work_task_service::get(&db.conn, task_id).await.unwrap();
        assert_eq!(stored.conversation_id, Some(caller.current_session_id));
        assert_eq!(
            stored.task_status,
            crate::db::entities::work_task::WorkTaskBusinessStatus::InProgress
        );
        let queue = prompt_queue_service::snapshot(&db.conn, caller.current_session_id)
            .await
            .unwrap();
        assert_eq!(queue.items.len(), 1);
        assert_eq!(queue.items[0].task_id, Some(task_id));
    }

    #[tokio::test]
    async fn another_session_cannot_rewrite_a_claimed_card() {
        let (host, caller, task_id, _) = fixture().await;
        assert!(
            host.use_action(
                &caller,
                "claim-2".into(),
                "task.claim".into(),
                json!({ "task_id": task_id }),
            )
            .await
            .accepted
        );
        let other_session = seed_conversation(
            &host.db,
            work_task_service::get(&host.db.conn, task_id)
                .await
                .unwrap()
                .folder_id,
            AgentType::ClaudeCode,
        )
        .await;
        let other = HostControlCaller {
            current_session_id: other_session,
            working_dir: "/tmp/task-host-control".into(),
            writes_allowed: true,
        };
        let denied = host
            .use_action(
                &other,
                "update-2".into(),
                "task.update".into(),
                json!({ "task_id": task_id, "title": "Rewrite" }),
            )
            .await;
        assert!(!denied.accepted);
        assert!(denied.note.unwrap().contains("own active"));
    }

    #[tokio::test]
    async fn agent_can_refine_and_directly_claim_backlog() {
        let (host, caller, _, db) = fixture().await;
        let session = conversation_service::get_by_id(&db.conn, caller.current_session_id)
            .await
            .unwrap();
        let task_id = work_task_service::create(
            &db.conn,
            WorkTaskDraft {
                folder_id: session.folder_id,
                title: "Parked idea".into(),
                initial_status: Some(
                    crate::db::entities::work_task::WorkTaskBusinessStatus::Backlog,
                ),
                config: serde_json::to_value(WorkTaskConfig {
                    display_text: "Not ready to run".into(),
                    prompt_blocks: vec![],
                    ..Default::default()
                })
                .unwrap(),
            },
        )
        .await
        .unwrap()
        .id;

        let updated = host
            .use_action(
                &caller,
                "update-backlog".into(),
                "task.update".into(),
                json!({
                    "task_id": task_id,
                    "title": "Refined parked idea",
                }),
            )
            .await;
        assert!(updated.accepted, "{:?}", updated.note);

        let claimed = host
            .use_action(
                &caller,
                "claim-backlog".into(),
                "task.claim".into(),
                json!({ "task_id": task_id }),
            )
            .await;
        assert!(claimed.accepted, "{:?}", claimed.note);
        let stored = work_task_service::get(&db.conn, task_id).await.unwrap();
        assert_eq!(
            stored.task_status,
            crate::db::entities::work_task::WorkTaskBusinessStatus::InProgress
        );
        assert_eq!(
            stored.execution_mode,
            Some(crate::db::entities::work_task::WorkTaskExecutionMode::Session)
        );
        assert_eq!(
            prompt_queue_service::snapshot(&db.conn, caller.current_session_id)
                .await
                .unwrap()
                .items
                .len(),
            1
        );
    }
}
