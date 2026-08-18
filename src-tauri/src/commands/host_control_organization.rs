//! Collection and Workbench capabilities for the progressive Host Control gateway.
//!
//! Collection membership and Workbench Session references are backend-owned
//! SQLite facts. Pane layout, window mounts and focus are still device-local,
//! so this provider deliberately exposes no layout/open/focus action and never
//! reports that a client displayed a persisted organization change.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::acp::host_control::{
    HostControlAccessLevel, HostControlCaller, HostControlCapability, HostControlUseOutcome,
};
use crate::commands::collections::{
    assign_conversations_to_collection_core, create_collection_core, list_collections_core,
    list_conversation_collection_refs_core, move_collection_core, rename_collection_core,
};
use crate::commands::conversations::{list_workbench_tabs_core, save_workbench_tabs_core};
use crate::commands::workbenches::{
    create_workbench_core, list_workbenches_core, rename_workbench_core,
};
use crate::db::entities::conversation::ConversationKind;
use crate::db::service::{conversation_service, folder_service};
use crate::db::AppDatabase;
use crate::models::{CollectionInfo, DbConversationSummary, OpenedTab, WorkbenchInfo};
use crate::web::event_bridge::{emit_event, EventEmitter};

pub const ORGANIZATION_CHANGED_EVENT: &str = "organization://changed";
pub const WORKBENCH_PLACE_SESSION_EVENT: &str = "workbench://place-session";

const MAX_NAME_CHARS: usize = 80;
const MAX_IDEMPOTENCY_ENTRIES: usize = 512;
const MAX_TAB_CAS_ATTEMPTS: usize = 4;
const HOST_CONTROL_EVENT_ORIGIN: &str = "host-control";

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

#[derive(Debug)]
struct CallerScope {
    root_folder_id: i32,
    folder_ids: HashSet<i32>,
}

pub struct OrganizationHostControl {
    db: Arc<AppDatabase>,
    emitter: EventEmitter,
    /// Held across a write so concurrent replays cannot both pass the lookup.
    writes: Mutex<IdempotencyCache>,
}

impl OrganizationHostControl {
    pub fn new(db: Arc<AppDatabase>, emitter: EventEmitter) -> Self {
        Self {
            db,
            emitter,
            writes: Mutex::new(IdempotencyCache::default()),
        }
    }

