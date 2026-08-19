//! Model and thinking-effort selector capabilities for the progressive Host
//! Control gateway.
//!
//! Selectors are Harness-advertised config options: the model is the option
//! with ACP category `model` (or the conventional `model` id), thinking effort
//! is category `thought_level` (pi-acp's `thought_level` id, Grok's
//! synthesized `reasoning_effort`). A Host write pins the semantic
//! `__codeg_host_*__` key in the Session's `preferred_config_values`, which
//! every later (re)connect resolves against whatever that Harness advertises;
//! when a runtime is live the switch is also requested immediately, exactly
//! like the composer's `acp_set_config_option` path. Effort vocabularies are
//! per-Harness (low/medium/high, off/minimal/.../xhigh, …), so values are
//! validated against the live advertised choices whenever a runtime exists
//! and pinned unchecked otherwise.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::acp::connection::{PREFERRED_MODEL_CONFIG_KEY, PREFERRED_THOUGHT_LEVEL_CONFIG_KEY};
use crate::acp::host_control::{
    HostControlAccessLevel, HostControlCaller, HostControlCapability, HostControlUseOutcome,
};
use crate::acp::manager::ConnectionManager;
use crate::acp::types::{SessionConfigKindInfo, SessionConfigOptionInfo};
use crate::db::entities::conversation::ConversationKind;
use crate::db::service::conversation_service;
use crate::db::AppDatabase;
use crate::models::DbConversationSummary;

const MAX_MODEL_VALUE_CHARS: usize = 256;
const MAX_EFFORT_VALUE_CHARS: usize = 128;
const MAX_IDEMPOTENCY_ENTRIES: usize = 512;
/// How many advertised values a rejection note lists before truncating.
const MAX_PREVIEW_VALUES: usize = 10;

/// Info-form category strings produced by `map_session_config_category` (and
/// hardcoded by Grok's synthesized selectors). The model match mirrors the
/// composer's `isModelConfigOption`.
const MODEL_CATEGORY: &str = "model";
const MODEL_OPTION_ID: &str = "model";
const THOUGHT_LEVEL_CATEGORY: &str = "thought_level";
const GROK_EFFORT_OPTION_ID: &str = "reasoning_effort";

#[derive(Clone)]
struct CachedWrite {
    fingerprint: String,
    outcome: HostControlUseOutcome,
}

#[derive(Default)]
struct IdempotencyCache {
    entries: HashMap<String, CachedWrite>,
    order: VecDeque<String>,
}

impl IdempotencyCache {
    fn insert(&mut self, request_id: String, value: CachedWrite) {
        if !self.entries.contains_key(&request_id) {
            self.order.push_back(request_id.clone());
        }
        self.entries.insert(request_id, value);
        while self.entries.len() > MAX_IDEMPOTENCY_ENTRIES {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

/// Which selector one field of `session.set_selectors` / `session.get_selectors`
/// addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorSlot {
    Model,
    ThinkingEffort,
}

impl SelectorSlot {
    fn semantic_key(self) -> &'static str {
        match self {
            SelectorSlot::Model => PREFERRED_MODEL_CONFIG_KEY,
            SelectorSlot::ThinkingEffort => PREFERRED_THOUGHT_LEVEL_CONFIG_KEY,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SelectorSlot::Model => "model",
            SelectorSlot::ThinkingEffort => "thinking_effort",
        }
    }
}

/// What a validated set can do about the live runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LivePlan {
    /// No live runtime: the pin applies (and is validated) at the next connect.
    NoRuntime,
    /// The live runtime does not advertise this selector: nothing to switch
    /// now, the pin still applies at the next connect.
    NotAdvertised,
    /// The live runtime advertises the selector and offers the value: request
    /// the switch immediately.
    Apply { config_id: String },
}

/// The live-runtime surface the selector actions need, isolated so tests can
/// drive the resolution/validation logic without a Harness process.
#[async_trait]
trait HostSelectorRuntime: Send + Sync {
    /// The selector options the Session's live runtime currently advertises,
    /// or `None` when no runtime is running for it.
    async fn advertised_options(
        &self,
        conversation_id: i32,
    ) -> Option<Vec<SessionConfigOptionInfo>>;
    /// Ask the live runtime to switch one selector, fire-and-forget like the
    /// composer's `acp_set_config_option` path.
    async fn apply_config_option(
        &self,
        conversation_id: i32,
        config_id: String,
        value_id: String,
    ) -> Result<(), String>;
}

