//! Persistent Session lifecycle provider for the progressive Host Control
//! gateway. The catalog/dispatcher in `host_control.rs` stays thin; all ACP,
//! path-scope, cleanup, and lifecycle semantics live here.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::acp::connection::PREFERRED_MODEL_CONFIG_KEY;
use crate::acp::host_control::{
    HostControlAccessLevel, HostControlCaller, HostControlCapability, HostControlUseOutcome,
};
use crate::acp::manager::ConnectionManager;
use crate::acp::types::{
    AgentOptionsSnapshot, PromptInputBlock, SessionConfigKindInfo, SessionConfigOptionInfo,
};
use crate::commands::acp::{acp_cancel_core, build_session_runtime_env, verify_agent_installed};
use crate::commands::conversations::{
    create_conversation_core_with_source, emit_conversation_deleted, emit_conversation_upsert,
};
use crate::db::entities::conversation::{
    ConversationKind, ConversationStatus, CREATED_BY_AGENT,
};
use crate::db::service::{conversation_service, folder_service, prompt_queue_service};
use crate::db::AppDatabase;
use crate::models::{AgentType, DbConversationSummary};
use crate::web::event_bridge::{emit_event, EventEmitter, PROMPT_QUEUE_CHANGED_EVENT};

const MAX_TITLE_CHARS: usize = 200;
const MAX_PROMPT_CHARS: usize = 100_000;
const MAX_MODEL_CHARS: usize = 256;
const MAX_MODE_CHARS: usize = 128;
const MAX_CONFIG_ENTRIES: usize = 32;
const MAX_CONFIG_KEY_CHARS: usize = 128;
const MAX_CONFIG_VALUE_CHARS: usize = 512;
const MAX_IDEMPOTENCY_ENTRIES: usize = 512;
const SELECTOR_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
use crate::db::service::prompt_queue_service::{
    HOST_CANCEL_PAUSE_REASON as CANCEL_QUEUE_PAUSE_REASON,
    HOST_STOP_PAUSE_REASON as STOP_QUEUE_PAUSE_REASON,
};

#[derive(Clone)]
struct CachedWrite {
    fingerprint: String,
    outcome: HostControlUseOutcome,
}

type WriteSlot = Arc<Mutex<Option<CachedWrite>>>;

#[derive(Default)]
struct IdempotencyRegistry {
    entries: HashMap<String, WriteSlot>,
    order: VecDeque<String>,
}

impl IdempotencyRegistry {
    fn slot(&mut self, request_id: &str) -> WriteSlot {
        if let Some(slot) = self.entries.get(request_id) {
            return Arc::clone(slot);
        }
        let mut scanned = 0usize;
        while self.entries.len() >= MAX_IDEMPOTENCY_ENTRIES && scanned < self.order.len() {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            let removable = self
                .entries
                .get(&oldest)
                .is_some_and(|slot| Arc::strong_count(slot) == 1);
            if removable {
                self.entries.remove(&oldest);
            } else {
                self.order.push_back(oldest);
            }
            scanned += 1;
        }
        let slot = Arc::new(Mutex::new(None));
        self.entries
            .insert(request_id.to_string(), Arc::clone(&slot));
        self.order.push_back(request_id.to_string());
        slot
    }
}

