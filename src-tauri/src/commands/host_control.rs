//! Host Core behind the progressive `codeg_help` / `codeg_use` MCP gateway.
//!
//! This module owns the capability catalog and the typed Session action
//! dispatcher. Tauri, HTTP, and the MCP companion must not reimplement these
//! semantics. The first slice intentionally contains only Session list/get/
//! rename; create is withheld until the existing lazy-row + ACP handshake can
//! guarantee a persistent external Session identity with the initial prompt.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::acp::host_control::{
    HostControlAccess, HostControlAccessLevel, HostControlCaller, HostControlCapability,
    HostControlHelpOutcome, HostControlRuntimeConfig, HostControlUseOutcome,
    HOST_CONTROL_CATALOG_VERSION,
};
use crate::chat_channel::manager::ChatChannelManager;
use crate::commands::conversations::{
    emit_conversation_upsert, list_all_conversations_core,
    sync_conversation_title_to_channels_core,
};
use crate::commands::host_control_organization::OrganizationHostControl;
use crate::db::entities::conversation::ConversationKind;
use crate::db::service::conversation_service;
use crate::db::AppDatabase;
use crate::models::DbConversationSummary;
use crate::web::event_bridge::EventEmitter;

const DEFAULT_SESSION_LIST_LIMIT: u32 = 50;
const MAX_SESSION_LIST_LIMIT: u32 = 200;
const MAX_QUERY_CHARS: usize = 200;
const MAX_SESSION_TITLE_CHARS: usize = 200;
const MAX_IDEMPOTENCY_ENTRIES: usize = 512;

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

pub struct DbSessionHostControl {
    db: Arc<AppDatabase>,
    emitter: EventEmitter,
    chat_channel_manager: ChatChannelManager,
    config: HostControlRuntimeConfig,
    organization: OrganizationHostControl,
    /// Held across a write so concurrent replays cannot both pass the lookup.
    writes: Mutex<IdempotencyCache>,
}