struct ManagedSelectorRuntime {
    manager: ConnectionManager,
}

#[async_trait]
impl HostSelectorRuntime for ManagedSelectorRuntime {
    async fn advertised_options(
        &self,
        conversation_id: i32,
    ) -> Option<Vec<SessionConfigOptionInfo>> {
        let connection_id = self
            .manager
            .find_connection_by_conversation_id(conversation_id)
            .await?;
        let state = self.manager.get_state(&connection_id).await?;
        state.read().await.config_options.clone()
    }

    async fn apply_config_option(
        &self,
        conversation_id: i32,
        config_id: String,
        value_id: String,
    ) -> Result<(), String> {
        let connection_id = self
            .manager
            .find_connection_by_conversation_id(conversation_id)
            .await
            .ok_or_else(|| "The Session has no active managed runtime.".to_string())?;
        self.manager
            .set_config_option(&connection_id, config_id, value_id)
            .await
            .map_err(|error| error.to_string())
    }
}

pub struct SelectorHostControl {
    db: Arc<AppDatabase>,
    runtime: Arc<dyn HostSelectorRuntime>,
    /// Held across a write so concurrent replays cannot both pass the lookup.
    writes: Mutex<IdempotencyCache>,
}

impl SelectorHostControl {
    pub fn new(db: Arc<AppDatabase>, manager: ConnectionManager) -> Self {
        Self {
            db,
            runtime: Arc::new(ManagedSelectorRuntime { manager }),
            writes: Mutex::new(IdempotencyCache::default()),
        }
    }

    #[cfg(any(test, feature = "test-utils"))]
    fn new_with_runtime(db: Arc<AppDatabase>, runtime: Arc<dyn HostSelectorRuntime>) -> Self {
        Self {
            db,
            runtime,
            writes: Mutex::new(IdempotencyCache::default()),
        }
    }

    /// Public-path tests still go through Host Core; only the live-runtime
    /// lookup is isolated so the suite never needs a Harness process. The
    /// isolated runtime behaves like "no runtime is running".
    #[cfg(any(test, feature = "test-utils"))]
    pub fn isolated_for_tests(db: Arc<AppDatabase>) -> Self {
        Self::new_with_runtime(db, Arc::new(IsolatedSelectorRuntime))
    }