#[derive(Debug, Clone)]
struct CreateSessionSpec {
    conversation_id: i32,
    folder_id: i32,
    cwd: String,
    harness: AgentType,
    model: Option<String>,
    mode_id: Option<String>,
    config_values: BTreeMap<String, String>,
    initial_prompt: Option<String>,
    request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InitialPromptStage {
    NotRequested,
    TurnStarted,
    DispatchUnknown,
    Failed(String),
}

#[derive(Debug, Clone)]
struct CreatedRuntime {
    connection_id: String,
    native_session_id: Option<String>,
    actual_model: Option<String>,
    prompt: InitialPromptStage,
    runtime_active: bool,
    identity_persisted: bool,
    setup_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CancelTurnRuntimeResult {
    NoActiveRuntime,
    NoActiveTurn { connection_id: String },
    CancelRequested { connection_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StopSessionRuntimeResult {
    AlreadyStopped,
    StopRequested {
        connection_id: String,
        cancelled_turn: bool,
    },
}

#[async_trait]
trait HostSessionRuntime: Send + Sync {
    async fn create_session(&self, spec: CreateSessionSpec) -> Result<CreatedRuntime, String>;
    async fn cancel_turn(&self, conversation_id: i32) -> Result<CancelTurnRuntimeResult, String>;
    async fn stop_session(&self, conversation_id: i32) -> Result<StopSessionRuntimeResult, String>;
}

struct ManagedAcpSessionRuntime {
    db: Arc<AppDatabase>,
    manager: ConnectionManager,
    emitter: EventEmitter,
    data_dir: PathBuf,
}

impl ManagedAcpSessionRuntime {
    fn new(
        db: Arc<AppDatabase>,
        manager: ConnectionManager,
        emitter: EventEmitter,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            db,
            manager,
            emitter,
            data_dir,
        }
    }

    async fn pause_queue_if_pending(
        &self,
        conversation_id: i32,
        reason: &str,
    ) -> Result<(), String> {
        match prompt_queue_service::pause_queue_if_pending(
            &self.db.conn,
            conversation_id,
            reason.to_string(),
        )
        .await
        {
            Ok(Some(snapshot)) => {
                emit_event(&self.emitter, PROMPT_QUEUE_CHANGED_EVENT, snapshot);
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(error) => Err(format!("Could not pause pending Session prompts: {error}")),
        }
    }
}

#[async_trait]
impl HostSessionRuntime for ManagedAcpSessionRuntime {
    async fn create_session(&self, spec: CreateSessionSpec) -> Result<CreatedRuntime, String> {
        verify_agent_installed(spec.harness)
            .await
            .map_err(|error| error.to_string())?;
        let runtime_env = build_session_runtime_env(&self.db, spec.harness, None, &self.data_dir)
            .await
            .map_err(|error| error.to_string())?;

        let mut preferred_config_values = spec.config_values.clone();
        if let Some(model) = spec.model.as_ref() {
            preferred_config_values.insert(PREFERRED_MODEL_CONFIG_KEY.to_string(), model.clone());
        }
        let needs_selector_verification =
            spec.model.is_some() || spec.mode_id.is_some() || !spec.config_values.is_empty();
        let spawn = self
            .manager
            .spawn_bound_agent(
                &self.db,
                spec.harness,
                spec.cwd.clone(),
                runtime_env,
                "host-control-background".to_string(),
                self.emitter.clone(),
                spec.mode_id.clone(),
                preferred_config_values,
                spec.conversation_id,
                spec.folder_id,
            )
            .await;
        let (connection_id, native_session_id, identity_persisted) = match spawn {
            Ok((connection_id, native_session_id)) => {
                (connection_id, Some(native_session_id), true)
            }
            Err(failure) => {
                if !failure.native_creation_may_have_started {
                    return Err(failure.error.to_string());
                }
                return Ok(CreatedRuntime {
                    connection_id: failure.connection_id,
                    native_session_id: failure.native_session_id,
                    actual_model: None,
                    prompt: InitialPromptStage::NotRequested,
                    runtime_active: false,
                    identity_persisted: false,
                    setup_error: Some(failure.error.to_string()),
                });
            }
        };

        let options = if needs_selector_verification {
            match self
                .manager
                .wait_for_session_options(&connection_id, SELECTOR_WAIT_TIMEOUT)
                .await
            {
                Ok(options) => Some(options),
                Err(error) => {
                    let _ = self.manager.disconnect(&connection_id).await;
                    return Ok(CreatedRuntime {
                        connection_id,
                        native_session_id,
                        actual_model: None,
                        prompt: InitialPromptStage::NotRequested,
                        runtime_active: false,
                        identity_persisted,
                        setup_error: Some(format!(
                            "The requested model/config could not be verified: {error}"
                        )),
                    });
                }
            }
        } else {
            None
        };
        if let Some(options) = options.as_ref() {
            if let Err(error) = verify_requested_options(
                options,
                spec.model.as_deref(),
                spec.mode_id.as_deref(),
                &spec.config_values,
            ) {
                let _ = self.manager.disconnect(&connection_id).await;
                return Ok(CreatedRuntime {
                    connection_id,
                    native_session_id,
                    actual_model: current_model(options),
                    prompt: InitialPromptStage::NotRequested,
                    runtime_active: false,
                    identity_persisted,
                    setup_error: Some(error),
                });
            }
        }
        let actual_model = options.as_ref().and_then(current_model);

        let prompt = match spec.initial_prompt {
            None => InitialPromptStage::NotRequested,
            Some(prompt) => {
                let client_message_id = format!("host-control:{}", spec.request_id);
                match self
                    .manager
                    .send_prompt_linked_with_message_id(
                        &self.db,
                        &connection_id,
                        vec![PromptInputBlock::Text { text: prompt }],
                        Some(spec.folder_id),
                        Some(spec.conversation_id),
                        Some(client_message_id),
                        true,
                    )
                    .await
                {
                    Ok(_) => InitialPromptStage::TurnStarted,
                    Err(crate::acp::error::AcpError::DispatchUncertain) => {
                        InitialPromptStage::DispatchUnknown
                    }
                    Err(error) => InitialPromptStage::Failed(error.to_string()),
                }
            }
        };

        Ok(CreatedRuntime {
            connection_id,
            native_session_id,
            actual_model,
            prompt,
            runtime_active: true,
            identity_persisted,
            setup_error: None,
        })
    }

    async fn cancel_turn(&self, conversation_id: i32) -> Result<CancelTurnRuntimeResult, String> {
        let Some(connection_id) = self
            .manager
            .find_connection_by_conversation_id(conversation_id)
            .await
        else {
            return Ok(CancelTurnRuntimeResult::NoActiveRuntime);
        };
        let turn_in_flight = match self.manager.get_state(&connection_id).await {
            Some(state) => state.read().await.turn_in_flight,
            None => return Ok(CancelTurnRuntimeResult::NoActiveRuntime),
        };
        if !turn_in_flight {
            return Ok(CancelTurnRuntimeResult::NoActiveTurn { connection_id });
        }
        self.pause_queue_if_pending(conversation_id, CANCEL_QUEUE_PAUSE_REASON)
            .await?;
        acp_cancel_core(&self.db, &self.manager, &self.emitter, &connection_id)
            .await
            .map_err(|error| error.to_string())?;
        Ok(CancelTurnRuntimeResult::CancelRequested { connection_id })
    }

    async fn stop_session(&self, conversation_id: i32) -> Result<StopSessionRuntimeResult, String> {
        let Some(connection_id) = self
            .manager
            .find_connection_by_conversation_id(conversation_id)
            .await
        else {
            self.pause_queue_if_pending(conversation_id, STOP_QUEUE_PAUSE_REASON)
                .await?;
            return Ok(StopSessionRuntimeResult::AlreadyStopped);
        };
        let turn_in_flight = match self.manager.get_state(&connection_id).await {
            Some(state) => state.read().await.turn_in_flight,
            None => false,
        };
        self.pause_queue_if_pending(conversation_id, STOP_QUEUE_PAUSE_REASON)
            .await?;

        // Cancel is intentionally sent even when idle. We have already paused
        // the queue with a propagated result above; the shared core rechecks it
        // and an idle cancel remains harmless. Disconnect then stops only the
        // runtime; the persistent Session and native transcript stay resumable.
        let cancel_error = acp_cancel_core(&self.db, &self.manager, &self.emitter, &connection_id)
            .await
            .err();
        let disconnect_result = self.manager.disconnect(&connection_id).await;
        if let Err(error) = disconnect_result {
            if !matches!(error, crate::acp::error::AcpError::ConnectionNotFound(_)) {
                return Err(error.to_string());
            }
        }
        if let Some(error) = cancel_error {
            tracing::warn!(
                conversation_id,
                connection_id = %connection_id,
                "Host stop disconnected the runtime after cancel reported: {error}"
            );
        }
        Ok(StopSessionRuntimeResult::StopRequested {
            connection_id,
            cancelled_turn: turn_in_flight,
        })
    }
}

pub(crate) struct SessionHostControlProvider {
    db: Arc<AppDatabase>,
    emitter: EventEmitter,
    runtime: Arc<dyn HostSessionRuntime>,
    writes: Mutex<IdempotencyRegistry>,
}

impl SessionHostControlProvider {
    pub(crate) fn new(
        db: Arc<AppDatabase>,
        emitter: EventEmitter,
        manager: ConnectionManager,
        data_dir: PathBuf,
    ) -> Self {
        let runtime = Arc::new(ManagedAcpSessionRuntime::new(
            Arc::clone(&db),
            manager,
            emitter.clone(),
            data_dir,
        ));
        Self {
            db,
            emitter,
            runtime,
            writes: Mutex::new(IdempotencyRegistry::default()),
        }
    }

    #[cfg(any(test, feature = "test-utils"))]
    fn new_with_runtime(
        db: Arc<AppDatabase>,
        emitter: EventEmitter,
        runtime: Arc<dyn HostSessionRuntime>,
    ) -> Self {
        Self {
            db,
            emitter,
            runtime,
            writes: Mutex::new(IdempotencyRegistry::default()),
        }
    }

    /// Public-path tests still go through Host Core; only the ACP spawn is
    /// isolated so the suite does not require a live Harness binary.
    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn isolated_for_tests(db: Arc<AppDatabase>, emitter: EventEmitter) -> Self {
        Self::new_with_runtime(db, emitter, Arc::new(IsolatedHostSessionRuntime))
    }

    pub(crate) fn handles(action: &str) -> bool {
        matches!(
            action,
            "session.create" | "session.cancel_turn" | "session.stop"
        )
    }

    pub(crate) fn capabilities() -> Vec<HostControlCapability> {
        vec![
            HostControlCapability {
                action: "session.create".to_string(),
                description: "Create a persistent managed Session in the caller's current project/cwd. The Host starts a real ACP/native Session, returns its stable Codeg Session id, optionally starts one initial prompt, and never opens or focuses UI by default. Model is resolved through the Harness-advertised model capability; provider-specific settings use config_values."
                    .to_string(),
                access: HostControlAccessLevel::Write,
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["harness"],
                    "properties": {
                        "harness": {
                            "type": "string",
                            "description": "Agent/Harness wire id such as codex or claude_code."
                        },
                        "folder_id": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": i32::MAX,
                            "description": "Existing folder id. Omit for the caller's folder; other project scopes are rejected."
                        },
                        "cwd": {
                            "type": "string",
                            "description": "Existing working directory. Omit for the token-bound caller cwd; a different cwd is rejected."
                        },
                        "title": {
                            "type": "string",
                            "maxLength": MAX_TITLE_CHARS
                        },
                        "model": {
                            "type": "string",
                            "maxLength": MAX_MODEL_CHARS,
                            "description": "Optional model value verified against the Harness-advertised model selector."
                        },
                        "mode_id": {
                            "type": "string",
                            "maxLength": MAX_MODE_CHARS
                        },
                        "config_values": {
                            "type": "object",
                            "maxProperties": MAX_CONFIG_ENTRIES,
                            "additionalProperties": {
                                "type": "string",
                                "maxLength": MAX_CONFIG_VALUE_CHARS
                            },
                            "description": "Optional exact Harness config-option id to value mappings. Every requested value is verified before success."
                        },
                        "initial_prompt": {
                            "type": "string",
                            "maxLength": MAX_PROMPT_CHARS,
                            "description": "Optional first user prompt. Creation without this still persists the native Session identity."
                        },
                        "collection_id": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": i32::MAX,
                            "description": "Optional primary Collection in the caller's Path scope. Placement runs after the Session exists; a placement failure does not delete the Session."
                        }
                    }
                }),
                result_stages: vec![
                    "created".to_string(),
                    "turn_started".to_string(),
                    "prompt_dispatch_unknown".to_string(),
                    "created_prompt_failed".to_string(),
                    "created_setup_failed".to_string(),
                    "creation_failed_cleaned".to_string(),
                    "creation_failed_cleanup_required".to_string(),
                    "creation_recovery_required".to_string(),
                ],
            },
            HostControlCapability {
                action: "session.cancel_turn".to_string(),
                description: "Cancel only the target Session's current Turn. Pending follow-ups are paused first; the persistent Session and runtime stay available. Idle or unloaded targets return an explicit no-op stage."
                    .to_string(),
                access: HostControlAccessLevel::Write,
                input_schema: target_session_schema(),
                result_stages: vec![
                    "cancel_requested".to_string(),
                    "no_active_turn".to_string(),
                    "no_active_runtime".to_string(),
                ],
            },
            HostControlCapability {
                action: "session.stop".to_string(),
                description: "Stop the target Session's current managed runtime: pause pending follow-ups, cancel the current Turn if present, and request runtime disconnect. This preserves the Codeg Session, native transcript, and resume identity; it never deletes or archives."
                    .to_string(),
                access: HostControlAccessLevel::Write,
                input_schema: target_session_schema(),
                result_stages: vec![
                    "runtime_stop_requested".to_string(),
                    "already_stopped".to_string(),
                ],
            },
        ]
    }