impl DbSessionHostControl {
    pub fn new(
        db: Arc<AppDatabase>,
        emitter: EventEmitter,
        chat_channel_manager: ChatChannelManager,
        config: HostControlRuntimeConfig,
    ) -> Self {
        let organization = OrganizationHostControl::new(db.clone(), emitter.clone());
        Self {
            db,
            emitter,
            chat_channel_manager,
            config,
            organization,
            writes: Mutex::new(IdempotencyCache::default()),
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
        if !matches!(session.kind, ConversationKind::Regular | ConversationKind::Chat)
            || session.parent_id.is_some()
        {
            return Err(
                "Host Control is available only to ordinary persistent Codeg Sessions."
                    .to_string(),
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
            || !matches!(target.kind, ConversationKind::Regular | ConversationKind::Chat)
            || target.parent_id.is_some()
        {
            return Err(format!(
                "Session {target_session_id} is outside the calling Session's current project scope."
            ));
        }
        Ok(target)
    }

    fn session_summary(session: DbConversationSummary) -> Value {
        json!({
            "session_id": session.id,
            "title": session.title,
            "agent_type": session.agent_type,
            "status": session.status,
            "kind": session.kind,
            "model": session.model,
            "message_count": session.message_count,
            "updated_at": session.updated_at,
            "archived": session.archived_at.is_some(),
        })
    }

    fn capabilities(writes_allowed: bool) -> Vec<HostControlCapability> {
        let mut capabilities = vec![
            HostControlCapability {
                action: "session.list".to_string(),
                description: "List persistent Sessions in the calling Session's current project scope, newest first."
                    .to_string(),
                access: HostControlAccessLevel::Read,
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "query": {
                            "type": "string",
                            "maxLength": MAX_QUERY_CHARS,
                            "description": "Optional title substring."
                        },
                        "archived": {
                            "type": "boolean",
                            "default": false
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": MAX_SESSION_LIST_LIMIT,
                            "default": DEFAULT_SESSION_LIST_LIMIT
                        }
                    }
                }),
                result_stages: vec!["read".to_string()],
            },
            HostControlCapability {
                action: "session.get".to_string(),
                description: "Read metadata for one persistent Session in the calling Session's current project scope."
                    .to_string(),
                access: HostControlAccessLevel::Read,
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["session_id"],
                    "properties": {
                        "session_id": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": i32::MAX,
                            "description": "Target Session id, not the calling Session identity."
                        }
                    }
                }),
                result_stages: vec!["read".to_string()],
            },
        ];
        if writes_allowed {
            capabilities.push(HostControlCapability {
                action: "session.rename".to_string(),
                description: "Persist a manual title for a Session in the calling Session's current project scope. Omit session_id to rename the caller."
                    .to_string(),
                access: HostControlAccessLevel::Write,
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title"],
                    "properties": {
                        "session_id": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": i32::MAX,
                            "description": "Optional target Session id. Omit for the token-derived current Session."
                        },
                        "title": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": MAX_SESSION_TITLE_CHARS
                        }
                    }
                }),
                result_stages: vec!["persisted".to_string()],
            });
        }
        capabilities.extend(OrganizationHostControl::capabilities(writes_allowed));
        capabilities
    }

    async fn session_list(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: SessionListInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let query = match normalize_optional(params.query, MAX_QUERY_CHARS, "query") {
            Ok(query) => query,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let limit = params.limit.unwrap_or(DEFAULT_SESSION_LIST_LIMIT);
        if !(1..=MAX_SESSION_LIST_LIMIT).contains(&limit) {
            return HostControlUseOutcome::rejected(
                request_id,
                action,
                format!("limit must be between 1 and {MAX_SESSION_LIST_LIMIT}"),
            );
        }
        let caller_scope = match self.caller_scope(caller).await {
            Ok(scope) => scope,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let sessions = match list_all_conversations_core(
            &self.db.conn,
            Some(vec![caller_scope.folder_id]),
            None,
            query,
            None,
            None,
            params.archived,
            false,
        )
        .await
        {
            Ok(sessions) => sessions,
            Err(error) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    format!("Could not list Sessions: {error}"),
                )
            }
        };
        let mut sessions: Vec<_> = sessions
            .into_iter()
            .filter(|session| session.id != caller.current_session_id)
            .collect();
        let truncated = sessions.len() > limit as usize;
        sessions.truncate(limit as usize);
        let items: Vec<Value> = sessions.into_iter().map(Self::session_summary).collect();
        HostControlUseOutcome {
            accepted: true,
            request_id,
            action,
            stage: "read".to_string(),
            replayed: false,
            data: json!({ "sessions": items, "truncated": truncated }),
            note: None,
        }
    }

    async fn session_get(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: SessionGetInput = match parse_input(&action, input) {
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
        let session = match self.manageable_target(caller, params.session_id).await {
            Ok(session) => session,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        HostControlUseOutcome {
            accepted: true,
            request_id,
            action,
            stage: "read".to_string(),
            replayed: false,
            note: None,
            data: Self::session_summary(session),
        }
    }

    async fn session_rename(
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

        let params: SessionRenameInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let title = params.title.trim();
        if title.is_empty() {
            return HostControlUseOutcome::rejected(request_id, action, "title must not be empty");
        }
        if title.chars().count() > MAX_SESSION_TITLE_CHARS {
            return HostControlUseOutcome::rejected(
                request_id,
                action,
                format!("title must be at most {MAX_SESSION_TITLE_CHARS} characters"),
            );
        }
        let session_id = params.session_id.unwrap_or(caller.current_session_id);
        if session_id <= 0 {
            return HostControlUseOutcome::rejected(
                request_id,
                action,
                "session_id must be a positive integer",
            );
        }
        let target = match self.manageable_target(caller, session_id).await {
            Ok(target) => target,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        match conversation_service::update_title_if_live_in_folder(
            &self.db.conn,
            session_id,
            target.folder_id,
            title.to_string(),
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    format!("Session {session_id} is no longer available to rename."),
                )
            }
            Err(error) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    format!("Could not rename Session {session_id}: {error}"),
                )
            }
        }
        emit_conversation_upsert(&self.emitter, &self.db.conn, session_id).await;
        sync_conversation_title_to_channels_core(
            &self.db.conn,
            &self.chat_channel_manager,
            session_id,
        )
        .await;
        let summary = conversation_service::get_by_id(&self.db.conn, session_id)
            .await
            .ok();
        let outcome = HostControlUseOutcome {
            accepted: true,
            request_id: request_id.clone(),
            action: action.clone(),
            stage: "persisted".to_string(),
            replayed: false,
            data: json!({
                "session_id": session_id,
                "title": title,
                "title_locked": summary.as_ref().map(|value| value.title_locked).unwrap_or(true),
                "updated_at": summary.as_ref().map(|value| value.updated_at),
            }),
            note: Some(format!("Renamed Session {session_id} to {title:?}.")),
        };
        writes.insert(
            request_id,
            CachedWrite {
                fingerprint,
                outcome: outcome.clone(),
            },
        );
        outcome
    }
}