    pub fn capabilities(writes_allowed: bool) -> Vec<HostControlCapability> {
        let mut capabilities = vec![capability(
            "session.get_selectors",
            "Read one Session's model and thinking-effort selectors in the caller's current project scope: the Host-pinned values that apply at every runtime (re)start and, while a runtime is live, the Harness-advertised current value and available choices for each selector.",
            HostControlAccessLevel::Read,
            target_session_schema(),
            &["read"],
        )];
        if writes_allowed {
            capabilities.push(capability(
                "session.set_selectors",
                "Pin one Session's model and/or thinking effort (at least one is required). The pin persists and applies at every later runtime (re)start, winning over the user's saved per-agent defaults; when a runtime is live and the Harness advertises a matching selector, the switch is also requested immediately and takes effect on subsequent Turns. Thinking-effort vocabularies differ per Harness: call session.get_selectors first and pick from the advertised choices. Without a live runtime the value is pinned unchecked and the Harness validates it at the next (re)start.",
                HostControlAccessLevel::Write,
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["session_id"],
                    "properties": {
                        "session_id": session_id_schema(),
                        "model": {
                            "type": "string",
                            "maxLength": MAX_MODEL_VALUE_CHARS,
                            "description": "Model value as the Harness advertises it (see session.get_selectors)."
                        },
                        "thinking_effort": {
                            "type": "string",
                            "maxLength": MAX_EFFORT_VALUE_CHARS,
                            "description": "Thinking-effort value as the Harness advertises it; vocabularies differ per Harness (low/medium/high, off/minimal/.../xhigh, ...)."
                        }
                    }
                }),
                &["persisted"],
            ));
        }
        capabilities
    }

    pub fn access_for(action: &str) -> Option<HostControlAccessLevel> {
        match action {
            "session.get_selectors" => Some(HostControlAccessLevel::Read),
            "session.set_selectors" => Some(HostControlAccessLevel::Write),
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
            "session.get_selectors" => self.get_selectors(caller, request_id, action, input).await,
            "session.set_selectors" => self.set_selectors(caller, request_id, action, input).await,
            _ => HostControlUseOutcome::rejected(
                request_id,
                action,
                "Unknown selector Host Control action. Call codeg_help to refresh the catalog.",
            ),
        }
    }

    async fn get_selectors(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: GetSelectorsInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        if params.session_id <= 0 {
            return HostControlUseOutcome::rejected(
                request_id,
                action,
                "session_id must be a positive integer",
            );
        }
        let target = match self.manageable_target(caller, params.session_id).await {
            Ok(target) => target,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let (_, pins) = match conversation_service::selector_prefs(&self.db.conn, target.id).await {
            Ok(prefs) => prefs,
            Err(error) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    format!("Could not read Session selector pins: {error}"),
                )
            }
        };
        let advertised = self.runtime.advertised_options(target.id).await;
        HostControlUseOutcome {
            accepted: true,
            request_id,
            action,
            stage: "read".to_string(),
            replayed: false,
            data: json!({
                "session_id": target.id,
                "agent_type": target.agent_type,
                "runtime_active": advertised.is_some(),
                "model": selector_view(&pins, advertised.as_deref(), SelectorSlot::Model),
                "thinking_effort": selector_view(
                    &pins,
                    advertised.as_deref(),
                    SelectorSlot::ThinkingEffort,
                ),
                "pinned_config_values": pins,
            }),
            note: None,
        }
    }

    async fn set_selectors(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let fingerprint = write_fingerprint(&action, &input);
        let mut writes = self.writes.lock().await;
        if let Some(cached) = writes.entries.get(&request_id) {
            if cached.fingerprint != fingerprint {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    "request_id was already used with different action input",
                );
            }
            let mut outcome = cached.outcome.clone();
            outcome.replayed = true;
            return outcome;
        }

        let outcome = self
            .set_selectors_uncached(caller, request_id.clone(), action.clone(), input)
            .await;
        if outcome.accepted {
            writes.insert(
                request_id,
                CachedWrite {
                    fingerprint,
                    outcome: outcome.clone(),
                },
            );
        }
        outcome
    }

    async fn set_selectors_uncached(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: SetSelectorsInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        if params.session_id <= 0 {
            return HostControlUseOutcome::rejected(
                request_id,
                action,
                "session_id must be a positive integer",
            );
        }
        let model = match normalize_optional(params.model, MAX_MODEL_VALUE_CHARS, "model") {
            Ok(value) => value,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let thinking_effort = match normalize_optional(
            params.thinking_effort,
            MAX_EFFORT_VALUE_CHARS,
            "thinking_effort",
        ) {
            Ok(value) => value,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let requested: Vec<(SelectorSlot, String)> = [
            model.map(|value| (SelectorSlot::Model, value)),
            thinking_effort.map(|value| (SelectorSlot::ThinkingEffort, value)),
        ]
        .into_iter()
        .flatten()
        .collect();
        if requested.is_empty() {
            return HostControlUseOutcome::rejected(
                request_id,
                action,
                "at least one of model / thinking_effort is required",
            );
        }
        let target = match self.manageable_target(caller, params.session_id).await {
            Ok(target) => target,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };

        // Validate every requested field against the live runtime before
        // writing anything, so a bad effort value cannot leave the model pin
        // half-applied behind it.
        let advertised = self.runtime.advertised_options(target.id).await;
        let mut plans: Vec<(SelectorSlot, String, LivePlan)> = Vec::with_capacity(requested.len());
        for (slot, value) in requested {
            match plan_live_change(advertised.as_deref(), slot, &value) {
                Ok(plan) => plans.push((slot, value, plan)),
                Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
            }
        }

        let mut fields = serde_json::Map::new();
        let mut live_notes: Vec<String> = Vec::new();
        for (slot, value, plan) in plans {
            if let Err(error) = conversation_service::merge_selector_config_value(
                &self.db.conn,
                target.id,
                slot.semantic_key(),
                &value,
            )
            .await
            {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    format!("Could not persist the {} pin: {error}", slot.label()),
                );
            }
            let (applied_live, live_status, config_id, live_error) = match plan {
                LivePlan::NoRuntime => (false, "no_active_runtime", None, None),
                LivePlan::NotAdvertised => (false, "selector_not_advertised", None, None),
                LivePlan::Apply { config_id } => match self
                    .runtime
                    .apply_config_option(target.id, config_id.clone(), value.clone())
                    .await
                {
                    Ok(()) => (true, "applied", Some(config_id), None),
                    Err(error) => {
                        live_notes.push(format!(
                            "the live {} switch was requested but the runtime reported: {error}",
                            slot.label()
                        ));
                        (false, "apply_failed", Some(config_id), Some(error))
                    }
                },
            };
            fields.insert(
                slot.label().to_string(),
                json!({
                    "pinned": value,
                    "applied_live": applied_live,
                    "live_status": live_status,
                    "config_id": config_id,
                    "live_error": live_error,
                }),
            );
        }

        let mut note = format!(
            "Pinned selector values for Session {}; they apply at every later runtime (re)start.",
            target.id
        );
        if !live_notes.is_empty() {
            note.push_str(&format!(" However, {}", live_notes.join("; ")));
        }
        HostControlUseOutcome {
            accepted: true,
            request_id,
            action,
            stage: "persisted".to_string(),
            replayed: false,
            data: {
                let mut data = serde_json::Map::new();
                data.insert("session_id".to_string(), json!(target.id));
                data.extend(fields);
                Value::Object(data)
            },
            note: Some(note),
        }
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
}

/// The advertised option a slot resolves to, if the Harness published one.
fn find_advertised_option(
    options: &[SessionConfigOptionInfo],
    slot: SelectorSlot,
) -> Option<&SessionConfigOptionInfo> {
    options.iter().find(|option| match slot {
        SelectorSlot::Model => {
            option.category.as_deref() == Some(MODEL_CATEGORY) || option.id == MODEL_OPTION_ID
        }
        SelectorSlot::ThinkingEffort => {
            option.category.as_deref() == Some(THOUGHT_LEVEL_CATEGORY)
                || option.id == THOUGHT_LEVEL_CATEGORY
                || option.id == GROK_EFFORT_OPTION_ID
        }
    })
}

fn advertised_current_value(option: &SessionConfigOptionInfo) -> String {
    match &option.kind {
        SessionConfigKindInfo::Select(select) => select.current_value.clone(),
        SessionConfigKindInfo::Boolean(boolean) => boolean.current_value.to_string(),
    }
}

fn advertised_choices(option: &SessionConfigOptionInfo) -> Option<Vec<String>> {
    match &option.kind {
        SessionConfigKindInfo::Select(select) => Some(
            select
                .options
                .iter()
                .map(|choice| choice.value.clone())
                .collect(),
        ),
        SessionConfigKindInfo::Boolean(_) => None,
    }
}

/// Decide what a set can do about the live runtime, validating the value
/// against the advertised choices when the selector is live.
fn plan_live_change(
    advertised: Option<&[SessionConfigOptionInfo]>,
    slot: SelectorSlot,
    value: &str,
) -> Result<LivePlan, String> {
    let Some(options) = advertised else {
        return Ok(LivePlan::NoRuntime);
    };
    let Some(option) = find_advertised_option(options, slot) else {
        return Ok(LivePlan::NotAdvertised);
    };
    let Some(choices) = advertised_choices(option) else {
        return Err(format!(
            "the live Harness's {} selector is a toggle, not a valued choice",
            slot.label()
        ));
    };
    if choices.iter().any(|choice| choice == value) {
        return Ok(LivePlan::Apply {
            config_id: option.id.clone(),
        });
    }
    Err(format!(
        "{} must be one of the live Harness's advertised values: {}",
        slot.label(),
        choices_preview(&choices)
    ))
}

fn choices_preview(choices: &[String]) -> String {
    let shown: Vec<&str> = choices
        .iter()
        .take(MAX_PREVIEW_VALUES)
        .map(String::as_str)
        .collect();
    if choices.len() > MAX_PREVIEW_VALUES {
        format!("{}, … ({} total)", shown.join(", "), choices.len())
    } else {
        shown.join(", ")
    }
}

/// The `get_selectors` view of one selector: the Host pin, plus the live
/// Harness truth when a runtime is running.
fn selector_view(
    pins: &BTreeMap<String, String>,
    advertised: Option<&[SessionConfigOptionInfo]>,
    slot: SelectorSlot,
) -> Value {
    let option = advertised.and_then(|options| find_advertised_option(options, slot));
    json!({
        "pinned": pins.get(slot.semantic_key()),
        "supported": advertised.map(|_| option.is_some()),
        "current": option.map(advertised_current_value),
        "config_id": option.map(|option| option.id.clone()),
        "available": option.and_then(advertised_choices),
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GetSelectorsInput {
    session_id: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetSelectorsInput {
    session_id: i32,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    thinking_effort: Option<String>,
}

fn capability(
    action: &str,
    description: &str,
    access: HostControlAccessLevel,
    input_schema: Value,
    result_stages: &[&str],
) -> HostControlCapability {
    HostControlCapability {
        action: action.to_string(),
        description: description.to_string(),
        access,
        input_schema,
        result_stages: result_stages
            .iter()
            .map(|stage| stage.to_string())
            .collect(),
    }
}

fn session_id_schema() -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": i32::MAX,
        "description": "Target persistent Codeg Session id in the caller's current project scope."
    })
}

fn target_session_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["session_id"],
        "properties": {
            "session_id": session_id_schema()
        }
    })
}