    pub(crate) async fn dispatch(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let fingerprint = write_fingerprint(&action, &input);
        let slot = self.writes.lock().await.slot(&request_id);
        let mut cached = slot.lock().await;
        if let Some(prior) = cached.as_ref() {
            if prior.fingerprint != fingerprint {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    "request_id was already used with different action input",
                );
            }
            let mut outcome = prior.outcome.clone();
            outcome.replayed = true;
            return outcome;
        }

        let outcome = match action.as_str() {
            "session.create" => {
                self.create(caller, request_id.clone(), action.clone(), input)
                    .await
            }
            "session.cancel_turn" => {
                self.cancel_turn(caller, request_id.clone(), action.clone(), input)
                    .await
            }
            "session.stop" => {
                self.stop(caller, request_id.clone(), action.clone(), input)
                    .await
            }
            _ => HostControlUseOutcome::rejected(
                request_id.clone(),
                action.clone(),
                "Unknown Session lifecycle action",
            ),
        };
        *cached = Some(CachedWrite {
            fingerprint,
            outcome: outcome.clone(),
        });
        outcome
    }

    async fn create(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: SessionCreateInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let (caller_session, cwd) = match self
            .resolve_create_scope(caller, params.folder_id, params.cwd)
            .await
        {
            Ok(scope) => scope,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let title = match normalize_optional(params.title, MAX_TITLE_CHARS, "title") {
            Ok(value) => value,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let model = match normalize_optional(params.model, MAX_MODEL_CHARS, "model") {
            Ok(value) => value,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let mode_id = match normalize_optional(params.mode_id, MAX_MODE_CHARS, "mode_id") {
            Ok(value) => value,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let initial_prompt =
            match normalize_optional(params.initial_prompt, MAX_PROMPT_CHARS, "initial_prompt") {
                Ok(value) => value,
                Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
            };
        let config_values = match normalize_config_values(params.config_values) {
            Ok(values) => values,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };

        let conversation_id = match create_conversation_core_with_source(
            &self.db.conn,
            caller_session.folder_id,
            params.harness,
            title.clone(),
            CREATED_BY_AGENT,
        )
        .await
        {
            Ok(id) => id,
            Err(error) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    format!("Could not allocate the persistent Session row: {error}"),
                )
            }
        };

        let runtime = self
            .runtime
            .create_session(CreateSessionSpec {
                conversation_id,
                folder_id: caller_session.folder_id,
                cwd: cwd.clone(),
                harness: params.harness,
                model: model.clone(),
                mode_id: mode_id.clone(),
                config_values: config_values.clone(),
                initial_prompt,
                request_id: request_id.clone(),
            })
            .await;
        let runtime = match runtime {
            Ok(runtime) => runtime,
            Err(error) => {
                return self
                    .creation_failed_outcome(request_id, action, conversation_id, error)
                    .await;
            }
        };

        if !runtime.identity_persisted {
            let Some(native_session_id) = runtime.native_session_id.as_ref() else {
                let _ = conversation_service::update_status(
                    &self.db.conn,
                    conversation_id,
                    ConversationStatus::Cancelled,
                )
                .await;
                emit_conversation_upsert(&self.emitter, &self.db.conn, conversation_id).await;
                return HostControlUseOutcome {
                    accepted: false,
                    request_id,
                    action,
                    stage: "creation_recovery_required".to_string(),
                    replayed: false,
                    data: json!({
                        "session_id": conversation_id,
                        "native_session_id": null,
                        "connection_id": runtime.connection_id,
                        "runtime_active": false,
                        "session_preserved": true,
                        "opened": false,
                        "focused": false,
                    }),
                    note: Some(format!(
                        "The Harness may have created a native Session, but Codeg could not confirm its resume identity. The provisional row was retained explicitly for recovery: {}",
                        runtime.setup_error.as_deref().unwrap_or("native Session creation outcome is unknown")
                    )),
                };
            };
            if let Err(error) = conversation_service::update_external_id(
                &self.db.conn,
                conversation_id,
                native_session_id.clone(),
            )
            .await
            {
                let _ = conversation_service::update_status(
                    &self.db.conn,
                    conversation_id,
                    ConversationStatus::Cancelled,
                )
                .await;
                emit_conversation_upsert(&self.emitter, &self.db.conn, conversation_id).await;
                return HostControlUseOutcome {
                    accepted: false,
                    request_id,
                    action,
                    stage: "creation_recovery_required".to_string(),
                    replayed: false,
                    data: json!({
                        "session_id": conversation_id,
                        "native_session_id": runtime.native_session_id,
                        "connection_id": runtime.connection_id,
                        "runtime_active": false,
                        "session_preserved": true,
                        "opened": false,
                        "focused": false,
                    }),
                    note: Some(format!(
                        "The Harness created a native Session, but Codeg could not persist its resume identity ({error}). The provisional row was retained explicitly for recovery."
                    )),
                };
            }
        }
        if runtime.prompt == InitialPromptStage::NotRequested {
            if let Err(error) = conversation_service::update_status(
                &self.db.conn,
                conversation_id,
                ConversationStatus::PendingReview,
            )
            .await
            {
                tracing::error!(
                    conversation_id,
                    "Could not mark promptless Host-created Session idle: {error}"
                );
            }
        }
        emit_conversation_upsert(&self.emitter, &self.db.conn, conversation_id).await;

        let (stage, prompt_status, note) = if let Some(error) = runtime.setup_error.as_ref() {
            (
                "created_setup_failed",
                "not_started",
                format!(
                    "Created persistent Session {conversation_id}, but stopped its runtime because requested setup could not be confirmed: {error}"
                ),
            )
        } else {
            match &runtime.prompt {
                InitialPromptStage::NotRequested => (
                    "created",
                    "not_requested",
                    format!("Created persistent Session {conversation_id} without opening or focusing UI."),
                ),
                InitialPromptStage::TurnStarted => (
                    "turn_started",
                    "started",
                    format!("Created persistent Session {conversation_id} and started its initial Turn."),
                ),
                InitialPromptStage::DispatchUnknown => (
                    "prompt_dispatch_unknown",
                    "dispatch_unknown",
                    format!("Created persistent Session {conversation_id}; the initial prompt may have reached the Harness, so Codeg will not replay it automatically."),
                ),
                InitialPromptStage::Failed(error) => (
                    "created_prompt_failed",
                    "failed",
                    format!("Created persistent Session {conversation_id}, but its initial prompt was not accepted: {error}"),
                ),
            }
        };
        HostControlUseOutcome {
            accepted: true,
            request_id,
            action,
            stage: stage.to_string(),
            replayed: false,
            data: json!({
                "session_id": conversation_id,
                "native_session_id": runtime.native_session_id,
                "connection_id": runtime.connection_id,
                "folder_id": caller_session.folder_id,
                "cwd": cwd,
                "harness": params.harness,
                "model": runtime.actual_model.or(model),
                "mode_id": mode_id,
                "config_values": config_values,
                "prompt_status": prompt_status,
                "runtime_active": runtime.runtime_active,
                "setup_error": runtime.setup_error,
                "opened": false,
                "focused": false,
                "requested_collection_id": params.collection_id,
            }),
            note: Some(note),
        }
    }

    async fn cancel_turn(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let target = match self.target_from_input(caller, &action, input).await {
            Ok(target) => target,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        match self.runtime.cancel_turn(target.id).await {
            Ok(CancelTurnRuntimeResult::NoActiveRuntime) => success_outcome(
                request_id,
                action,
                "no_active_runtime",
                json!({ "session_id": target.id, "cancelled": false }),
                "The persistent Session has no active managed runtime; no Turn was cancelled.",
            ),
            Ok(CancelTurnRuntimeResult::NoActiveTurn { connection_id }) => success_outcome(
                request_id,
                action,
                "no_active_turn",
                json!({
                    "session_id": target.id,
                    "connection_id": connection_id,
                    "cancelled": false,
                }),
                "The Session runtime is idle; no Turn was cancelled.",
            ),
            Ok(CancelTurnRuntimeResult::CancelRequested { connection_id }) => success_outcome(
                request_id,
                action,
                "cancel_requested",
                json!({
                    "session_id": target.id,
                    "connection_id": connection_id,
                    "cancelled": true,
                    "runtime_preserved": true,
                    "pending_prompts_paused": true,
                }),
                "Cancellation was requested for the current Turn; the persistent Session and runtime were preserved.",
            ),
            Err(error) => HostControlUseOutcome::rejected(
                request_id,
                action,
                format!("Could not cancel the current Turn: {error}"),
            ),
        }
    }

    async fn stop(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let target = match self.target_from_input(caller, &action, input).await {
            Ok(target) => target,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        match self.runtime.stop_session(target.id).await {
            Ok(StopSessionRuntimeResult::AlreadyStopped) => success_outcome(
                request_id,
                action,
                "already_stopped",
                json!({
                    "session_id": target.id,
                    "runtime_stopped": true,
                    "session_preserved": true,
                    "pending_prompts_paused": true,
                }),
                "The Session had no active managed runtime. Its persistent identity and history were preserved.",
            ),
            Ok(StopSessionRuntimeResult::StopRequested {
                connection_id,
                cancelled_turn,
            }) => success_outcome(
                request_id,
                action,
                "runtime_stop_requested",
                json!({
                    "session_id": target.id,
                    "connection_id": connection_id,
                    "runtime_stop_requested": true,
                    "cancelled_turn": cancelled_turn,
                    "session_preserved": true,
                    "pending_prompts_paused": true,
                }),
                "The managed runtime was asked to stop. The persistent Session and native transcript were preserved for resume.",
            ),
            Err(error) => HostControlUseOutcome::rejected(
                request_id,
                action,
                format!("Could not stop the Session runtime: {error}"),
            ),
        }
    }

    async fn target_from_input(
        &self,
        caller: &HostControlCaller,
        action: &str,
        input: Value,
    ) -> Result<DbConversationSummary, String> {
        let params: TargetSessionInput = parse_input(action, input)?;
        if params.session_id <= 0 {
            return Err("session_id must be a positive integer".to_string());
        }
        self.manageable_target(caller, params.session_id).await
    }

    async fn caller_scope(
        &self,
        caller: &HostControlCaller,
    ) -> Result<DbConversationSummary, String> {
        let session = conversation_service::get_by_id(&self.db.conn, caller.current_session_id)
            .await
            .map_err(|_| {
                "The calling Session is no longer an active Codeg Session. Resume it before using Host Control."
                    .to_string()
            })?;
        if !matches!(
            session.kind,
            ConversationKind::Regular | ConversationKind::Chat
        ) || session.parent_id.is_some()
        {
            return Err(
                "Host Control is available only to ordinary persistent Codeg Sessions.".to_string(),
            );
        }
        Ok(session)
    }

    async fn manageable_target(
        &self,
        caller: &HostControlCaller,
        target_session_id: i32,
    ) -> Result<DbConversationSummary, String> {
        let source = self.caller_scope(caller).await?;
        let target = conversation_service::get_by_id(&self.db.conn, target_session_id)
            .await
            .map_err(|_| {
                format!(
                    "Session {target_session_id} was not found in the calling Session's current project scope."
                )
            })?;
        if target.folder_id != source.folder_id
            || !matches!(
                target.kind,
                ConversationKind::Regular | ConversationKind::Chat
            )
            || target.parent_id.is_some()
        {
            return Err(format!(
                "Session {target_session_id} is outside the calling Session's current project scope."
            ));
        }
        Ok(target)
    }

    async fn resolve_create_scope(
        &self,
        caller: &HostControlCaller,
        folder_id: Option<i32>,
        cwd: Option<String>,
    ) -> Result<(DbConversationSummary, String), String> {
        let source = self.caller_scope(caller).await?;
        let folder_id = folder_id.unwrap_or(source.folder_id);
        if folder_id != source.folder_id {
            return Err(
                "folder_id must remain inside the calling Session's current project scope."
                    .to_string(),
            );
        }
        let folder = folder_service::get_folder_by_id(&self.db.conn, folder_id)
            .await
            .map_err(|error| format!("Could not read folder {folder_id}: {error}"))?
            .ok_or_else(|| format!("Folder {folder_id} is no longer available."))?;
        let caller_cwd = canonical_existing_directory(&caller.working_dir, "calling Session cwd")?;
        let requested = match cwd {
            Some(value) if !value.trim().is_empty() => PathBuf::from(value.trim()),
            _ => caller.working_dir.clone(),
        };
        let requested_cwd = canonical_existing_directory(&requested, "cwd")?;
        if !paths_match(&caller_cwd, &requested_cwd) {
            return Err(
                "cwd must match the token-bound calling Session working directory.".to_string(),
            );
        }
        let folder_runtime_path = source.origin_cwd.as_deref().unwrap_or(folder.path.as_str());
        let folder_cwd = canonical_existing_directory(
            Path::new(folder_runtime_path),
            "folder working directory",
        )?;
        if !paths_match(&folder_cwd, &requested_cwd) {
            return Err(
                "The calling Session cwd no longer matches its registered folder scope."
                    .to_string(),
            );
        }
        Ok((source, requested_cwd.to_string_lossy().into_owned()))
    }

    async fn creation_failed_outcome(
        &self,
        request_id: String,
        action: String,
        conversation_id: i32,
        error: String,
    ) -> HostControlUseOutcome {
        match conversation_service::soft_delete(&self.db.conn, conversation_id).await {
            Ok(()) => {
                emit_conversation_deleted(&self.emitter, conversation_id);
                HostControlUseOutcome {
                    accepted: false,
                    request_id,
                    action,
                    stage: "creation_failed_cleaned".to_string(),
                    replayed: false,
                    data: json!({
                        "session_id": conversation_id,
                        "cleaned": true,
                    }),
                    note: Some(format!(
                        "Session creation failed and its provisional Codeg row was removed: {error}"
                    )),
                }
            }
            Err(cleanup_error) => {
                let _ = conversation_service::update_status(
                    &self.db.conn,
                    conversation_id,
                    ConversationStatus::Cancelled,
                )
                .await;
                emit_conversation_upsert(&self.emitter, &self.db.conn, conversation_id).await;
                HostControlUseOutcome {
                    accepted: false,
                    request_id,
                    action,
                    stage: "creation_failed_cleanup_required".to_string(),
                    replayed: false,
                    data: json!({
                        "session_id": conversation_id,
                        "cleaned": false,
                    }),
                    note: Some(format!(
                        "Session creation failed ({error}) and automatic cleanup also failed ({cleanup_error}). The provisional row was retained explicitly for recovery."
                    )),
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionCreateInput {
    harness: AgentType,
    #[serde(default)]
    folder_id: Option<i32>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    mode_id: Option<String>,
    #[serde(default)]
    config_values: BTreeMap<String, String>,
    #[serde(default)]
    initial_prompt: Option<String>,
    #[serde(default)]
    collection_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetSessionInput {
    session_id: i32,
}

fn target_session_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["session_id"],
        "properties": {
            "session_id": {
                "type": "integer",
                "minimum": 1,
                "maximum": i32::MAX,
                "description": "Target persistent Codeg Session id in the caller's current project scope."
            }
        }
    })
}

fn parse_input<T: for<'de> Deserialize<'de>>(action: &str, input: Value) -> Result<T, String> {
    serde_json::from_value(input).map_err(|error| format!("Invalid input for {action}: {error}"))
}

fn normalize_optional(
    value: Option<String>,
    max_chars: usize,
    label: &str,
) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > max_chars {
        return Err(format!("{label} must be at most {max_chars} characters"));
    }
    Ok(Some(value.to_string()))
}

fn normalize_config_values(
    values: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    if values.len() > MAX_CONFIG_ENTRIES {
        return Err(format!(
            "config_values must contain at most {MAX_CONFIG_ENTRIES} entries"
        ));
    }
    let mut normalized = BTreeMap::new();
    for (key, value) in values {
        let key = key.trim();
        let value = value.trim();
        if key.is_empty()
            || key.chars().count() > MAX_CONFIG_KEY_CHARS
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(format!(
                "config_values keys must contain 1 to {MAX_CONFIG_KEY_CHARS} safe identifier characters"
            ));
        }
        if key == PREFERRED_MODEL_CONFIG_KEY {
            return Err("config_values contains a reserved Host key".to_string());
        }
        if value.is_empty() || value.chars().count() > MAX_CONFIG_VALUE_CHARS {
            return Err(format!(
                "config_values[{key:?}] must contain 1 to {MAX_CONFIG_VALUE_CHARS} characters"
            ));
        }
        normalized.insert(key.to_string(), value.to_string());
    }
    Ok(normalized)
}

fn canonical_existing_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("{label} does not exist or is not accessible: {error}"))?;
    if !metadata.is_dir() {
        return Err(format!("{label} must be an existing directory"));
    }
    std::fs::canonicalize(path).map_err(|error| format!("Could not resolve {label}: {error}"))
}

fn paths_match(left: &Path, right: &Path) -> bool {
    crate::parsers::path_eq_for_matching(&left.to_string_lossy(), &right.to_string_lossy())
}

fn current_option_value(option: &SessionConfigOptionInfo) -> String {
    match &option.kind {
        SessionConfigKindInfo::Select(select) => select.current_value.clone(),
        SessionConfigKindInfo::Boolean(boolean) => boolean.current_value.to_string(),
    }
}

fn current_model(options: &AgentOptionsSnapshot) -> Option<String> {
    options
        .config_options
        .iter()
        .find(|option| option.category.as_deref() == Some("model"))
        .map(current_option_value)
        .filter(|value| !value.is_empty())
}

fn verify_requested_options(
    options: &AgentOptionsSnapshot,
    model: Option<&str>,
    mode_id: Option<&str>,
    config_values: &BTreeMap<String, String>,
) -> Result<(), String> {
    if let Some(model) = model {
        let actual = current_model(options).ok_or_else(|| {
            "The Harness did not advertise a verifiable model selector; the requested model was not applied."
                .to_string()
        })?;
        if actual != model {
            return Err(format!(
                "The Harness did not apply requested model {model:?}; its active model is {actual:?}."
            ));
        }
    }
    if let Some(mode_id) = mode_id {
        let actual = options
            .modes
            .as_ref()
            .map(|modes| modes.current_mode_id.as_str())
            .ok_or_else(|| {
                "The Harness did not advertise a verifiable Session mode; mode_id was not applied."
                    .to_string()
            })?;
        if actual != mode_id {
            return Err(format!(
                "The Harness did not apply requested mode {mode_id:?}; its active mode is {actual:?}."
            ));
        }
    }
    for (config_id, expected) in config_values {
        let option = options
            .config_options
            .iter()
            .find(|option| option.id == *config_id)
            .ok_or_else(|| {
                format!("The Harness did not advertise requested config option {config_id:?}.")
            })?;
        let actual = current_option_value(option);
        if actual != *expected {
            return Err(format!(
                "The Harness did not apply config {config_id:?}={expected:?}; its active value is {actual:?}."
            ));
        }
    }
    Ok(())
}

#[cfg(any(test, feature = "test-utils"))]
struct IsolatedHostSessionRuntime;

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl HostSessionRuntime for IsolatedHostSessionRuntime {
    async fn create_session(&self, spec: CreateSessionSpec) -> Result<CreatedRuntime, String> {
        Ok(CreatedRuntime {
            connection_id: format!("isolated-{}", spec.conversation_id),
            native_session_id: Some(format!("native-{}", spec.conversation_id)),
            actual_model: spec.model.clone(),
            prompt: InitialPromptStage::NotRequested,
            runtime_active: true,
            identity_persisted: false,
            setup_error: None,
        })
    }

    async fn cancel_turn(&self, _conversation_id: i32) -> Result<CancelTurnRuntimeResult, String> {
        Ok(CancelTurnRuntimeResult::NoActiveRuntime)
    }

    async fn stop_session(
        &self,
        _conversation_id: i32,
    ) -> Result<StopSessionRuntimeResult, String> {
        Ok(StopSessionRuntimeResult::AlreadyStopped)
    }
}

fn write_fingerprint(action: &str, input: &Value) -> String {
    let encoded = serde_json::to_vec(&(action, input)).unwrap_or_default();
    format!("{:x}", Sha256::digest(encoded))
}

fn success_outcome(
    request_id: String,
    action: String,
    stage: &str,
    data: Value,
    note: &str,
) -> HostControlUseOutcome {
    HostControlUseOutcome {
        accepted: true,
        request_id,
        action,
        stage: stage.to_string(),
        replayed: false,
        data,
        note: Some(note.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};

    #[derive(Default)]
    struct FakeRuntime {
        create_calls: Mutex<Vec<CreateSessionSpec>>,
        create_error: Mutex<Option<String>>,
        setup_error: Mutex<Option<String>>,
        native_identity_unknown: Mutex<bool>,
        cancel_result: Mutex<Option<CancelTurnRuntimeResult>>,
        stop_result: Mutex<Option<StopSessionRuntimeResult>>,
    }

    #[async_trait]
    impl HostSessionRuntime for FakeRuntime {
        async fn create_session(&self, spec: CreateSessionSpec) -> Result<CreatedRuntime, String> {
            self.create_calls.lock().await.push(spec.clone());
            if let Some(error) = self.create_error.lock().await.clone() {
                return Err(error);
            }
            let setup_error = self.setup_error.lock().await.clone();
            let native_session_id = if *self.native_identity_unknown.lock().await {
                None
            } else {
                Some("native-created".to_string())
            };
            Ok(CreatedRuntime {
                connection_id: "connection-created".to_string(),
                native_session_id,
                actual_model: spec.model.clone(),
                prompt: if spec.initial_prompt.is_some() {
                    InitialPromptStage::TurnStarted
                } else {
                    InitialPromptStage::NotRequested
                },
                runtime_active: setup_error.is_none(),
                identity_persisted: false,
                setup_error,
            })
        }

        async fn cancel_turn(
            &self,
            _conversation_id: i32,
        ) -> Result<CancelTurnRuntimeResult, String> {
            Ok(self
                .cancel_result
                .lock()
                .await
                .clone()
                .unwrap_or(CancelTurnRuntimeResult::NoActiveRuntime))
        }

        async fn stop_session(
            &self,
            _conversation_id: i32,
        ) -> Result<StopSessionRuntimeResult, String> {
            Ok(self
                .stop_result
                .lock()
                .await
                .clone()
                .unwrap_or(StopSessionRuntimeResult::AlreadyStopped))
        }
    }

    async fn fixture() -> (
        SessionHostControlProvider,
        Arc<FakeRuntime>,
        i32,
        i32,
        PathBuf,
    ) {
        let db = Arc::new(fresh_in_memory_db().await);
        let cwd = std::env::current_dir().unwrap();
        let folder = seed_folder(&db, &cwd.to_string_lossy()).await;
        let caller = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let runtime = Arc::new(FakeRuntime::default());
        let provider =
            SessionHostControlProvider::new_with_runtime(db, EventEmitter::Noop, runtime.clone());
        (provider, runtime, caller, target, cwd)
    }

    fn caller(id: i32, cwd: PathBuf) -> HostControlCaller {
        HostControlCaller {
            current_session_id: id,
            working_dir: cwd,
            writes_allowed: true,
        }
    }

    #[tokio::test]
    async fn create_returns_stable_identity_does_not_focus_and_replays_once() {
        let (provider, runtime, caller_id, _, cwd) = fixture().await;
        let input = json!({
            "harness": "claude_code",
            "model": "claude-sonnet",
            "initial_prompt": "Review the current change"
        });
        let first = provider
            .dispatch(
                &caller(caller_id, cwd.clone()),
                "create-once".into(),
                "session.create".into(),
                input.clone(),
            )
            .await;
        assert!(first.accepted);
        assert_eq!(first.stage, "turn_started");
        assert_eq!(first.data["native_session_id"], "native-created");
        assert_eq!(first.data["focused"], false);
        assert_eq!(first.data["opened"], false);
        let session_id = first.data["session_id"].as_i64().unwrap() as i32;
        let stored = conversation_service::get_by_id(&provider.db.conn, session_id)
            .await
            .unwrap();
        assert_eq!(stored.external_id.as_deref(), Some("native-created"));

        let replay = provider
            .dispatch(
                &caller(caller_id, cwd),
                "create-once".into(),
                "session.create".into(),
                input,
            )
            .await;
        assert!(replay.accepted);
        assert!(replay.replayed);
        assert_eq!(replay.data["session_id"], session_id);
        assert_eq!(runtime.create_calls.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn failed_pre_native_create_soft_deletes_the_provisional_row() {
        let (provider, runtime, caller_id, _, cwd) = fixture().await;
        *runtime.create_error.lock().await = Some("handshake failed".to_string());
        let outcome = provider
            .dispatch(
                &caller(caller_id, cwd),
                "create-fails".into(),
                "session.create".into(),
                json!({ "harness": "codex" }),
            )
            .await;
        assert!(!outcome.accepted);
        assert_eq!(outcome.stage, "creation_failed_cleaned");
        let session_id = outcome.data["session_id"].as_i64().unwrap() as i32;
        assert!(
            conversation_service::get_by_id(&provider.db.conn, session_id)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn post_native_setup_failure_preserves_an_explicit_resumable_session() {
        let (provider, runtime, caller_id, _, cwd) = fixture().await;
        *runtime.setup_error.lock().await = Some("model was rejected".to_string());
        let outcome = provider
            .dispatch(
                &caller(caller_id, cwd),
                "setup-failed".into(),
                "session.create".into(),
                json!({ "harness": "codex", "model": "missing" }),
            )
            .await;

        assert!(outcome.accepted);
        assert_eq!(outcome.stage, "created_setup_failed");
        assert_eq!(outcome.data["runtime_active"], false);
        let session_id = outcome.data["session_id"].as_i64().unwrap() as i32;
        let session = conversation_service::get_by_id(&provider.db.conn, session_id)
            .await
            .expect("post-native failures must retain the Codeg row");
        assert_eq!(session.external_id.as_deref(), Some("native-created"));
    }

    #[tokio::test]
    async fn ambiguous_native_handshake_preserves_recovery_row_instead_of_ghosting() {
        let (provider, runtime, caller_id, _, cwd) = fixture().await;
        *runtime.native_identity_unknown.lock().await = true;
        *runtime.setup_error.lock().await = Some("handshake timed out".to_string());
        let outcome = provider
            .dispatch(
                &caller(caller_id, cwd),
                "ambiguous-create".into(),
                "session.create".into(),
                json!({ "harness": "codex" }),
            )
            .await;

        assert!(!outcome.accepted);
        assert_eq!(outcome.stage, "creation_recovery_required");
        assert!(outcome.data["native_session_id"].is_null());
        let session_id = outcome.data["session_id"].as_i64().unwrap() as i32;
        let session = conversation_service::get_by_id(&provider.db.conn, session_id)
            .await
            .expect("ambiguous native creation must retain a visible recovery row");
        assert_eq!(session.status, "cancelled");
    }

    #[tokio::test]
    async fn create_rejects_other_cwd_before_allocating_or_spawning() {
        let (provider, runtime, caller_id, _, cwd) = fixture().await;
        let other = tempfile::tempdir().unwrap();
        let outcome = provider
            .dispatch(
                &caller(caller_id, cwd),
                "wrong-cwd".into(),
                "session.create".into(),
                json!({
                    "harness": "codex",
                    "cwd": other.path().to_string_lossy(),
                }),
            )
            .await;
        assert!(!outcome.accepted);
        assert!(outcome.note.unwrap().contains("token-bound"));
        assert!(runtime.create_calls.lock().await.is_empty());
    }

    #[tokio::test]
    async fn cancel_and_stop_report_precise_runtime_boundaries_and_replay() {
        let (provider, runtime, caller_id, target_id, cwd) = fixture().await;
        *runtime.cancel_result.lock().await = Some(CancelTurnRuntimeResult::CancelRequested {
            connection_id: "live".into(),
        });
        let cancel = provider
            .dispatch(
                &caller(caller_id, cwd.clone()),
                "cancel-once".into(),
                "session.cancel_turn".into(),
                json!({ "session_id": target_id }),
            )
            .await;
        assert!(cancel.accepted);
        assert_eq!(cancel.stage, "cancel_requested");
        assert_eq!(cancel.data["runtime_preserved"], true);

        *runtime.stop_result.lock().await = Some(StopSessionRuntimeResult::StopRequested {
            connection_id: "live".into(),
            cancelled_turn: true,
        });
        let stop = provider
            .dispatch(
                &caller(caller_id, cwd.clone()),
                "stop-once".into(),
                "session.stop".into(),
                json!({ "session_id": target_id }),
            )
            .await;
        assert!(stop.accepted);
        assert_eq!(stop.stage, "runtime_stop_requested");
        assert_eq!(stop.data["session_preserved"], true);

        let replay = provider
            .dispatch(
                &caller(caller_id, cwd),
                "stop-once".into(),
                "session.stop".into(),
                json!({ "session_id": target_id }),
            )
            .await;
        assert!(replay.replayed);
        assert_eq!(replay.stage, "runtime_stop_requested");
    }

    #[test]
    fn lifecycle_catalog_never_accepts_model_owned_caller_identity() {
        let encoded = serde_json::to_string(&SessionHostControlProvider::capabilities()).unwrap();
        assert!(encoded.contains("session.create"));
        assert!(encoded.contains("session.cancel_turn"));
        assert!(encoded.contains("session.stop"));
        for forbidden in ["from_session_id", "source_session_id", "current_session_id"] {
            assert!(!encoded.contains(forbidden));
        }
    }
}
