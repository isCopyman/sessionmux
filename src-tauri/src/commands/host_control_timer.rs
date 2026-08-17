//! Current-Session Timer actions for Codeg's progressive Host Control MCP.
//!
//! Caller identity is token-derived. No action accepts a Session id, so a
//! model cannot redirect a continuation loop to another Session.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::acp::host_control::{
    HostControlAccessLevel, HostControlCaller, HostControlCapability, HostControlUseOutcome,
};
use crate::db::service::session_timer_service;
use crate::db::AppDatabase;
use crate::models::session_timer::{
    CreateSessionTimerInput, SessionTimerInfo, UpdateSessionTimerInput, DEFAULT_IDLE_GRACE_SECS,
    MAX_IDLE_GRACE_SECS,
};
use crate::web::event_bridge::{
    emit_event, EventEmitter, SessionTimerChanged, SESSION_TIMER_CHANGED_EVENT,
};

pub struct TimerHostControl {
    db: Arc<AppDatabase>,
    emitter: EventEmitter,
}

impl TimerHostControl {
    pub fn new(db: Arc<AppDatabase>, emitter: EventEmitter) -> Self {
        Self { db, emitter }
    }

    pub fn capabilities(writes_allowed: bool) -> Vec<HostControlCapability> {
        let mut capabilities = vec![capability(
            "timer.list",
            "List idle continuation timers owned by the token-derived current Session.",
            HostControlAccessLevel::Read,
            empty_schema(),
        )];
        if writes_allowed {
            capabilities.extend([
                capability(
                    "timer.create",
                    "Keep the current Session advancing: after each completed Turn, enqueue this text as the next ordinary user follow-up until the Agent pauses or stops the timer.",
                    HostControlAccessLevel::Write,
                    json!({
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["prompt"],
                        "properties": {
                            "prompt": prompt_schema(),
                            "idle_grace_seconds": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": MAX_IDLE_GRACE_SECS,
                                "default": DEFAULT_IDLE_GRACE_SECS,
                                "description": "Short debounce after Turn completion; this is not a wall-clock schedule."
                            }
                        }
                    }),
                ),
                capability(
                    "timer.update",
                    "Replace the continuation text for one timer owned by the current Session.",
                    HostControlAccessLevel::Write,
                    json!({
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["timer_id", "prompt"],
                        "properties": {
                            "timer_id": timer_id_schema(),
                            "prompt": prompt_schema()
                        }
                    }),
                ),
                capability(
                    "timer.pause",
                    "Pause a current-Session continuation timer without deleting its text.",
                    HostControlAccessLevel::Write,
                    timer_target_schema(),
                ),
                capability(
                    "timer.resume",
                    "Resume a paused current-Session continuation timer.",
                    HostControlAccessLevel::Write,
                    timer_target_schema(),
                ),
                capability(
                    "timer.stop",
                    "Permanently stop and delete a current-Session continuation timer after the objective is complete.",
                    HostControlAccessLevel::Write,
                    timer_target_schema(),
                ),
            ]);
        }
        capabilities
    }

    pub fn access_for(action: &str) -> Option<HostControlAccessLevel> {
        match action {
            "timer.list" => Some(HostControlAccessLevel::Read),
            "timer.create" | "timer.update" | "timer.pause" | "timer.resume" | "timer.stop" => {
                Some(HostControlAccessLevel::Write)
            }
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
            "timer.list" => {
                if let Err(note) = parse_input::<EmptyInput>(&action, input) {
                    return HostControlUseOutcome::rejected(request_id, action, note);
                }
                match session_timer_service::list(&self.db.conn, caller.current_session_id).await {
                    Ok(timers) => accepted(request_id, action, "read", json!({ "timers": timers })),
                    Err(error) => rejected(request_id, action, error),
                }
            }
            "timer.create" => {
                let params = match parse_input::<CreateInput>(&action, input) {
                    Ok(params) => params,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                let result = session_timer_service::create(
                    &self.db.conn,
                    CreateSessionTimerInput {
                        conversation_id: caller.current_session_id,
                        prompt_text: params.prompt,
                        idle_grace_secs: params
                            .idle_grace_seconds
                            .unwrap_or(DEFAULT_IDLE_GRACE_SECS),
                        client_dedupe_id: Some(format!("host-timer-{request_id}")),
                    },
                )
                .await;
                self.finish_timer_write(request_id, action, result).await
            }
            "timer.update" => {
                let params = match parse_input::<UpdateInput>(&action, input) {
                    Ok(params) => params,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                self.update_owned(
                    caller,
                    request_id,
                    action,
                    params.timer_id,
                    Some(params.prompt),
                    None,
                )
                .await
            }
            "timer.pause" | "timer.resume" => {
                let params = match parse_input::<TimerTargetInput>(&action, input) {
                    Ok(params) => params,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                let enabled = action == "timer.resume";
                self.update_owned(
                    caller,
                    request_id,
                    action,
                    params.timer_id,
                    None,
                    Some(enabled),
                )
                .await
            }
            "timer.stop" => {
                let params = match parse_input::<TimerTargetInput>(&action, input) {
                    Ok(params) => params,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                let timer = match self.owned(caller, &params.timer_id).await {
                    Ok(timer) => timer,
                    Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
                };
                match session_timer_service::delete(&self.db.conn, &timer.id).await {
                    Ok(()) => {
                        self.publish(caller.current_session_id);
                        accepted(
                            request_id,
                            action,
                            "persisted",
                            json!({ "timer_id": timer.id, "stopped": true }),
                        )
                    }
                    Err(error) => rejected(request_id, action, error),
                }
            }
            _ => HostControlUseOutcome::rejected(
                request_id,
                action,
                "Unknown Timer Host Control action",
            ),
        }
    }

    async fn update_owned(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        timer_id: String,
        prompt_text: Option<String>,
        enabled: Option<bool>,
    ) -> HostControlUseOutcome {
        let timer = match self.owned(caller, &timer_id).await {
            Ok(timer) => timer,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let result = session_timer_service::update(
            &self.db.conn,
            &timer.id,
            UpdateSessionTimerInput {
                prompt_text,
                enabled,
                idle_grace_secs: None,
                expected_updated_at: timer.updated_at,
            },
        )
        .await;
        self.finish_timer_write(request_id, action, result).await
    }

    async fn owned(
        &self,
        caller: &HostControlCaller,
        timer_id: &str,
    ) -> Result<SessionTimerInfo, String> {
        let timer = session_timer_service::find(&self.db.conn, timer_id)
            .await
            .map_err(|error| format!("Timer not found: {error}"))?;
        if timer.conversation_id != caller.current_session_id {
            return Err("Timer does not belong to the calling Session".into());
        }
        Ok(timer)
    }

    async fn finish_timer_write(
        &self,
        request_id: String,
        action: String,
        result: Result<SessionTimerInfo, crate::db::error::DbError>,
    ) -> HostControlUseOutcome {
        match result {
            Ok(timer) => {
                self.publish(timer.conversation_id);
                accepted(request_id, action, "persisted", json!({ "timer": timer }))
            }
            Err(error) => rejected(request_id, action, error),
        }
    }

    fn publish(&self, conversation_id: i32) {
        emit_event(
            &self.emitter,
            SESSION_TIMER_CHANGED_EVENT,
            SessionTimerChanged {
                conversation_ids: vec![conversation_id],
            },
        );
    }
}

fn accepted(request_id: String, action: String, stage: &str, data: Value) -> HostControlUseOutcome {
    HostControlUseOutcome {
        accepted: true,
        request_id,
        action,
        stage: stage.into(),
        replayed: false,
        data,
        note: None,
    }
}

fn rejected(
    request_id: String,
    action: String,
    error: crate::db::error::DbError,
) -> HostControlUseOutcome {
    HostControlUseOutcome::rejected(
        request_id,
        action,
        format!("Timer operation failed: {error}"),
    )
}

fn capability(
    action: &str,
    description: &str,
    access: HostControlAccessLevel,
    input_schema: Value,
) -> HostControlCapability {
    let stage = match access {
        HostControlAccessLevel::Read => "read",
        HostControlAccessLevel::Write => "persisted",
    };
    HostControlCapability {
        action: action.into(),
        description: description.into(),
        access,
        input_schema,
        result_stages: vec![stage.into()],
    }
}

fn empty_schema() -> Value {
    json!({ "type": "object", "additionalProperties": false, "properties": {} })
}

fn prompt_schema() -> Value {
    json!({ "type": "string", "minLength": 1, "maxLength": 8000 })
}

fn timer_id_schema() -> Value {
    json!({ "type": "string", "minLength": 1, "maxLength": 200 })
}

fn timer_target_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["timer_id"],
        "properties": { "timer_id": timer_id_schema() }
    })
}

fn parse_input<T: DeserializeOwned>(action: &str, input: Value) -> Result<T, String> {
    serde_json::from_value(input).map_err(|error| format!("Invalid input for {action}: {error}"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateInput {
    prompt: String,
    #[serde(default)]
    idle_grace_seconds: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateInput {
    timer_id: String,
    prompt: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimerTargetInput {
    timer_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::AgentType;

    #[tokio::test]
    async fn agent_can_manage_only_its_own_continuation() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/timer-host").await;
        let current = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let other = seed_conversation(&db, folder_id, AgentType::ClaudeCode).await;
        let host = TimerHostControl::new(Arc::new(db), EventEmitter::Noop);
        let caller = HostControlCaller {
            current_session_id: current,
            working_dir: std::path::PathBuf::from("/tmp/timer-host"),
            writes_allowed: true,
        };

        let created = host
            .use_action(
                &caller,
                "create-1".into(),
                "timer.create".into(),
                json!({ "prompt": "Read PLAN.md and continue" }),
            )
            .await;
        assert!(created.accepted);
        let timer_id = created.data["timer"]["id"].as_str().unwrap().to_string();

        let paused = host
            .use_action(
                &caller,
                "pause-1".into(),
                "timer.pause".into(),
                json!({ "timer_id": timer_id }),
            )
            .await;
        assert_eq!(paused.data["timer"]["enabled"], false);

        let foreign = session_timer_service::create(
            &host.db.conn,
            CreateSessionTimerInput {
                conversation_id: other,
                prompt_text: "foreign".into(),
                idle_grace_secs: 2,
                client_dedupe_id: Some("foreign-timer".into()),
            },
        )
        .await
        .unwrap();
        let rejected = host
            .use_action(
                &caller,
                "stop-foreign".into(),
                "timer.stop".into(),
                json!({ "timer_id": foreign.id }),
            )
            .await;
        assert!(!rejected.accepted);
    }
}