    pub fn capabilities(writes_allowed: bool) -> Vec<HostControlCapability> {
        let mut capabilities = vec![
            capability(
                "collection.list",
                "List Collections and primary Session memberships in the calling Session's Path scope.",
                HostControlAccessLevel::Read,
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {}
                }),
                &["read"],
            ),
            capability(
                "workbench.list",
                "List the app-wide saved Workbench namespace. Session references outside the caller's Path are redacted; device-local layout and focus are not returned.",
                HostControlAccessLevel::Read,
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {}
                }),
                &["read"],
            ),
        ];
        if !writes_allowed {
            return capabilities;
        }

        capabilities.extend([
            capability(
                "collection.create",
                "Create a Collection in the calling Session's Path scope without changing any Session cwd or focus.",
                HostControlAccessLevel::Write,
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name"],
                    "properties": {
                        "name": name_schema(),
                        "parent_id": positive_nullable_id_schema("Optional parent Collection id in the same Path scope. Omit or use null for a root Collection.")
                    }
                }),
                &["persisted"],
            ),
            capability(
                "collection.rename",
                "Rename a Collection by stable id without changing Session identities, cwd or focus.",
                HostControlAccessLevel::Write,
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["collection_id", "name"],
                    "properties": {
                        "collection_id": positive_id_schema("Collection id in the calling Session's Path scope."),
                        "name": name_schema()
                    }
                }),
                &["persisted"],
            ),
            capability(
                "collection.move",
                "Move a Collection under another Collection in the same Path scope. Omit parent_id or use null to move it to the Path root.",
                HostControlAccessLevel::Write,
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["collection_id"],
                    "properties": {
                        "collection_id": positive_id_schema("Collection id in the calling Session's Path scope."),
                        "parent_id": positive_nullable_id_schema("New parent Collection id in the same Path scope, or null for the root.")
                    }
                }),
                &["persisted"],
            ),
            capability(
                "collection.add_session",
                "Set one existing Session's primary Collection. This may move it from its previous Collection but never changes its cwd, runtime or identity.",
                HostControlAccessLevel::Write,
                collection_session_schema(),
                &["persisted"],
            ),
            capability(
                "collection.remove_session",
                "Remove one Session from the specified primary Collection, leaving it Unclassified. The Session and runtime are not deleted or stopped.",
                HostControlAccessLevel::Write,
                collection_session_schema(),
                &["persisted"],
            ),
            capability(
                "workbench.create",
                "Create an app-wide saved Workbench without mounting, switching to or focusing it.",
                HostControlAccessLevel::Write,
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": { "name": name_schema() }
                }),
                &["persisted"],
            ),
            capability(
                "workbench.rename",
                "Rename an app-wide saved Workbench by stable id without mounting, switching to or focusing it.",
                HostControlAccessLevel::Write,
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["workbench_id", "name"],
                    "properties": {
                        "workbench_id": positive_id_schema("Saved Workbench id in this Codeg workspace."),
                        "name": name_schema()
                    }
                }),
                &["persisted"],
            ),
            capability(
                "workbench.add_session",
                "Persist a Session reference in a saved Workbench without switching Workbenches or requesting UI focus. Device-local Pane layout is unchanged.",
                HostControlAccessLevel::Write,
                workbench_session_schema(),
                &["persisted"],
            ),
            capability(
                "workbench.place_session",
                "Open and focus a Session in a saved Workbench, optionally placing it to the right or below the current Pane. This is an explicit UI-affecting action.",
                HostControlAccessLevel::Write,
                workbench_place_session_schema(),
                &["ui_requested"],
            ),
            capability(
                "workbench.remove_session",
                "Remove a persisted Session reference from one Workbench. The Session and runtime are not deleted or stopped; no Workbench switch is requested.",
                HostControlAccessLevel::Write,
                workbench_session_schema(),
                &["persisted"],
            ),
        ]);
        capabilities
    }

    pub fn access_for(action: &str) -> Option<HostControlAccessLevel> {
        match action {
            "collection.list" | "workbench.list" => Some(HostControlAccessLevel::Read),
            "collection.create"
            | "collection.rename"
            | "collection.move"
            | "collection.add_session"
            | "collection.remove_session"
            | "workbench.create"
            | "workbench.rename"
            | "workbench.add_session"
            | "workbench.place_session"
            | "workbench.remove_session" => Some(HostControlAccessLevel::Write),
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
        if matches!(
            Self::access_for(&action),
            Some(HostControlAccessLevel::Write)
        ) {
            return self
                .use_write_action(caller, request_id, action, input)
                .await;
        }
        match action.as_str() {
            "collection.list" => {
                self.collection_list(caller, request_id, action, input)
                    .await
            }
            "workbench.list" => self.workbench_list(caller, request_id, action, input).await,
            _ => HostControlUseOutcome::rejected(
                request_id,
                action,
                "Unknown organization action. Call codeg_help to refresh the catalog.",
            ),
        }
    }

    async fn use_write_action(
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

        let outcome = match action.as_str() {
            "collection.create" => {
                self.collection_create(caller, request_id.clone(), action.clone(), input)
                    .await
            }
            "collection.rename" => {
                self.collection_rename(caller, request_id.clone(), action.clone(), input)
                    .await
            }
            "collection.move" => {
                self.collection_move(caller, request_id.clone(), action.clone(), input)
                    .await
            }
            "collection.add_session" => {
                self.collection_add_session(caller, request_id.clone(), action.clone(), input)
                    .await
            }
            "collection.remove_session" => {
                self.collection_remove_session(caller, request_id.clone(), action.clone(), input)
                    .await
            }
            "workbench.create" => {
                self.workbench_create(caller, request_id.clone(), action.clone(), input)
                    .await
            }
            "workbench.rename" => {
                self.workbench_rename(caller, request_id.clone(), action.clone(), input)
                    .await
            }
            "workbench.add_session" => {
                self.workbench_set_session(caller, request_id.clone(), action.clone(), input, true)
                    .await
            }
            "workbench.place_session" => {
                self.workbench_place_session(caller, request_id.clone(), action.clone(), input)
                    .await
            }
            "workbench.remove_session" => {
                self.workbench_set_session(caller, request_id.clone(), action.clone(), input, false)
                    .await
            }
            _ => HostControlUseOutcome::rejected(
                request_id.clone(),
                action.clone(),
                "Unknown organization action. Call codeg_help to refresh the catalog.",
            ),
        };
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

    async fn caller_scope(&self, caller: &HostControlCaller) -> Result<CallerScope, String> {
        let session = conversation_service::get_by_id(&self.db.conn, caller.current_session_id)
            .await
            .map_err(|_| {
                "The calling Session is no longer an active Codeg Session. Resume it before using Host Control."
                    .to_string()
            })?;
        if !is_ordinary_session(&session) {
            return Err(
                "Host Control organization actions are available only to ordinary persistent Codeg Sessions."
                    .to_string(),
            );
        }
        let source_folder = folder_service::get_folder_by_id(&self.db.conn, session.folder_id)
            .await
            .map_err(|error| format!("Could not resolve the calling Session's Path: {error}"))?
            .ok_or_else(|| {
                "The calling Session's execution Folder no longer exists.".to_string()
            })?;
        let root_folder_id = source_folder.parent_id.unwrap_or(source_folder.id);
        let folder_ids = folder_service::list_all_folder_details(&self.db.conn)
            .await
            .map_err(|error| {
                format!("Could not resolve the calling Session's Path scope: {error}")
            })?
            .into_iter()
            .filter(|folder| {
                folder.id == root_folder_id || folder.parent_id == Some(root_folder_id)
            })
            .map(|folder| folder.id)
            .collect();
        Ok(CallerScope {
            root_folder_id,
            folder_ids,
        })
    }

    async fn manageable_session(
        &self,
        scope: &CallerScope,
        session_id: i32,
    ) -> Result<DbConversationSummary, String> {
        require_positive_id(session_id, "session_id")?;
        let session = conversation_service::get_by_id(&self.db.conn, session_id)
            .await
            .map_err(|_| {
                format!("Session {session_id} was not found in the calling Session's Path scope.")
            })?;
        if !scope.folder_ids.contains(&session.folder_id) || !is_ordinary_session(&session) {
            return Err(format!(
                "Session {session_id} is outside the calling Session's Path scope."
            ));
        }
        Ok(session)
    }

    async fn scoped_collections(&self, scope: &CallerScope) -> Result<Vec<CollectionInfo>, String> {
        list_collections_core(&self.db.conn)
            .await
            .map_err(|error| command_error("Could not list Collections", error))
            .map(|items| {
                items
                    .into_iter()
                    .filter(|item| item.root_folder_id == Some(scope.root_folder_id))
                    .collect()
            })
    }

    async fn scoped_collection(
        &self,
        scope: &CallerScope,
        collection_id: i32,
    ) -> Result<CollectionInfo, String> {
        require_positive_id(collection_id, "collection_id")?;
        self.scoped_collections(scope)
            .await?
            .into_iter()
            .find(|item| item.id == collection_id)
            .ok_or_else(|| {
                format!(
                    "Collection {collection_id} was not found in the calling Session's Path scope."
                )
            })
    }

    async fn collection_list(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        if let Err(note) = parse_input::<EmptyInput>(&action, input) {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        let scope = match self.caller_scope(caller).await {
            Ok(scope) => scope,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let collections = match self.scoped_collections(&scope).await {
            Ok(items) => items,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let session_ids = match self.scoped_session_ids(&scope).await {
            Ok(ids) => ids,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let refs = match list_conversation_collection_refs_core(&self.db.conn, session_ids).await {
            Ok(refs) => refs,
            Err(error) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    command_error("Could not list Collection memberships", error),
                )
            }
        };
        let mut sessions_by_collection: HashMap<i32, Vec<i32>> = HashMap::new();
        for item in refs {
            sessions_by_collection
                .entry(item.collection_id)
                .or_default()
                .push(item.conversation_id);
        }
        let collections: Vec<Value> = collections
            .into_iter()
            .map(|item| {
                let session_ids = sessions_by_collection.remove(&item.id).unwrap_or_default();
                json!({
                    "collection_id": item.id,
                    "root_folder_id": item.root_folder_id,
                    "parent_id": item.parent_id,
                    "name": item.name,
                    "position": item.position,
                    "session_ids": session_ids,
                    "created_at": item.created_at,
                    "updated_at": item.updated_at,
                })
            })
            .collect();
        read_outcome(
            request_id,
            action,
            json!({
                "root_folder_id": scope.root_folder_id,
                "collections": collections,
            }),
        )
    }

    async fn scoped_session_ids(&self, scope: &CallerScope) -> Result<Vec<i32>, String> {
        let folder_ids: Vec<i32> = scope.folder_ids.iter().copied().collect();
        let mut sessions = conversation_service::list_all(
            &self.db.conn,
            Some(folder_ids.clone()),
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .await
        .map_err(|error| format!("Could not list Sessions in Path scope: {error}"))?;
        sessions.extend(
            conversation_service::list_all(
                &self.db.conn,
                Some(folder_ids),
                None,
                None,
                None,
                None,
                true,
                false,
            )
            .await
            .map_err(|error| format!("Could not list archived Sessions in Path scope: {error}"))?,
        );
        Ok(sessions
            .into_iter()
            .filter(is_ordinary_session)
            .map(|session| session.id)
            .collect())
    }

    async fn collection_create(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: CollectionCreateInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let name = match normalize_name(params.name) {
            Ok(name) => name,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let scope = match self.caller_scope(caller).await {
            Ok(scope) => scope,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        if let Some(parent_id) = params.parent_id {
            if let Err(note) = self.scoped_collection(&scope, parent_id).await {
                return HostControlUseOutcome::rejected(request_id, action, note);
            }
        }
        let created = match create_collection_core(
            &self.db.conn,
            name,
            params.parent_id,
            Some(scope.root_folder_id),
        )
        .await
        {
            Ok(created) => created,
            Err(error) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    command_error("Could not create Collection", error),
                )
            }
        };
        self.emit_change("collection", created.id);
        persisted_outcome(
            request_id,
            action,
            json!({ "collection": created }),
            "Created the Collection without changing any Session execution context or UI focus.",
        )
    }

    async fn collection_rename(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: CollectionRenameInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let name = match normalize_name(params.name) {
            Ok(name) => name,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let scope = match self.caller_scope(caller).await {
            Ok(scope) => scope,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        if let Err(note) = self.scoped_collection(&scope, params.collection_id).await {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        let updated = match rename_collection_core(&self.db.conn, params.collection_id, name).await
        {
            Ok(updated) => updated,
            Err(error) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    command_error("Could not rename Collection", error),
                )
            }
        };
        self.emit_change("collection", updated.id);
        persisted_outcome(
            request_id,
            action,
            json!({ "collection": updated }),
            "Renamed the Collection without changing Session identities, execution contexts or UI focus.",
        )
    }

    async fn collection_move(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: CollectionMoveInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let scope = match self.caller_scope(caller).await {
            Ok(scope) => scope,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        if let Err(note) = self.scoped_collection(&scope, params.collection_id).await {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        if let Some(parent_id) = params.parent_id {
            if let Err(note) = self.scoped_collection(&scope, parent_id).await {
                return HostControlUseOutcome::rejected(request_id, action, note);
            }
        }
        let updated =
            match move_collection_core(&self.db.conn, params.collection_id, params.parent_id).await
            {
                Ok(updated) => updated,
                Err(error) => {
                    return HostControlUseOutcome::rejected(
                        request_id,
                        action,
                        command_error("Could not move Collection", error),
                    )
                }
            };
        self.emit_change("collection", updated.id);
        persisted_outcome(
            request_id,
            action,
            json!({ "collection": updated }),
            "Moved the Collection without changing any Session execution context or UI focus.",
        )
    }

    async fn collection_add_session(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: CollectionSessionInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let scope = match self.caller_scope(caller).await {
            Ok(scope) => scope,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        if let Err(note) = self.scoped_collection(&scope, params.collection_id).await {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        if let Err(note) = self.manageable_session(&scope, params.session_id).await {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        let previous_collection_id = match self.session_collection_id(params.session_id).await {
            Ok(value) => value,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let refs = match assign_conversations_to_collection_core(
            &self.db.conn,
            vec![params.session_id],
            Some(params.collection_id),
        )
        .await
        {
            Ok(refs) => refs,
            Err(error) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    command_error("Could not add Session to Collection", error),
                )
            }
        };
        self.emit_change("collection_membership", params.collection_id);
        persisted_outcome(
            request_id,
            action,
            json!({
                "session_id": params.session_id,
                "collection_id": params.collection_id,
                "previous_collection_id": previous_collection_id,
                "changed": previous_collection_id != Some(params.collection_id),
                "membership": refs.into_iter().next(),
            }),
            "Persisted the Session's primary Collection. Its stable identity, cwd, runtime and UI focus were unchanged.",
        )
    }

    async fn collection_remove_session(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: CollectionSessionInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let scope = match self.caller_scope(caller).await {
            Ok(scope) => scope,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        if let Err(note) = self.scoped_collection(&scope, params.collection_id).await {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        if let Err(note) = self.manageable_session(&scope, params.session_id).await {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        let previous_collection_id = match self.session_collection_id(params.session_id).await {
            Ok(value) => value,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        if previous_collection_id != Some(params.collection_id) {
            return HostControlUseOutcome::rejected(
                request_id,
                action,
                format!(
                    "Session {} is not primarily assigned to Collection {}.",
                    params.session_id, params.collection_id
                ),
            );
        }
        if let Err(error) =
            assign_conversations_to_collection_core(&self.db.conn, vec![params.session_id], None)
                .await
        {
            return HostControlUseOutcome::rejected(
                request_id,
                action,
                command_error("Could not remove Session from Collection", error),
            );
        }
        self.emit_change("collection_membership", params.collection_id);
        persisted_outcome(
            request_id,
            action,
            json!({
                "session_id": params.session_id,
                "collection_id": Value::Null,
                "previous_collection_id": params.collection_id,
                "changed": true,
            }),
            "The Session is now Unclassified. Its stable identity, cwd, runtime and UI focus were unchanged.",
        )
    }

    async fn session_collection_id(&self, session_id: i32) -> Result<Option<i32>, String> {
        list_conversation_collection_refs_core(&self.db.conn, vec![session_id])
            .await
            .map_err(|error| command_error("Could not read Collection membership", error))
            .map(|refs| refs.into_iter().next().map(|item| item.collection_id))
    }

    async fn existing_workbench(&self, workbench_id: i32) -> Result<WorkbenchInfo, String> {
        require_positive_id(workbench_id, "workbench_id")?;
        list_workbenches_core(&self.db.conn)
            .await
            .map_err(|error| command_error("Could not list Workbenches", error))?
            .into_iter()
            .find(|workbench| workbench.id == workbench_id)
            .ok_or_else(|| {
                format!("Workbench {workbench_id} was not found in this Codeg workspace.")
            })
    }

    async fn workbench_list(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        if let Err(note) = parse_input::<EmptyInput>(&action, input) {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        let scope = match self.caller_scope(caller).await {
            Ok(scope) => scope,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let allowed_session_ids: HashSet<i32> = match self.scoped_session_ids(&scope).await {
            Ok(ids) => ids.into_iter().collect(),
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let workbenches = match list_workbenches_core(&self.db.conn).await {
            Ok(items) => items,
            Err(error) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    command_error("Could not list Workbenches", error),
                )
            }
        };
        let mut items = Vec::with_capacity(workbenches.len());
        for workbench in workbenches {
            let snapshot = match list_workbench_tabs_core(&self.db.conn, workbench.id).await {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return HostControlUseOutcome::rejected(
                        request_id,
                        action,
                        command_error("Could not list Workbench Session references", error),
                    )
                }
            };
            let all_session_count = snapshot
                .items
                .iter()
                .filter(|tab| tab.conversation_id.is_some())
                .count();
            let session_ids: Vec<i32> = snapshot
                .items
                .into_iter()
                .filter_map(|tab| tab.conversation_id)
                .filter(|session_id| allowed_session_ids.contains(session_id))
                .collect();
            items.push(json!({
                "workbench_id": workbench.id,
                "name": workbench.name,
                "position": workbench.position,
                "is_pinned": workbench.is_pinned,
                "session_ids": session_ids,
                "contains_out_of_scope_sessions": all_session_count > session_ids.len(),
                "created_at": workbench.created_at,
                "updated_at": workbench.updated_at,
            }));
        }
        read_outcome(
            request_id,
            action,
            json!({
                "workbenches": items,
                "membership_authority": "backend",
                "layout_authority": "device_local_not_available_to_host_control",
            }),
        )
    }

    async fn workbench_create(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: WorkbenchCreateInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        if let Err(note) = self.caller_scope(caller).await {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        let name = match params.name.map(normalize_name).transpose() {
            Ok(name) => name,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let created = match create_workbench_core(&self.db.conn, name).await {
            Ok(created) => created,
            Err(error) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    command_error("Could not create Workbench", error),
                )
            }
        };
        self.emit_change("workbench", created.id);
        persisted_outcome(
            request_id,
            action,
            json!({
                "workbench": created,
                "mounted": false,
                "focused": false,
                "layout": "not_created",
            }),
            "Created the saved Workbench metadata. It was not mounted or focused, and no device-local layout was fabricated.",
        )
    }

    async fn workbench_rename(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: WorkbenchRenameInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        if let Err(note) = self.caller_scope(caller).await {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        if let Err(note) = self.existing_workbench(params.workbench_id).await {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        let name = match normalize_name(params.name) {
            Ok(name) => name,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let updated = match rename_workbench_core(&self.db.conn, params.workbench_id, name).await {
            Ok(updated) => updated,
            Err(error) => {
                return HostControlUseOutcome::rejected(
                    request_id,
                    action,
                    command_error("Could not rename Workbench", error),
                )
            }
        };
        self.emit_change("workbench", updated.id);
        persisted_outcome(
            request_id,
            action,
            json!({
                "workbench": updated,
                "mounted": Value::Null,
                "focused": Value::Null,
                "layout_changed": false,
            }),
            "Renamed the saved Workbench metadata without switching Workbenches or changing device-local layout/focus.",
        )
    }

    async fn workbench_set_session(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
        present: bool,
    ) -> HostControlUseOutcome {
        let params: WorkbenchSessionInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let scope = match self.caller_scope(caller).await {
            Ok(scope) => scope,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        if let Err(note) = self.existing_workbench(params.workbench_id).await {
            return HostControlUseOutcome::rejected(request_id, action, note);
        }
        let session = match self.manageable_session(&scope, params.session_id).await {
            Ok(session) => session,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };

        for _ in 0..MAX_TAB_CAS_ATTEMPTS {
            let snapshot = match list_workbench_tabs_core(&self.db.conn, params.workbench_id).await
            {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return HostControlUseOutcome::rejected(
                        request_id,
                        action,
                        command_error("Could not read Workbench Session references", error),
                    )
                }
            };
            let contains = snapshot
                .items
                .iter()
                .any(|tab| tab.conversation_id == Some(params.session_id));
            if contains == present {
                return persisted_outcome(
                    request_id,
                    action,
                    json!({
                        "workbench_id": params.workbench_id,
                        "session_id": params.session_id,
                        "changed": false,
                        "version": snapshot.version,
                        "layout_changed": false,
                        "focus_requested": false,
                    }),
                    "The requested Workbench membership already matched backend truth; no layout or focus command was issued.",
                );
            }

            let mut items = snapshot.items;
            if present {
                let position = items.iter().map(|item| item.position).max().unwrap_or(-1) + 1;
                items.push(OpenedTab {
                    id: 0,
                    folder_id: session.folder_id,
                    conversation_id: Some(session.id),
                    room_id: None,
                    agent_type: session.agent_type,
                    position,
                    is_active: false,
                    is_pinned: true,
                });
            } else {
                items.retain(|tab| tab.conversation_id != Some(params.session_id));
            }
            let saved = match save_workbench_tabs_core(
                &self.db.conn,
                &self.emitter,
                params.workbench_id,
                items,
                snapshot.version,
                HOST_CONTROL_EVENT_ORIGIN.to_string(),
            )
            .await
            {
                Ok(saved) => saved,
                Err(error) => {
                    return HostControlUseOutcome::rejected(
                        request_id,
                        action,
                        command_error("Could not persist Workbench Session reference", error),
                    )
                }
            };
            if !saved.accepted {
                continue;
            }
            self.emit_change("workbench", params.workbench_id);
            return persisted_outcome(
                request_id,
                action,
                json!({
                    "workbench_id": params.workbench_id,
                    "session_id": params.session_id,
                    "changed": true,
                    "present": present,
                    "version": saved.version,
                    "layout_changed": false,
                    "focus_requested": false,
                }),
                if present {
                    "Persisted the Session reference and broadcast backend membership truth. No Workbench switch, explicit focus or device-local Pane layout change was requested."
                } else {
                    "Removed only this Workbench's Session reference. The Session/runtime were not deleted or stopped, and no Workbench switch was requested."
                },
            );
        }

        HostControlUseOutcome::rejected(
            request_id,
            action,
            "The Workbench changed concurrently too many times; no Host Control membership result was claimed. Retry after refreshing with workbench.list.",
        )
    }

    async fn workbench_place_session(
        &self,
        caller: &HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome {
        let params: WorkbenchPlaceSessionInput = match parse_input(&action, input) {
            Ok(params) => params,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let scope = match self.caller_scope(caller).await {
            Ok(scope) => scope,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };
        let session = match self.manageable_session(&scope, params.session_id).await {
            Ok(session) => session,
            Err(note) => return HostControlUseOutcome::rejected(request_id, action, note),
        };

        let membership = self
            .workbench_set_session(
                caller,
                request_id.clone(),
                action.clone(),
                json!({
                    "workbench_id": params.workbench_id,
                    "session_id": params.session_id,
                }),
                true,
            )
            .await;
        if !membership.accepted {
            return membership;
        }

        let placement = params.placement.unwrap_or_default();
        emit_event(
            &self.emitter,
            WORKBENCH_PLACE_SESSION_EVENT,
            json!({
                "requestId": request_id,
                "workbenchId": params.workbench_id,
                "folderId": session.folder_id,
                "conversationId": session.id,
                "agent": session.agent_type,
                "placement": placement,
            }),
        );

        HostControlUseOutcome {
            accepted: true,
            request_id,
            action,
            stage: "ui_requested".to_string(),
            replayed: false,
            data: json!({
                "workbench_id": params.workbench_id,
                "session_id": params.session_id,
                "membership_changed": membership.data.get("changed").cloned().unwrap_or(Value::Bool(false)),
                "version": membership.data.get("version").cloned().unwrap_or(Value::Null),
                "placement": placement,
                "focus_requested": true,
            }),
            note: Some(
                "Persisted the Session reference and requested the connected workspace UI to open, focus and place it. The backend does not claim that a disconnected View applied the request."
                    .to_string(),
            ),
        }
    }

    fn emit_change(&self, entity: &str, id: i32) {
        emit_event(
            &self.emitter,
            ORGANIZATION_CHANGED_EVENT,
            json!({ "entity": entity, "id": id, "origin": HOST_CONTROL_EVENT_ORIGIN }),
        );
    }
}

fn capability(
    action: &str,
    description: &str,
    access: HostControlAccessLevel,
    input_schema: Value,
    stages: &[&str],
) -> HostControlCapability {
    HostControlCapability {
        action: action.to_string(),
        description: description.to_string(),
        access,
        input_schema,
        result_stages: stages.iter().map(|stage| (*stage).to_string()).collect(),
    }
}

fn positive_id_schema(description: &str) -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": i32::MAX,
        "description": description,
    })
}

fn positive_nullable_id_schema(description: &str) -> Value {
    json!({
        "type": ["integer", "null"],
        "minimum": 1,
        "maximum": i32::MAX,
        "description": description,
    })
}

fn name_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": MAX_NAME_CHARS,
    })
}

fn collection_session_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["collection_id", "session_id"],
        "properties": {
            "collection_id": positive_id_schema("Collection id in the calling Session's Path scope."),
            "session_id": positive_id_schema("Existing Session id in the calling Session's Path scope."),
        }
    })
}

fn workbench_session_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["workbench_id", "session_id"],
        "properties": {
            "workbench_id": positive_id_schema("Saved Workbench id in this Codeg workspace."),
            "session_id": positive_id_schema("Existing Session id in the calling Session's Path scope."),
        }
    })
}

fn workbench_place_session_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["workbench_id", "session_id"],
        "properties": {
            "workbench_id": positive_id_schema("Saved Workbench id in this Codeg workspace."),
            "session_id": positive_id_schema("Existing Session id in the calling Session's Path scope."),
            "placement": {
                "type": "string",
                "enum": ["tab", "right", "down"],
                "default": "tab",
                "description": "Open as a tab in the active Pane, or split to the right/below."
            }
        }
    })
}

fn parse_input<T: DeserializeOwned>(action: &str, input: Value) -> Result<T, String> {
    serde_json::from_value(input).map_err(|error| format!("Invalid input for {action}: {error}"))
}

fn normalize_name(name: String) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name must not be empty".to_string());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("name must be at most {MAX_NAME_CHARS} characters"));
    }
    Ok(name.to_string())
}

fn require_positive_id(id: i32, label: &str) -> Result<(), String> {
    if id <= 0 {
        Err(format!("{label} must be a positive integer"))
    } else {
        Ok(())
    }
}

fn command_error(label: &str, error: crate::app_error::AppCommandError) -> String {
    match error.detail {
        Some(detail) => format!("{label}: {detail}"),
        None => format!("{label}: {}", error.message),
    }
}

fn is_ordinary_session(session: &DbConversationSummary) -> bool {
    matches!(
        session.kind,
        ConversationKind::Regular | ConversationKind::Chat
    ) && session.parent_id.is_none()
}

fn write_fingerprint(action: &str, input: &Value) -> String {
    let encoded = serde_json::to_vec(&(action, input)).unwrap_or_default();
    format!("{:x}", Sha256::digest(encoded))
}

fn read_outcome(request_id: String, action: String, data: Value) -> HostControlUseOutcome {
    HostControlUseOutcome {
        accepted: true,
        request_id,
        action,
        stage: "read".to_string(),
        replayed: false,
        data,
        note: None,
    }
}

fn persisted_outcome(
    request_id: String,
    action: String,
    data: Value,
    note: &str,
) -> HostControlUseOutcome {
    HostControlUseOutcome {
        accepted: true,
        request_id,
        action,
        stage: "persisted".to_string(),
        replayed: false,
        data,
        note: Some(note.to_string()),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionCreateInput {
    name: String,
    #[serde(default)]
    parent_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionRenameInput {
    collection_id: i32,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionMoveInput {
    collection_id: i32,
    #[serde(default)]
    parent_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionSessionInput {
    collection_id: i32,
    session_id: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkbenchCreateInput {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkbenchRenameInput {
    workbench_id: i32,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkbenchSessionInput {
    workbench_id: i32,
    session_id: i32,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkbenchPlacement {
    #[default]
    Tab,
    Right,
    Down,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkbenchPlaceSessionInput {
    workbench_id: i32,
    session_id: i32,
    #[serde(default)]
    placement: Option<WorkbenchPlacement>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::AgentType;
    use std::path::PathBuf;

    async fn fixture() -> (OrganizationHostControl, i32, i32, i32) {
        let db = Arc::new(fresh_in_memory_db().await);
        let root = seed_folder(&db, "/tmp/codeg-host-organization").await;
        let caller = seed_conversation(&db, root, AgentType::Codex).await;
        let target = seed_conversation(&db, root, AgentType::ClaudeCode).await;
        let foreign_folder = seed_folder(&db, "/tmp/codeg-host-organization-foreign").await;
        let foreign = seed_conversation(&db, foreign_folder, AgentType::Gemini).await;
        (
            OrganizationHostControl::new(db, EventEmitter::Noop),
            caller,
            target,
            foreign,
        )
    }

    fn caller(id: i32) -> HostControlCaller {
        HostControlCaller {
            current_session_id: id,
            working_dir: PathBuf::from("/tmp/codeg-host-organization"),
            writes_allowed: true,
        }
    }

    async fn use_action(
        host: &OrganizationHostControl,
        caller_id: i32,
        request_id: &str,
        action: &str,
        input: Value,
    ) -> HostControlUseOutcome {
        host.use_action(
            &caller(caller_id),
            request_id.to_string(),
            action.to_string(),
            input,
        )
        .await
    }

    #[tokio::test]
    async fn collection_actions_complete_a_scoped_nondestructive_round_trip() {
        let (host, caller_id, target_id, foreign_id) = fixture().await;
        let root = use_action(
            &host,
            caller_id,
            "collection-create-root",
            "collection.create",
            json!({ "name": " Research " }),
        )
        .await;
        assert!(root.accepted);
        assert_eq!(root.stage, "persisted");
        let root_id = root.data["collection"]["id"].as_i64().unwrap() as i32;

        let child = use_action(
            &host,
            caller_id,
            "collection-create-child",
            "collection.create",
            json!({ "name": "Sources", "parent_id": root_id }),
        )
        .await;
        let child_id = child.data["collection"]["id"].as_i64().unwrap() as i32;

        let added = use_action(
            &host,
            caller_id,
            "collection-add",
            "collection.add_session",
            json!({ "collection_id": child_id, "session_id": target_id }),
        )
        .await;
        assert!(added.accepted);
        assert_eq!(added.data["changed"], true);

        let listed = use_action(
            &host,
            caller_id,
            "collection-list",
            "collection.list",
            json!({}),
        )
        .await;
        let listed_child = listed.data["collections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["collection_id"] == child_id)
            .unwrap();
        assert_eq!(listed_child["session_ids"], json!([target_id]));

        let renamed = use_action(
            &host,
            caller_id,
            "collection-rename",
            "collection.rename",
            json!({ "collection_id": child_id, "name": "Primary sources" }),
        )
        .await;
        assert_eq!(renamed.data["collection"]["name"], "Primary sources");

        let moved = use_action(
            &host,
            caller_id,
            "collection-move",
            "collection.move",
            json!({ "collection_id": child_id, "parent_id": null }),
        )
        .await;
        assert!(moved.data["collection"]["parent_id"].is_null());

        let removed = use_action(
            &host,
            caller_id,
            "collection-remove",
            "collection.remove_session",
            json!({ "collection_id": child_id, "session_id": target_id }),
        )
        .await;
        assert!(removed.accepted);
        assert!(conversation_service::get_by_id(&host.db.conn, target_id)
            .await
            .is_ok());

        let denied = use_action(
            &host,
            caller_id,
            "collection-foreign",
            "collection.add_session",
            json!({ "collection_id": child_id, "session_id": foreign_id }),
        )
        .await;
        assert!(!denied.accepted);
        assert!(denied.note.unwrap().contains("outside"));
    }

    #[tokio::test]
    async fn collection_writes_replay_by_host_request_id() {
        let (host, caller_id, _, _) = fixture().await;
        let first = use_action(
            &host,
            caller_id,
            "same-request",
            "collection.create",
            json!({ "name": "One" }),
        )
        .await;
        let replay = use_action(
            &host,
            caller_id,
            "same-request",
            "collection.create",
            json!({ "name": "One" }),
        )
        .await;
        assert!(first.accepted && replay.accepted);
        assert!(replay.replayed);
        assert_eq!(first.data, replay.data);

        let collision = use_action(
            &host,
            caller_id,
            "same-request",
            "collection.create",
            json!({ "name": "Two" }),
        )
        .await;
        assert!(!collision.accepted);
        assert!(collision.note.unwrap().contains("different action input"));
    }

    #[tokio::test]
    async fn workbench_membership_is_backend_authoritative_and_does_not_claim_layout() {
        let (host, caller_id, target_id, foreign_id) = fixture().await;
        let created = use_action(
            &host,
            caller_id,
            "workbench-create",
            "workbench.create",
            json!({ "name": "Review" }),
        )
        .await;
        let workbench_id = created.data["workbench"]["id"].as_i64().unwrap() as i32;
        assert_eq!(created.data["mounted"], false);
        assert_eq!(created.data["focused"], false);

        let added = use_action(
            &host,
            caller_id,
            "workbench-add",
            "workbench.add_session",
            json!({ "workbench_id": workbench_id, "session_id": target_id }),
        )
        .await;
        assert!(added.accepted);
        assert_eq!(added.data["layout_changed"], false);
        assert_eq!(added.data["focus_requested"], false);

        let placed = use_action(
            &host,
            caller_id,
            "workbench-place",
            "workbench.place_session",
            json!({
                "workbench_id": workbench_id,
                "session_id": target_id,
                "placement": "right"
            }),
        )
        .await;
        assert!(placed.accepted);
        assert_eq!(placed.stage, "ui_requested");
        assert_eq!(placed.data["membership_changed"], false);
        assert_eq!(placed.data["placement"], "right");
        assert_eq!(placed.data["focus_requested"], true);

        let snapshot = list_workbench_tabs_core(&host.db.conn, workbench_id)
            .await
            .unwrap();
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].conversation_id, Some(target_id));
        assert!(!snapshot.items[0].is_active);

        let listed = use_action(
            &host,
            caller_id,
            "workbench-list",
            "workbench.list",
            json!({}),
        )
        .await;
        assert_eq!(
            listed.data["layout_authority"],
            "device_local_not_available_to_host_control"
        );
        let listed_workbench = listed.data["workbenches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["workbench_id"] == workbench_id)
            .unwrap();
        assert_eq!(listed_workbench["session_ids"], json!([target_id]));

        let foreign = use_action(
            &host,
            caller_id,
            "workbench-add-foreign",
            "workbench.add_session",
            json!({ "workbench_id": workbench_id, "session_id": foreign_id }),
        )
        .await;
        assert!(!foreign.accepted);

        let missing_workbench = use_action(
            &host,
            caller_id,
            "workbench-remove-missing",
            "workbench.remove_session",
            json!({ "workbench_id": workbench_id + 10_000, "session_id": target_id }),
        )
        .await;
        assert!(!missing_workbench.accepted);
        assert!(missing_workbench.note.unwrap().contains("was not found"));

        let removed = use_action(
            &host,
            caller_id,
            "workbench-remove",
            "workbench.remove_session",
            json!({ "workbench_id": workbench_id, "session_id": target_id }),
        )
        .await;
        assert!(removed.accepted);
        assert!(list_workbench_tabs_core(&host.db.conn, workbench_id)
            .await
            .unwrap()
            .items
            .is_empty());
        assert!(conversation_service::get_by_id(&host.db.conn, target_id)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn typed_inputs_reject_model_owned_identity_and_unknown_fields() {
        let (host, caller_id, _, _) = fixture().await;
        let outcome = use_action(
            &host,
            caller_id,
            "unknown-field",
            "collection.create",
            json!({ "name": "X", "from_session_id": caller_id }),
        )
        .await;
        assert!(!outcome.accepted);
        assert!(outcome.note.unwrap().contains("unknown field"));
    }
}