#[async_trait]
impl HostControlAccess for DbSessionHostControl {
    async fn help(
        &self,
        caller: HostControlCaller,
        query: Option<String>,
        action: Option<String>,
    ) -> HostControlHelpOutcome {
        let config = self.config.snapshot().await;
        if !config.enabled {
            return HostControlHelpOutcome::unavailable(
                "Codeg Host Control is disabled in the current runtime policy.",
            );
        }
        let writes_allowed = config.writes_enabled && caller.writes_allowed;
        let query = query.map(|value| value.to_lowercase());
        let mut capabilities = Self::capabilities(writes_allowed);
        if let Some(action) = action.as_deref() {
            capabilities.retain(|capability| capability.action == action);
        }
        if let Some(query) = query.as_deref() {
            capabilities.retain(|capability| {
                capability.action.to_lowercase().contains(query)
                    || capability.description.to_lowercase().contains(query)
            });
        }
        let note = capabilities.is_empty().then(|| {
            "No currently permitted Host Control action matched the requested filter.".to_string()
        });
        HostControlHelpOutcome {
            available: true,
            catalog_version: HOST_CONTROL_CATALOG_VERSION.to_string(),
            capabilities,
            note,
        }
    }

    async fn use_action(
        &self,
        caller: HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let config = self.config.snapshot().await;
        if !config.enabled {
            return HostControlUseOutcome::rejected(
                request_id,
                action,
                "Codeg Host Control is disabled in the current runtime policy.",
            );
        }
        match action.as_str() {
            "session.list" => self.session_list(&caller, request_id, action, input).await,
            "session.get" => self.session_get(&caller, request_id, action, input).await,
            "session.rename" => {
                if !config.writes_enabled || !caller.writes_allowed {
                    HostControlUseOutcome::rejected(
                        request_id,
                        action,
                        "This Session's live Host policy does not allow Host Control writes.",
                    )
                } else {
                    self.session_rename(&caller, request_id, action, input)
                        .await
                }
            }
            _ if OrganizationHostControl::access_for(&action).is_some() => {
                if matches!(
                    OrganizationHostControl::access_for(&action),
                    Some(HostControlAccessLevel::Write)
                ) && (!config.writes_enabled || !caller.writes_allowed)
                {
                    HostControlUseOutcome::rejected(
                        request_id,
                        action,
                        "This Session's live Host policy does not allow Host Control writes.",
                    )
                } else {
                    self.organization
                        .use_action(&caller, request_id, action, input)
                        .await
                }
            }
            _ => HostControlUseOutcome::rejected(
                request_id,
                action,
                "Unknown or currently unavailable Host Control action. Call codeg_help to refresh the catalog.",
            ),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionListInput {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    archived: bool,
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionGetInput {
    session_id: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionRenameInput {
    #[serde(default)]
    session_id: Option<i32>,
    title: String,
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

fn write_fingerprint(action: &str, input: &Value) -> String {
    let encoded = serde_json::to_vec(&(action, input)).unwrap_or_default();
    format!("{:x}", Sha256::digest(encoded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::host_control::{HostControlCaller, HostControlConfig};
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::agent::AgentType;
    use std::path::PathBuf;

    async fn fixture() -> (DbSessionHostControl, HostControlRuntimeConfig, i32, i32) {
        let db = Arc::new(fresh_in_memory_db().await);
        let folder = seed_folder(&db, "/tmp/codeg-host-control").await;
        let caller = seed_conversation(&db, folder, AgentType::Codex).await;
        let target = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let config = HostControlRuntimeConfig::new();
        let host = DbSessionHostControl::new(
            db,
            EventEmitter::Noop,
            ChatChannelManager::new(),
            config.clone(),
        );
        (host, config, caller, target)
    }

    fn caller(id: i32, writes_allowed: bool) -> HostControlCaller {
        HostControlCaller {
            current_session_id: id,
            working_dir: PathBuf::from("/tmp/codeg-host-control"),
            writes_allowed,
        }
    }

    #[tokio::test]
    async fn catalog_filters_writes_by_token_bound_policy() {
        let (host, _, caller_id, _) = fixture().await;
        let read_only = host.help(caller(caller_id, false), None, None).await;
        assert!(read_only.available);
        assert_eq!(read_only.capabilities.len(), 4);
        assert!(read_only
            .capabilities
            .iter()
            .all(|capability| capability.access == HostControlAccessLevel::Read));
        assert!(read_only
            .capabilities
            .iter()
            .any(|capability| capability.action == "collection.list"));
        assert!(read_only
            .capabilities
            .iter()
            .any(|capability| capability.action == "workbench.list"));

        let writable = host.help(caller(caller_id, true), None, None).await;
        assert!(writable
            .capabilities
            .iter()
            .any(|capability| capability.action == "session.rename"));
        assert!(writable
            .capabilities
            .iter()
            .any(|capability| capability.action == "collection.add_session"));
        assert!(writable
            .capabilities
            .iter()
            .any(|capability| capability.action == "workbench.add_session"));
    }

    #[tokio::test]
    async fn list_excludes_the_token_derived_current_session() {
        let (host, _, caller_id, target_id) = fixture().await;
        let outcome = host
            .use_action(
                caller(caller_id, true),
                "req-list".into(),
                "session.list".into(),
                json!({}),
            )
            .await;
        assert!(outcome.accepted);
        let ids: Vec<i64> = outcome.data["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value["session_id"].as_i64())
            .collect();
        assert_eq!(ids, vec![i64::from(target_id)]);
        assert!(!ids.contains(&i64::from(caller_id)));
    }

    #[tokio::test]
    async fn list_and_get_do_not_cross_the_callers_project_scope_or_return_transcript() {
        let (host, _, caller_id, target_id) = fixture().await;
        let other_folder = seed_folder(&host.db, "/tmp/codeg-host-control-other").await;
        let other_id = seed_conversation(&host.db, other_folder, AgentType::Gemini).await;

        let listed = host
            .use_action(
                caller(caller_id, true),
                "req-scoped-list".into(),
                "session.list".into(),
                json!({}),
            )
            .await;
        let listed_ids: Vec<i64> = listed.data["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value["session_id"].as_i64())
            .collect();
        assert_eq!(listed_ids, vec![i64::from(target_id)]);
        assert!(!listed_ids.contains(&i64::from(other_id)));

        let same_scope = host
            .use_action(
                caller(caller_id, true),
                "req-scoped-get".into(),
                "session.get".into(),
                json!({ "session_id": target_id }),
            )
            .await;
        assert!(same_scope.accepted);
        assert_eq!(same_scope.data["session_id"], target_id);
        assert!(same_scope.data.get("messages").is_none());
        assert!(same_scope.data.get("turns").is_none());

        let cross_scope = host
            .use_action(
                caller(caller_id, true),
                "req-cross-get".into(),
                "session.get".into(),
                json!({ "session_id": other_id }),
            )
            .await;
        assert!(!cross_scope.accepted);
        assert!(cross_scope.note.unwrap().contains("outside"));
    }

    #[tokio::test]
    async fn typed_inputs_reject_unknown_fields() {
        let (host, _, caller_id, target_id) = fixture().await;
        let outcome = host
            .use_action(
                caller(caller_id, true),
                "req-get".into(),
                "session.get".into(),
                json!({ "session_id": target_id, "from_session_id": caller_id }),
            )
            .await;
        assert!(!outcome.accepted);
        assert!(outcome.note.unwrap().contains("unknown field"));
    }

    #[tokio::test]
    async fn rename_defaults_to_caller_and_replays_idempotently() {
        let (host, _, caller_id, _) = fixture().await;
        let first = host
            .use_action(
                caller(caller_id, true),
                "req-rename".into(),
                "session.rename".into(),
                json!({ "title": "  Host-managed Session  " }),
            )
            .await;
        assert!(first.accepted);
        assert_eq!(first.stage, "persisted");
        assert!(!first.replayed);

        let replay = host
            .use_action(
                caller(caller_id, true),
                "req-rename".into(),
                "session.rename".into(),
                json!({ "title": "  Host-managed Session  " }),
            )
            .await;
        assert!(replay.accepted);
        assert!(replay.replayed);
        assert_eq!(replay.data["session_id"], caller_id);

        let summary = conversation_service::get_by_id(&host.db.conn, caller_id)
            .await
            .unwrap();
        assert_eq!(summary.title.as_deref(), Some("Host-managed Session"));
        assert!(summary.title_locked);
    }

    #[tokio::test]
    async fn rename_rejects_cross_project_and_soft_deleted_targets() {
        let (host, _, caller_id, target_id) = fixture().await;
        let other_folder = seed_folder(&host.db, "/tmp/codeg-host-control-rename-other").await;
        let other_id = seed_conversation(&host.db, other_folder, AgentType::Gemini).await;

        let cross_scope = host
            .use_action(
                caller(caller_id, true),
                "req-cross-rename".into(),
                "session.rename".into(),
                json!({ "session_id": other_id, "title": "Must not change" }),
            )
            .await;
        assert!(!cross_scope.accepted);
        assert_eq!(
            conversation_service::get_by_id(&host.db.conn, other_id)
                .await
                .unwrap()
                .title,
            None
        );

        conversation_service::soft_delete(&host.db.conn, target_id)
            .await
            .unwrap();
        let deleted = host
            .use_action(
                caller(caller_id, true),
                "req-deleted-rename".into(),
                "session.rename".into(),
                json!({ "session_id": target_id, "title": "Ghost title" }),
            )
            .await;
        assert!(!deleted.accepted);
        assert!(deleted.note.unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn runtime_policy_is_rechecked_for_existing_access_object() {
        let (host, config, caller_id, target_id) = fixture().await;
        config
            .set(HostControlConfig {
                enabled: true,
                writes_enabled: false,
            })
            .await;
        let denied = host
            .use_action(
                caller(caller_id, true),
                "req-denied".into(),
                "session.rename".into(),
                json!({ "session_id": target_id, "title": "Denied" }),
            )
            .await;
        assert!(!denied.accepted);
        assert!(denied.note.unwrap().contains("does not allow"));

        config
            .set(HostControlConfig {
                enabled: false,
                writes_enabled: false,
            })
            .await;
        let help = host.help(caller(caller_id, true), None, None).await;
        assert!(!help.available);
    }
}