fn parse_input<T: DeserializeOwned>(action: &str, input: Value) -> Result<T, String> {
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

fn write_fingerprint(action: &str, input: &Value) -> String {
    let encoded = serde_json::to_vec(&(action, input)).unwrap_or_default();
    format!("{:x}", Sha256::digest(encoded))
}

#[cfg(any(test, feature = "test-utils"))]
struct IsolatedSelectorRuntime;

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl HostSelectorRuntime for IsolatedSelectorRuntime {
    async fn advertised_options(
        &self,
        _conversation_id: i32,
    ) -> Option<Vec<SessionConfigOptionInfo>> {
        None
    }

    async fn apply_config_option(
        &self,
        _conversation_id: i32,
        _config_id: String,
        _value_id: String,
    ) -> Result<(), String> {
        Err("no live runtime in the isolated test environment".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::types::SessionConfigSelectInfo;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::agent::AgentType;
    use std::path::PathBuf;

    #[derive(Default)]
    struct FakeSelectorRuntime {
        advertised: Mutex<Option<Vec<SessionConfigOptionInfo>>>,
        applied: Mutex<Vec<(i32, String, String)>>,
        apply_error: Mutex<Option<String>>,
    }

    #[async_trait]
    impl HostSelectorRuntime for FakeSelectorRuntime {
        async fn advertised_options(
            &self,
            _conversation_id: i32,
        ) -> Option<Vec<SessionConfigOptionInfo>> {
            self.advertised.lock().await.clone()
        }

        async fn apply_config_option(
            &self,
            conversation_id: i32,
            config_id: String,
            value_id: String,
        ) -> Result<(), String> {
            self.applied
                .lock()
                .await
                .push((conversation_id, config_id, value_id));
            if let Some(error) = self.apply_error.lock().await.clone() {
                return Err(error);
            }
            Ok(())
        }
    }

    fn select_option(
        id: &str,
        category: Option<&str>,
        current: &str,
        values: &[&str],
    ) -> SessionConfigOptionInfo {
        SessionConfigOptionInfo {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            category: category.map(str::to_string),
            kind: SessionConfigKindInfo::Select(SessionConfigSelectInfo {
                current_value: current.to_string(),
                options: values
                    .iter()
                    .map(|value| crate::acp::types::SessionConfigSelectOptionInfo {
                        value: value.to_string(),
                        name: value.to_string(),
                        description: None,
                    })
                    .collect(),
                groups: Vec::new(),
            }),
        }
    }

    async fn fixture() -> (SelectorHostControl, Arc<FakeSelectorRuntime>, i32, i32) {
        let db = Arc::new(fresh_in_memory_db().await);
        let folder = seed_folder(&db, "/tmp/codeg-host-selectors").await;
        let caller = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let runtime = Arc::new(FakeSelectorRuntime::default());
        let provider = SelectorHostControl::new_with_runtime(db, runtime.clone());
        (provider, runtime, caller, target)
    }

    fn caller(id: i32) -> HostControlCaller {
        HostControlCaller {
            current_session_id: id,
            working_dir: PathBuf::from("/tmp/codeg-host-selectors"),
            writes_allowed: true,
        }
    }

    async fn pinned_values(
        host: &SelectorHostControl,
        session_id: i32,
    ) -> BTreeMap<String, String> {
        conversation_service::selector_prefs(&host.db.conn, session_id)
            .await
            .unwrap()
            .1
    }

    #[tokio::test]
    async fn get_selectors_reports_pins_and_live_advertised_values() {
        let (host, runtime, caller_id, target_id) = fixture().await;
        conversation_service::merge_selector_config_value(
            &host.db.conn,
            target_id,
            PREFERRED_MODEL_CONFIG_KEY,
            "claude-opus",
        )
        .await
        .unwrap();
        *runtime.advertised.lock().await = Some(vec![
            select_option(
                "provider-model",
                Some("model"),
                "claude-sonnet",
                &["claude-sonnet", "claude-opus"],
            ),
            select_option(
                "thought_level",
                Some("thought_level"),
                "medium",
                &["low", "medium", "high"],
            ),
        ]);

        let outcome = host
            .use_action(
                &caller(caller_id),
                "req-get-selectors".into(),
                "session.get_selectors".into(),
                json!({ "session_id": target_id }),
            )
            .await;
        assert!(outcome.accepted, "{outcome:?}");
        assert_eq!(outcome.stage, "read");
        assert_eq!(outcome.data["runtime_active"], true);
        assert_eq!(outcome.data["model"]["pinned"], "claude-opus");
        assert_eq!(outcome.data["model"]["current"], "claude-sonnet");
        assert_eq!(outcome.data["model"]["config_id"], "provider-model");
        assert_eq!(
            outcome.data["model"]["available"],
            json!(["claude-sonnet", "claude-opus"])
        );
        assert_eq!(outcome.data["thinking_effort"]["pinned"], Value::Null);
        assert_eq!(outcome.data["thinking_effort"]["current"], "medium");
        assert_eq!(outcome.data["thinking_effort"]["supported"], true);
    }

    #[tokio::test]
    async fn get_selectors_without_a_runtime_reports_pins_only() {
        let (host, _, caller_id, target_id) = fixture().await;
        conversation_service::merge_selector_config_value(
            &host.db.conn,
            target_id,
            PREFERRED_THOUGHT_LEVEL_CONFIG_KEY,
            "high",
        )
        .await
        .unwrap();

        let outcome = host
            .use_action(
                &caller(caller_id),
                "req-get-offline".into(),
                "session.get_selectors".into(),
                json!({ "session_id": target_id }),
            )
            .await;
        assert!(outcome.accepted, "{outcome:?}");
        assert_eq!(outcome.data["runtime_active"], false);
        assert_eq!(outcome.data["thinking_effort"]["pinned"], "high");
        assert_eq!(outcome.data["thinking_effort"]["current"], Value::Null);
        assert_eq!(outcome.data["thinking_effort"]["supported"], Value::Null);
        assert_eq!(outcome.data["model"]["available"], Value::Null);
    }

    #[tokio::test]
    async fn set_selectors_pins_semantic_keys_and_applies_live() {
        let (host, runtime, caller_id, target_id) = fixture().await;
        *runtime.advertised.lock().await = Some(vec![
            select_option("model", Some("model"), "a", &["a", "b"]),
            select_option("reasoning_effort", Some("mode"), "low", &["low", "high"]),
        ]);

        let outcome = host
            .use_action(
                &caller(caller_id),
                "req-set".into(),
                "session.set_selectors".into(),
                json!({ "session_id": target_id, "model": "b", "thinking_effort": "high" }),
            )
            .await;
        assert!(outcome.accepted, "{outcome:?}");
        assert_eq!(outcome.stage, "persisted");
        assert_eq!(outcome.data["model"]["applied_live"], true);
        assert_eq!(outcome.data["model"]["config_id"], "model");
        assert_eq!(outcome.data["thinking_effort"]["applied_live"], true);
        // Grok's effort selector carries category "mode"; the id is what must
        // not be confused with a Harness's real mode selector.
        assert_eq!(
            outcome.data["thinking_effort"]["config_id"],
            "reasoning_effort"
        );

        let pins = pinned_values(&host, target_id).await;
        assert_eq!(
            pins.get(PREFERRED_MODEL_CONFIG_KEY).map(String::as_str),
            Some("b")
        );
        assert_eq!(
            pins.get(PREFERRED_THOUGHT_LEVEL_CONFIG_KEY)
                .map(String::as_str),
            Some("high")
        );

        let applied = runtime.applied.lock().await;
        assert_eq!(
            *applied,
            [
                (target_id, "model".to_string(), "b".to_string()),
                (
                    target_id,
                    "reasoning_effort".to_string(),
                    "high".to_string()
                ),
            ]
        );
    }

    #[tokio::test]
    async fn set_selectors_rejects_a_value_the_live_harness_does_not_offer() {
        let (host, runtime, caller_id, target_id) = fixture().await;
        *runtime.advertised.lock().await = Some(vec![
            select_option("model", Some("model"), "a", &["a", "b"]),
            select_option("thought_level", None, "low", &["low", "high"]),
        ]);

        let outcome = host
            .use_action(
                &caller(caller_id),
                "req-set-invalid".into(),
                "session.set_selectors".into(),
                json!({ "session_id": target_id, "model": "b", "thinking_effort": "xhigh" }),
            )
            .await;
        assert!(!outcome.accepted);
        let note = outcome.note.unwrap();
        assert!(note.contains("thinking_effort"), "{note}");
        assert!(note.contains("low, high"), "{note}");
        // All-or-nothing: the valid model change was not pinned either.
        assert!(pinned_values(&host, target_id).await.is_empty());
        assert!(runtime.applied.lock().await.is_empty());
    }

    #[tokio::test]
    async fn set_selectors_without_a_runtime_pins_unchecked() {
        let (host, runtime, caller_id, target_id) = fixture().await;
        let outcome = host
            .use_action(
                &caller(caller_id),
                "req-set-offline".into(),
                "session.set_selectors".into(),
                json!({ "session_id": target_id, "thinking_effort": "ultra" }),
            )
            .await;
        assert!(outcome.accepted, "{outcome:?}");
        assert_eq!(outcome.data["thinking_effort"]["applied_live"], false);
        assert_eq!(
            outcome.data["thinking_effort"]["live_status"],
            "no_active_runtime"
        );
        let pins = pinned_values(&host, target_id).await;
        assert_eq!(
            pins.get(PREFERRED_THOUGHT_LEVEL_CONFIG_KEY)
                .map(String::as_str),
            Some("ultra")
        );
        assert!(runtime.applied.lock().await.is_empty());
    }

    #[tokio::test]
    async fn set_selectors_reports_a_live_apply_failure_without_losing_the_pin() {
        let (host, runtime, caller_id, target_id) = fixture().await;
        *runtime.advertised.lock().await = Some(vec![select_option(
            "model",
            Some("model"),
            "a",
            &["a", "b"],
        )]);
        *runtime.apply_error.lock().await = Some("process exited".to_string());

        let outcome = host
            .use_action(
                &caller(caller_id),
                "req-set-apply-fails".into(),
                "session.set_selectors".into(),
                json!({ "session_id": target_id, "model": "b" }),
            )
            .await;
        assert!(outcome.accepted, "{outcome:?}");
        assert_eq!(outcome.data["model"]["applied_live"], false);
        assert_eq!(outcome.data["model"]["live_status"], "apply_failed");
        assert_eq!(outcome.data["model"]["live_error"], "process exited");
        assert!(outcome.note.unwrap().contains("process exited"));
        let pins = pinned_values(&host, target_id).await;
        assert_eq!(
            pins.get(PREFERRED_MODEL_CONFIG_KEY).map(String::as_str),
            Some("b")
        );
    }

    #[tokio::test]
    async fn set_selectors_requires_a_field_and_rejects_unknown_fields() {
        let (host, _, caller_id, target_id) = fixture().await;
        let empty = host
            .use_action(
                &caller(caller_id),
                "req-set-empty".into(),
                "session.set_selectors".into(),
                json!({ "session_id": target_id }),
            )
            .await;
        assert!(!empty.accepted);
        assert!(empty.note.unwrap().contains("at least one"));

        let unknown = host
            .use_action(
                &caller(caller_id),
                "req-set-unknown".into(),
                "session.set_selectors".into(),
                json!({ "session_id": target_id, "mode_id": "plan" }),
            )
            .await;
        assert!(!unknown.accepted);
        assert!(unknown.note.unwrap().contains("unknown field"));
    }

    #[tokio::test]
    async fn selectors_reject_cross_project_targets() {
        let (host, _, caller_id, _) = fixture().await;
        let other_folder = seed_folder(&host.db, "/tmp/codeg-host-selectors-other").await;
        let other_id = seed_conversation(&host.db, other_folder, AgentType::Gemini).await;

        let get = host
            .use_action(
                &caller(caller_id),
                "req-get-cross".into(),
                "session.get_selectors".into(),
                json!({ "session_id": other_id }),
            )
            .await;
        assert!(!get.accepted);
        assert!(get.note.unwrap().contains("outside"));

        let set = host
            .use_action(
                &caller(caller_id),
                "req-set-cross".into(),
                "session.set_selectors".into(),
                json!({ "session_id": other_id, "model": "b" }),
            )
            .await;
        assert!(!set.accepted);
        assert!(pinned_values(&host, other_id).await.is_empty());
    }

    #[tokio::test]
    async fn set_selectors_replays_by_request_id() {
        let (host, runtime, caller_id, target_id) = fixture().await;
        let input = json!({ "session_id": target_id, "model": "any-model" });
        let first = host
            .use_action(
                &caller(caller_id),
                "req-set-replay".into(),
                "session.set_selectors".into(),
                input.clone(),
            )
            .await;
        assert!(first.accepted);
        assert!(!first.replayed);

        let replay = host
            .use_action(
                &caller(caller_id),
                "req-set-replay".into(),
                "session.set_selectors".into(),
                input,
            )
            .await;
        assert!(replay.accepted);
        assert!(replay.replayed);
        assert!(runtime.applied.lock().await.is_empty());

        let conflicting = host
            .use_action(
                &caller(caller_id),
                "req-set-replay".into(),
                "session.set_selectors".into(),
                json!({ "session_id": target_id, "model": "another-model" }),
            )
            .await;
        assert!(!conflicting.accepted);
        assert!(conflicting
            .note
            .unwrap()
            .contains("already used with different action input"));
    }

    #[test]
    fn selector_catalog_gates_writes_and_carries_no_caller_identity() {
        let read_only = SelectorHostControl::capabilities(false);
        assert_eq!(read_only.len(), 1);
        assert_eq!(read_only[0].action, "session.get_selectors");
        assert_eq!(read_only[0].access, HostControlAccessLevel::Read);

        let writable = SelectorHostControl::capabilities(true);
        assert!(writable
            .iter()
            .any(|capability| capability.action == "session.set_selectors"
                && capability.access == HostControlAccessLevel::Write));
        let encoded = serde_json::to_string(&writable).unwrap();
        for forbidden in ["from_session_id", "source_session_id", "current_session_id"] {
            assert!(!encoded.contains(forbidden));
        }
    }
}
