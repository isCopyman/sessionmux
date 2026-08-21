//! `create_automation` / `create_work_task` backing logic + settings
//! persistence.
//!
//! Two surfaces live here, mirroring `crate::commands::session_info`:
//!
//!   * [`DbChatAuthoring`] — the production [`ChatAuthoringAccess`] impl the
//!     delegation listener calls when a chat agent asks codeg to save an
//!     automation or queue a board task. It resolves the target project, builds
//!     the same drafts the editors build, and writes through
//!     `automation_create_core` / `work_task_create_core` so the lists get their
//!     broadcasts and the work-task pump its nudge.
//!   * The `chat_authoring.*` settings knobs (**default false, both**) — read at
//!     MCP injection time via [`ChatAuthoringRuntimeConfig`] to build
//!     `--features`, and AGAIN at write time by [`DbChatAuthoring`] so turning
//!     the switch off takes effect on sessions that are already running.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};

use crate::acp::chat_authoring::{
    local_timezone, truncate_chars, AuthoringContext, AuthoringOutcome, ChatAuthoringAccess,
    ChatAuthoringConfig, ChatAuthoringRuntimeConfig, ListTasksQuery, NewAutomationSpec,
    NewWorkTaskSpec, ProfileListEntry, ProfileListOutcome, TaskDetailOutcome, TaskEventRow,
    TaskListOutcome, TaskListRow, DEFAULT_TASK_LIST_LIMIT, MAX_TASK_LIST_LIMIT,
    TASK_EVENT_SUMMARY_CHARS, TASK_LAST_ERROR_CHARS, TASK_PROMPT_EXCERPT_CHARS,
    TASK_RECENT_EVENT_LIMIT,
};
use crate::acp::connection::PREFERRED_PROFILE_CONFIG_KEY;
use crate::acp::types::PromptInputBlock;
use crate::app_error::AppCommandError;
use crate::commands::claude_profile::{
    claude_profile_list_core, launch_config_values, profile_destination_summary,
    LAUNCH_MODEL_CONFIG_KEY,
};
use crate::db::entities::automation::{IsolationMode, TriggerKind};
use crate::db::entities::folder::FolderKind;
use crate::db::error::DbError;
use crate::db::service::{
    app_metadata_service, conversation_service, folder_service, work_task_service,
};
use crate::db::AppDatabase;
use crate::models::agent::AgentType;
use crate::models::{
    AutomationConfig, AutomationDraft, FolderDetail, WorkTaskConfig, WorkTaskDraft,
    WorkTaskEventInfo, WorkTaskInfo,
};
use crate::web::event_bridge::{emit_event, EventEmitter, CHAT_AUTHORING_SETTINGS_CHANGED_EVENT};

const KIND_AUTOMATION: &str = "automation";
const KIND_WORK_TASK: &str = "work_task";

/// Production [`ChatAuthoringAccess`]. Holds the DB plus the event emitter (so
/// creates broadcast exactly like a UI-driven create) and the runtime config it
/// re-checks before every write.
pub struct DbChatAuthoring {
    pub db: Arc<AppDatabase>,
    pub emitter: EventEmitter,
    pub config: ChatAuthoringRuntimeConfig,
    /// Data dir for Claude launch profiles (`claude-profiles/`). Same path
    /// `claude_profile_list_core` already uses.
    pub data_dir: PathBuf,
}

impl DbChatAuthoring {
    pub fn new(
        db: Arc<AppDatabase>,
        emitter: EventEmitter,
        config: ChatAuthoringRuntimeConfig,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            db,
            emitter,
            config,
            data_dir,
        }
    }

    /// Resolve the project the new automation / task belongs to.
    ///
    /// Order: an explicit `folder_path` wins; otherwise the caller's own
    /// conversation; otherwise the working directory the companion was launched
    /// with. Whatever matches is then normalized to its project root — a
    /// worktree folder resolves to the project it was cut from, mirroring the
    /// "turn this message into a task" action in the UI.
    async fn resolve_folder(
        &self,
        ctx: &AuthoringContext,
        folder_path: Option<&str>,
    ) -> Result<FolderDetail, String> {
        let conn = &self.db.conn;
        let folders = folder_service::list_all_folder_details(conn)
            .await
            .map_err(|e| format!("could not read the folder list: {e}"))?;

        let found = if let Some(raw) = folder_path {
            match match_folder_by_path(&folders, raw) {
                Some(f) => f,
                None => {
                    // "known to codeg" — the lookup spans every non-deleted
                    // folder row, which includes projects the user has opened
                    // before but currently has closed.
                    return Err(format!(
                        "no project matching '{raw}' is known to codeg. Open the project first, \
                         or omit folder_path to use the one this conversation is in."
                    ));
                }
            }
        } else {
            let from_conversation = match ctx.conversation_id {
                Some(id) => conversation_service::get_by_id(conn, id)
                    .await
                    .ok()
                    .and_then(|c| folders.iter().find(|f| f.id == c.folder_id).cloned()),
                None => None,
            };
            match from_conversation
                .or_else(|| match_folder_by_path(&folders, &ctx.working_dir.to_string_lossy()))
            {
                Some(f) => f,
                None => {
                    return Err(
                        "could not tell which project this conversation belongs to. \
                                Pass folder_path with the absolute path of the target project."
                            .to_string(),
                    );
                }
            }
        };

        // Worktree folders are flattened (a worktree of a worktree still points
        // at the original root), so one hop is always enough.
        let root = match found.parent_id {
            Some(parent_id) => folders
                .iter()
                .find(|f| f.id == parent_id)
                .cloned()
                .unwrap_or(found),
            None => found,
        };
        if root.kind != FolderKind::Regular || root.parent_id.is_some() {
            return Err(format!(
                "'{}' is not a project folder that can hold automations or tasks. \
                 Pass folder_path with the absolute path of the target project.",
                root.path
            ));
        }
        Ok(root)
    }
}

/// Match `raw` against the registered folders: an exact path first, then the
/// longest folder the path lives inside. Component-wise (`Path::starts_with`),
/// so `/repo/app-2` never matches `/repo/app`.
fn match_folder_by_path(folders: &[FolderDetail], raw: &str) -> Option<FolderDetail> {
    let candidate = Path::new(raw.trim());
    if candidate.as_os_str().is_empty() {
        return None;
    }
    folders
        .iter()
        .filter(|f| candidate.starts_with(Path::new(&f.path)))
        // Deepest match wins: a conversation inside a worktree should resolve to
        // that worktree (and then walk up to its project), not straight to some
        // ancestor project that also contains it.
        .max_by_key(|f| Path::new(&f.path).components().count())
        .cloned()
}

/// Validate an agent wire slug (`claude_code`, `custom:<id>`, …). `None` input
/// stays `None` — the caller decides what to fall back to.
fn parse_agent_slug(raw: Option<&str>) -> Result<Option<AgentType>, String> {
    match raw {
        None => Ok(None),
        Some(s) => AgentType::from_wire(s)
            .map(Some)
            .ok_or_else(|| format!("unknown agent_type '{s}'")),
    }
}

/// Profiles currently exist only for Claude Code. `agent` is the resolved
/// launch agent when we have one (automations always do). `None` means the
/// work-task inherit-from-board case — we cannot know the final agent at
/// create time without duplicating `effective_agent_config`, so only an
/// *explicit* non-Claude `agent_type` is rejected.
fn reject_profile_unless_claude(
    profile: Option<&str>,
    agent: Option<&AgentType>,
) -> Result<(), String> {
    let Some(profile) = profile else {
        return Ok(());
    };
    match agent {
        None | Some(AgentType::ClaudeCode) => Ok(()),
        Some(other) => Err(format!(
            "launch profile '{profile}' is only supported for Claude Code \
             (got agent_type '{}'). Omit `profile`, or pass agent_type=claude_code.",
            other.as_wire()
        )),
    }
}

fn clamp_task_list_limit(limit: Option<u32>) -> u64 {
    u64::from(
        limit
            .unwrap_or(DEFAULT_TASK_LIST_LIMIT)
            .clamp(1, MAX_TASK_LIST_LIMIT),
    )
}

fn compact_task_row(row: crate::db::entities::work_task::Model) -> TaskListRow {
    let cfg: WorkTaskConfig = serde_json::from_str(&row.config).unwrap_or_default();
    TaskListRow {
        id: row.id,
        title: row.title,
        status: work_task_service::status_str(row.status).to_string(),
        agent_type: nonempty(cfg.agent_type),
        updated_at: row.updated_at,
        has_worktree: row.worktree_folder_id.is_some(),
        conversation_id: row.conversation_id,
        failure_reason: nonempty(row.failure_reason),
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn compact_event_summary(event: &WorkTaskEventInfo) -> String {
    let Some(payload) = &event.payload else {
        return String::new();
    };
    let text = if event.kind == "status_changed" {
        let from = payload.get("from").and_then(|v| v.as_str()).unwrap_or("?");
        let to = payload.get("to").and_then(|v| v.as_str()).unwrap_or("?");
        format!("{from} → {to}")
    } else if let Some(s) = payload
        .get("message")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        s.to_string()
    } else if let Some(s) = payload
        .get("summary")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        s.to_string()
    } else if let Some(s) = payload
        .get("error")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        s.to_string()
    } else {
        payload.to_string()
    };
    truncate_chars(&text, TASK_EVENT_SUMMARY_CHARS)
}

fn task_detail_from_info(info: WorkTaskInfo, events: Vec<WorkTaskEventInfo>) -> TaskDetailOutcome {
    let cfg: WorkTaskConfig = serde_json::from_value(info.config).unwrap_or_default();
    let profile = cfg
        .config_values
        .get(PREFERRED_PROFILE_CONFIG_KEY)
        .cloned()
        .and_then(|s| nonempty(Some(s)));
    let model = cfg
        .config_values
        .get(LAUNCH_MODEL_CONFIG_KEY)
        .cloned()
        .and_then(|s| nonempty(Some(s)));
    TaskDetailOutcome {
        found: true,
        id: Some(info.id),
        title: Some(info.title),
        status: Some(work_task_service::status_str(info.status).to_string()),
        agent_type: nonempty(cfg.agent_type),
        model,
        profile,
        prompt_excerpt: Some(truncate_chars(&cfg.display_text, TASK_PROMPT_EXCERPT_CHARS)),
        base_branch: nonempty(info.base_branch),
        work_branch: nonempty(info.work_branch),
        has_worktree: Some(info.worktree_folder_id.is_some()),
        conversation_id: info.conversation_id,
        recent_events: events
            .into_iter()
            .map(|event| TaskEventRow {
                kind: event.kind.clone(),
                at: event.created_at,
                summary: compact_event_summary(&event),
            })
            .collect(),
        last_error: info
            .last_error
            .map(|s| truncate_chars(&s, TASK_LAST_ERROR_CHARS))
            .and_then(|s| nonempty(Some(s))),
        note: None,
    }
}

const UNKNOWN_STATUS_NOTE: &str = "unknown status filter. Use a board column \
     (todo, in_progress, attention, done) or a raw status \
     (todo, queued, preparing, running, awaiting_input, review, merging, \
     done, failed, canceled).";

/// Wrap a plain prompt string as the single text block both editors produce for
/// a text-only prompt.
fn text_prompt_blocks(prompt: &str) -> Result<Vec<serde_json::Value>, String> {
    let block = PromptInputBlock::Text {
        text: prompt.to_string(),
    };
    Ok(vec![serde_json::to_value(&block).map_err(|e| {
        format!("could not encode the prompt: {e}")
    })?])
}

#[async_trait]
impl ChatAuthoringAccess for DbChatAuthoring {
    async fn create_automation(
        &self,
        ctx: AuthoringContext,
        spec: NewAutomationSpec,
    ) -> AuthoringOutcome {
        // Re-check at call time, not just at injection: a session launched while
        // the switch was on keeps the tool listed after the user turns it off,
        // and "off" has to stop the write.
        if !self.config.automations_enabled().await {
            return AuthoringOutcome::rejected(
                KIND_AUTOMATION,
                "Creating automations from chat is turned off in codeg's settings \
                 (Settings → General → Create from chat). Ask the user to enable it.",
            );
        }
        let folder = match self.resolve_folder(&ctx, spec.folder_path.as_deref()).await {
            Ok(f) => f,
            Err(note) => return AuthoringOutcome::rejected(KIND_AUTOMATION, note),
        };
        let agent = match parse_agent_slug(spec.agent_type.as_deref()) {
            Ok(a) => a,
            Err(note) => return AuthoringOutcome::rejected(KIND_AUTOMATION, note),
        };
        // An automation always runs as a concrete agent (the fire path parses
        // this slug), so resolve a default rather than storing an empty string:
        // the project's configured agent, else whatever the caller is running as.
        let agent = match agent.or(folder.default_agent_type) {
            Some(a) => a,
            None => {
                let from_caller = match ctx.conversation_id {
                    Some(id) => conversation_service::get_by_id(&self.db.conn, id)
                        .await
                        .ok()
                        .map(|c| c.agent_type),
                    None => None,
                };
                match from_caller {
                    Some(a) => a,
                    None => {
                        return AuthoringOutcome::rejected(
                            KIND_AUTOMATION,
                            "this project has no default agent — pass agent_type \
                             (e.g. 'claude_code').",
                        );
                    }
                }
            }
        };
        let prompt_blocks = match text_prompt_blocks(&spec.prompt) {
            Ok(b) => b,
            Err(note) => return AuthoringOutcome::rejected(KIND_AUTOMATION, note),
        };
        if let Err(note) = reject_profile_unless_claude(spec.profile.as_deref(), Some(&agent)) {
            return AuthoringOutcome::rejected(KIND_AUTOMATION, note);
        }
        let config_values = match launch_config_values(
            &self.data_dir,
            spec.profile.as_deref(),
            spec.model.as_deref(),
        ) {
            Ok(v) => v,
            Err(note) => return AuthoringOutcome::rejected(KIND_AUTOMATION, note),
        };
        let config = AutomationConfig {
            action: spec.action,
            prompt_blocks,
            display_text: spec.prompt.clone(),
            mode_id: None,
            config_values,
            label_snapshot: None,
            // The authoring tool's action allowlist (companion.rs) only admits
            // launch_session / enqueue_task, so no target session exists here.
            target_conversation_id: None,
        };
        let config = match serde_json::to_value(&config) {
            Ok(v) => v,
            Err(e) => {
                return AuthoringOutcome::rejected(
                    KIND_AUTOMATION,
                    format!("could not encode the automation config: {e}"),
                );
            }
        };
        let cron = spec.cron.clone();
        let timezone = spec.timezone.clone().unwrap_or_else(local_timezone);
        let draft = AutomationDraft {
            name: spec.name.clone(),
            enabled: spec.enabled,
            trigger_kind: if cron.is_some() {
                TriggerKind::Schedule
            } else {
                TriggerKind::Manual
            },
            cron: cron.clone(),
            timezone: timezone.clone(),
            agent_type: agent.as_wire().into_owned(),
            root_folder_id: Some(folder.id),
            // A fresh worktree per run is the safe default and the only shape
            // `enqueue_task` accepts; branch / shared-in-root stay an editor-only
            // choice.
            isolation: IsolationMode::WorktreePerRun,
            branch: None,
            is_remote_branch: false,
            config,
        };
        match crate::commands::automation::automation_create_core(&self.emitter, &self.db, draft)
            .await
        {
            Ok(info) => AuthoringOutcome {
                created: true,
                kind: KIND_AUTOMATION.to_string(),
                id: Some(info.id),
                title: Some(info.name),
                folder_name: Some(folder.name),
                folder_path: Some(folder.path),
                agent_type: Some(info.agent_type),
                cron: info.cron,
                timezone: Some(info.timezone),
                next_run_at: info.next_run_at,
                note: (!info.enabled).then(|| {
                    "Saved switched off — the user can enable it on the Automations page."
                        .to_string()
                }),
            },
            // The service's validation errors (bad cron, unknown timezone, empty
            // prompt) are exactly what the LLM should read and retry against, so
            // they come back as a soft note rather than a tool error.
            Err(e) => AuthoringOutcome::rejected(KIND_AUTOMATION, e.to_string()),
        }
    }

    async fn create_work_task(
        &self,
        ctx: AuthoringContext,
        spec: NewWorkTaskSpec,
    ) -> AuthoringOutcome {
        if !self.config.work_tasks_enabled().await {
            return AuthoringOutcome::rejected(
                KIND_WORK_TASK,
                "Creating board tasks from chat is turned off in codeg's settings \
                 (Settings → General → Create from chat). Ask the user to enable it.",
            );
        }
        let folder = match self.resolve_folder(&ctx, spec.folder_path.as_deref()).await {
            Ok(f) => f,
            Err(note) => return AuthoringOutcome::rejected(KIND_WORK_TASK, note),
        };
        let agent = match parse_agent_slug(spec.agent_type.as_deref()) {
            Ok(a) => a,
            Err(note) => return AuthoringOutcome::rejected(KIND_WORK_TASK, note),
        };
        let prompt_blocks = match text_prompt_blocks(&spec.prompt) {
            Ok(b) => b,
            Err(note) => return AuthoringOutcome::rejected(KIND_WORK_TASK, note),
        };
        if let Err(note) = reject_profile_unless_claude(spec.profile.as_deref(), agent.as_ref()) {
            return AuthoringOutcome::rejected(KIND_WORK_TASK, note);
        }
        let config_values = match launch_config_values(
            &self.data_dir,
            spec.profile.as_deref(),
            spec.model.as_deref(),
        ) {
            Ok(v) => v,
            Err(note) => return AuthoringOutcome::rejected(KIND_WORK_TASK, note),
        };
        // `effective_agent_config` uses the task's config_values only when the
        // task has an agent override (otherwise the folder settings map wins
        // wholesale). A profile without agent_type would then be stored and
        // silently ignored at launch — the failure mode this tool must not
        // have. Pin Claude Code so the stored `__codeg_profile__` is applied.
        // `agent_type: None` without a profile still means inherit-the-board.
        let agent_for_store = if spec.profile.is_some() && agent.is_none() {
            Some(AgentType::ClaudeCode)
        } else {
            agent
        };
        let config = WorkTaskConfig {
            prompt_blocks,
            display_text: spec.prompt.clone(),
            agent_type: agent_for_store.map(|a| a.as_wire().into_owned()),
            mode_id: None,
            config_values,
            label_snapshot: None,
        };
        let config = match serde_json::to_value(&config) {
            Ok(v) => v,
            Err(e) => {
                return AuthoringOutcome::rejected(
                    KIND_WORK_TASK,
                    format!("could not encode the task config: {e}"),
                );
            }
        };
        let draft = WorkTaskDraft {
            folder_id: folder.id,
            title: spec.title.clone(),
            config,
        };
        match crate::commands::work_task::work_task_create_core(&self.emitter, &self.db, draft)
            .await
        {
            Ok(info) => AuthoringOutcome {
                created: true,
                kind: KIND_WORK_TASK.to_string(),
                id: Some(info.id),
                title: Some(info.title),
                folder_name: Some(folder.name),
                folder_path: Some(folder.path),
                agent_type: spec.agent_type.clone(),
                note: Some(
                    "Queued as a to-do; the user starts it from there \
                     (or auto-processing picks it up)."
                        .to_string(),
                ),
                ..Default::default()
            },
            Err(e) => AuthoringOutcome::rejected(KIND_WORK_TASK, e.to_string()),
        }
    }

    async fn list_profiles(&self, agent_type: Option<String>) -> ProfileListOutcome {
        let wire = agent_type
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("claude_code");
        match AgentType::from_wire(wire) {
            Some(AgentType::ClaudeCode) => match claude_profile_list_core(&self.data_dir) {
                Ok(list) => ProfileListOutcome {
                    profiles: list
                        .into_iter()
                        .map(|info| ProfileListEntry {
                            id: info.id.clone(),
                            label: info.label.clone(),
                            kind: info.kind,
                            destination: profile_destination_summary(&info),
                        })
                        .collect(),
                    note: None,
                },
                Err(e) => ProfileListOutcome {
                    profiles: Vec::new(),
                    note: Some(format!("could not list Claude launch profiles: {e}")),
                },
            },
            Some(_) => ProfileListOutcome {
                profiles: Vec::new(),
                note: Some(format!(
                    "Launch profiles currently exist only for Claude Code, not '{wire}'."
                )),
            },
            None => ProfileListOutcome {
                profiles: Vec::new(),
                note: Some(format!("unknown agent_type '{wire}'")),
            },
        }
    }

    async fn list_tasks(&self, ctx: AuthoringContext, query: ListTasksQuery) -> TaskListOutcome {
        let statuses = match query
            .status
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            None => None,
            Some(raw) => match work_task_service::board_status_filter(raw) {
                Some(s) => Some(s),
                None => return TaskListOutcome::rejected(UNKNOWN_STATUS_NOTE),
            },
        };
        let folder_id = if query.folder_path.as_deref() == Some("all") {
            None
        } else {
            match self
                .resolve_folder(&ctx, query.folder_path.as_deref())
                .await
            {
                Ok(f) => Some(f.id),
                Err(note) => return TaskListOutcome::rejected(note),
            }
        };
        let limit = clamp_task_list_limit(query.limit);
        match work_task_service::list_matching(&self.db.conn, folder_id, statuses.as_deref(), limit)
            .await
        {
            Ok((rows, total)) => {
                let truncated = total > limit;
                TaskListOutcome {
                    tasks: rows.into_iter().map(compact_task_row).collect(),
                    total,
                    truncated,
                    note: None,
                }
            }
            Err(e) => TaskListOutcome::rejected(format!("could not list tasks: {e}")),
        }
    }

    async fn get_task(&self, task_id: i32) -> TaskDetailOutcome {
        let info = match work_task_service::get(&self.db.conn, task_id).await {
            Ok(info) => info,
            Err(DbError::NotFound(_)) => {
                return TaskDetailOutcome::rejected(format!("no task with id {task_id}"));
            }
            Err(e) => {
                return TaskDetailOutcome::rejected(format!("could not load task {task_id}: {e}"));
            }
        };
        let events =
            match work_task_service::recent_events(&self.db.conn, task_id, TASK_RECENT_EVENT_LIMIT)
                .await
            {
                Ok(events) => events,
                Err(e) => {
                    return TaskDetailOutcome::rejected(format!(
                        "could not load events for task {task_id}: {e}"
                    ));
                }
            };
        task_detail_from_info(info, events)
    }
}

// ===========================================================================
// Settings persistence — `chat_authoring.automations_enabled` /
// `chat_authoring.work_tasks_enabled` (both default OFF). Mirrors
// `crate::commands::session_info`.
// ===========================================================================

pub const KEY_CHAT_AUTHORING_AUTOMATIONS: &str = "chat_authoring.automations_enabled";
pub const KEY_CHAT_AUTHORING_WORK_TASKS: &str = "chat_authoring.work_tasks_enabled";

/// Off by default, unlike the read-only `get_session_info` / `ask_user_question`
/// toggles: these tools write app state and a scheduled automation goes on to
/// spawn agents unattended, so the user opts in.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatAuthoringSettings {
    pub automations_enabled: bool,
    pub work_tasks_enabled: bool,
}

impl ChatAuthoringSettings {
    fn into_runtime_config(self) -> ChatAuthoringConfig {
        ChatAuthoringConfig {
            automations_enabled: self.automations_enabled,
            work_tasks_enabled: self.work_tasks_enabled,
        }
    }
}

async fn load_flag(conn: &DatabaseConnection, key: &str) -> bool {
    match app_metadata_service::get_value(conn, key).await {
        Ok(Some(raw)) => raw.parse::<bool>().unwrap_or(false),
        _ => false,
    }
}

/// Read the persisted keys from `app_metadata`, falling back to the default
/// (both off) for a missing or malformed value. Never errors hard.
pub async fn load_chat_authoring_settings(conn: &DatabaseConnection) -> ChatAuthoringSettings {
    ChatAuthoringSettings {
        automations_enabled: load_flag(conn, KEY_CHAT_AUTHORING_AUTOMATIONS).await,
        work_tasks_enabled: load_flag(conn, KEY_CHAT_AUTHORING_WORK_TASKS).await,
    }
}

/// Pull settings from the DB and push the resulting [`ChatAuthoringConfig`] onto
/// the shared runtime handle. Idempotent — safe on startup or after any save.
pub async fn apply_persisted_chat_authoring_config(
    conn: &DatabaseConnection,
    config: &ChatAuthoringRuntimeConfig,
) {
    let settings = load_chat_authoring_settings(conn).await;
    config.set(settings.into_runtime_config()).await;
}

/// Persist + apply + broadcast. Shared by the Tauri command and the HTTP handler
/// so the write + re-apply + notify chain lives in one place.
pub async fn set_chat_authoring_settings_core(
    conn: &DatabaseConnection,
    config: &ChatAuthoringRuntimeConfig,
    emitter: &EventEmitter,
    desired: ChatAuthoringSettings,
) -> Result<ChatAuthoringSettings, AppCommandError> {
    app_metadata_service::upsert_value(
        conn,
        KEY_CHAT_AUTHORING_AUTOMATIONS,
        &desired.automations_enabled.to_string(),
    )
    .await
    .map_err(AppCommandError::from)?;
    app_metadata_service::upsert_value(
        conn,
        KEY_CHAT_AUTHORING_WORK_TASKS,
        &desired.work_tasks_enabled.to_string(),
    )
    .await
    .map_err(AppCommandError::from)?;
    config.set(desired.clone().into_runtime_config()).await;
    emit_event(emitter, CHAT_AUTHORING_SETTINGS_CHANGED_EVENT, &desired);
    Ok(desired)
}

// -------- Tauri commands -----------------------------------------------------

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn get_chat_authoring_settings(
    #[cfg(feature = "tauri-runtime")] db: tauri::State<'_, crate::db::AppDatabase>,
) -> Result<ChatAuthoringSettings, AppCommandError> {
    #[cfg(feature = "tauri-runtime")]
    {
        Ok(load_chat_authoring_settings(&db.conn).await)
    }
    #[cfg(not(feature = "tauri-runtime"))]
    {
        Err(AppCommandError::configuration_invalid("tauri-only command"))
    }
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn set_chat_authoring_settings(
    #[cfg(feature = "tauri-runtime")] app: tauri::AppHandle,
    #[cfg(feature = "tauri-runtime")] db: tauri::State<'_, crate::db::AppDatabase>,
    #[cfg(feature = "tauri-runtime")] config: tauri::State<'_, ChatAuthoringRuntimeConfig>,
    settings: ChatAuthoringSettings,
) -> Result<ChatAuthoringSettings, AppCommandError> {
    #[cfg(feature = "tauri-runtime")]
    {
        let emitter = EventEmitter::Tauri(app);
        set_chat_authoring_settings_core(&db.conn, &config, &emitter, settings).await
    }
    #[cfg(not(feature = "tauri-runtime"))]
    {
        let _ = settings;
        Err(AppCommandError::configuration_invalid("tauri-only command"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn folder(id: i32, path: &str, parent: Option<i32>, kind: FolderKind) -> FolderDetail {
        FolderDetail {
            id,
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            path: path.to_string(),
            git_branch: None,
            default_agent_type: None,
            last_opened_at: Utc::now(),
            sort_order: 0,
            color: "blue".into(),
            parent_id: parent,
            kind,
            alias: None,
        }
    }

    #[test]
    fn match_folder_by_path_prefers_exact_then_deepest() {
        let folders = vec![
            folder(1, "/repo/app", None, FolderKind::Regular),
            folder(2, "/repo/app-2", None, FolderKind::Regular),
            folder(3, "/repo/app/worktrees/wt", Some(1), FolderKind::Regular),
        ];
        // Exact hit.
        assert_eq!(match_folder_by_path(&folders, "/repo/app").unwrap().id, 1);
        // A sibling whose name merely shares a prefix must NOT match.
        assert_eq!(match_folder_by_path(&folders, "/repo/app-2").unwrap().id, 2);
        // A path inside a folder resolves to the deepest containing folder.
        assert_eq!(
            match_folder_by_path(&folders, "/repo/app/worktrees/wt/src/lib.rs")
                .unwrap()
                .id,
            3
        );
        assert_eq!(
            match_folder_by_path(&folders, "/repo/app/src/lib.rs")
                .unwrap()
                .id,
            1
        );
        // Nothing registered under this path.
        assert!(match_folder_by_path(&folders, "/elsewhere").is_none());
        assert!(match_folder_by_path(&folders, "   ").is_none());
    }

    #[test]
    fn parse_agent_slug_validates() {
        assert_eq!(parse_agent_slug(None).unwrap(), None);
        assert_eq!(
            parse_agent_slug(Some("claude_code")).unwrap(),
            Some(AgentType::ClaudeCode)
        );
        assert!(parse_agent_slug(Some("not_an_agent")).is_err());
    }

    #[test]
    fn text_prompt_blocks_is_one_text_block() {
        let blocks = text_prompt_blocks("do the thing").unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "do the thing");
    }

    #[test]
    fn settings_default_is_both_off() {
        let s = ChatAuthoringSettings::default();
        assert!(!s.automations_enabled);
        assert!(!s.work_tasks_enabled);
    }

    // ── integration: the real write path against an in-memory DB ────────────

    use crate::db::service::{automation_service, work_task_service};
    use crate::db::test_helpers::fresh_in_memory_db;

    /// Wire a `DbChatAuthoring` over a fresh DB with both flags set as given.
    /// The `TempDir` must stay alive for the length of the test — it is the
    /// Claude profiles data dir.
    async fn harness(
        automations: bool,
        work_tasks: bool,
    ) -> (
        Arc<AppDatabase>,
        DbChatAuthoring,
        ChatAuthoringRuntimeConfig,
        tempfile::TempDir,
    ) {
        let db = Arc::new(fresh_in_memory_db().await);
        let config = ChatAuthoringRuntimeConfig::new();
        config
            .set(ChatAuthoringConfig {
                automations_enabled: automations,
                work_tasks_enabled: work_tasks,
            })
            .await;
        let data = tempfile::tempdir().unwrap();
        let access = DbChatAuthoring::new(
            db.clone(),
            EventEmitter::Noop,
            config.clone(),
            data.path().to_path_buf(),
        );
        (db, access, config, data)
    }

    fn ctx_at(dir: &str) -> AuthoringContext {
        AuthoringContext {
            conversation_id: None,
            working_dir: std::path::PathBuf::from(dir),
        }
    }

    fn automation_spec() -> NewAutomationSpec {
        NewAutomationSpec {
            name: "Nightly audit".into(),
            prompt: "audit the dependencies".into(),
            cron: Some("0 3 * * *".into()),
            timezone: Some("UTC".into()),
            action: Default::default(),
            agent_type: Some("claude_code".into()),
            folder_path: None,
            profile: None,
            model: None,
            enabled: true,
        }
    }

    fn work_task_spec() -> NewWorkTaskSpec {
        NewWorkTaskSpec {
            title: "Fix the flake".into(),
            prompt: "the retry test is flaky".into(),
            agent_type: None,
            folder_path: None,
            profile: None,
            model: None,
        }
    }

    /// The headline invariant: with the switch off the tool writes NOTHING, even
    /// though the companion may still be advertising it (it was injected while
    /// the switch was on). The injection-time check alone would let this through.
    #[tokio::test]
    async fn create_automation_refuses_and_writes_nothing_when_disabled() {
        let (db, access, _cfg, _data) = harness(false, true).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();

        let out = access
            .create_automation(ctx_at("/repo/app"), automation_spec())
            .await;

        assert!(!out.created);
        assert_eq!(out.kind, "automation");
        assert!(out.note.unwrap().contains("turned off"));
        assert!(automation_service::list(&db.conn).await.unwrap().is_empty());
    }

    /// …and the two flags gate independently: automations on must not unlock the
    /// board tool.
    #[tokio::test]
    async fn create_work_task_refuses_when_only_automations_enabled() {
        let (db, access, _cfg, _data) = harness(true, false).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();

        let out = access
            .create_work_task(ctx_at("/repo/app"), work_task_spec())
            .await;

        assert!(!out.created);
        assert!(out.note.unwrap().contains("turned off"));
        assert!(work_task_service::list(&db.conn, None)
            .await
            .unwrap()
            .is_empty());
    }

    /// Flipping the shared runtime handle takes effect on the NEXT call — no
    /// reconstruction of the access impl needed. This is what makes the call-time
    /// check meaningful for an already-running session.
    #[tokio::test]
    async fn flipping_the_flag_takes_effect_on_the_next_call() {
        let (db, access, cfg, _data) = harness(false, false).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();

        assert!(
            !access
                .create_automation(ctx_at("/repo/app"), automation_spec())
                .await
                .created
        );
        cfg.set(ChatAuthoringConfig {
            automations_enabled: true,
            work_tasks_enabled: false,
        })
        .await;
        let out = access
            .create_automation(ctx_at("/repo/app"), automation_spec())
            .await;
        assert!(out.created, "note: {:?}", out.note);
        assert_eq!(automation_service::list(&db.conn).await.unwrap().len(), 1);
    }

    /// A successful create lands a real row with the shape the fire path expects,
    /// and reports back the schedule the scheduler actually computed.
    #[tokio::test]
    async fn create_automation_persists_a_fireable_row() {
        let (db, access, _cfg, _data) = harness(true, false).await;
        let folder = folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();

        let out = access
            .create_automation(ctx_at("/repo/app"), automation_spec())
            .await;
        assert!(out.created, "note: {:?}", out.note);
        assert_eq!(out.folder_path.as_deref(), Some("/repo/app"));
        assert!(
            out.next_run_at.is_some(),
            "a scheduled automation has a next run"
        );

        let row = automation_service::get(&db.conn, out.id.unwrap())
            .await
            .unwrap();
        assert_eq!(row.root_folder_id, Some(folder.id));
        assert_eq!(row.agent_type, "claude_code");
        assert_eq!(row.cron.as_deref(), Some("0 3 * * *"));
        assert_eq!(row.isolation, IsolationMode::WorktreePerRun);
        assert!(row.branch.is_none());
        // The fire path parses this slug and replays these blocks verbatim.
        let cfg: AutomationConfig = serde_json::from_value(row.config).unwrap();
        assert_eq!(cfg.prompt_blocks.len(), 1);
        assert_eq!(cfg.prompt_blocks[0]["text"], "audit the dependencies");
        assert_eq!(cfg.display_text, "audit the dependencies");
        // Omitted profile/model must keep today's empty map, byte-for-byte.
        assert!(cfg.config_values.is_empty());
        assert_eq!(
            serde_json::to_value(&cfg.config_values).unwrap(),
            serde_json::json!({})
        );
    }

    /// A chat running inside a worktree targets the PROJECT, not the worktree —
    /// the board and the automations list are both project-scoped.
    #[tokio::test]
    async fn create_work_task_resolves_a_worktree_to_its_project() {
        let (db, access, _cfg, _data) = harness(false, true).await;
        let root = folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();
        folder_service::add_folder_with_parent(&db.conn, "/repo/app-task-1", Some(root.id))
            .await
            .unwrap();

        // Deep inside the worktree, so the deepest-match + parent-hop both run.
        let out = access
            .create_work_task(ctx_at("/repo/app-task-1/src"), work_task_spec())
            .await;

        assert!(out.created, "note: {:?}", out.note);
        let row = work_task_service::get(&db.conn, out.id.unwrap())
            .await
            .unwrap();
        assert_eq!(row.folder_id, root.id, "task belongs to the project root");
        // No agent override — the task inherits the board's configured default.
        let cfg: WorkTaskConfig = serde_json::from_value(row.config).unwrap();
        assert!(cfg.agent_type.is_none());
        assert_eq!(cfg.prompt_blocks.len(), 1);
        assert!(cfg.config_values.is_empty());
        assert_eq!(
            serde_json::to_value(&cfg.config_values).unwrap(),
            serde_json::json!({})
        );
    }

    /// An unresolvable target is a soft refusal telling the LLM what to pass —
    /// never a silent write to some arbitrary folder.
    #[tokio::test]
    async fn unresolvable_folder_is_a_soft_refusal() {
        let (db, access, _cfg, _data) = harness(true, true).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();

        // Working dir outside every registered folder.
        let out = access
            .create_automation(ctx_at("/somewhere/else"), automation_spec())
            .await;
        assert!(!out.created);
        assert!(out.note.unwrap().contains("folder_path"));

        // An explicit path that is not a registered folder.
        let mut spec = work_task_spec();
        spec.folder_path = Some("/not/registered".into());
        let out = access.create_work_task(ctx_at("/repo/app"), spec).await;
        assert!(!out.created);
        assert!(out.note.unwrap().contains("/not/registered"));
        assert!(work_task_service::list(&db.conn, None)
            .await
            .unwrap()
            .is_empty());
    }

    /// Settings persist and re-apply onto the shared runtime handle, so a save in
    /// one transport is visible to MCP injection and to the write path.
    #[tokio::test]
    async fn settings_persist_and_reapply_to_the_runtime_handle() {
        let db = fresh_in_memory_db().await;
        let config = ChatAuthoringRuntimeConfig::new();

        // Nothing stored yet → both off.
        apply_persisted_chat_authoring_config(&db.conn, &config).await;
        assert_eq!(config.snapshot().await, ChatAuthoringConfig::default());

        set_chat_authoring_settings_core(
            &db.conn,
            &config,
            &EventEmitter::Noop,
            ChatAuthoringSettings {
                automations_enabled: true,
                work_tasks_enabled: false,
            },
        )
        .await
        .unwrap();
        assert!(config.automations_enabled().await);
        assert!(!config.work_tasks_enabled().await);

        // Round-trips through the DB (what a fresh boot reads).
        let loaded = load_chat_authoring_settings(&db.conn).await;
        assert!(loaded.automations_enabled);
        assert!(!loaded.work_tasks_enabled);
        let fresh = ChatAuthoringRuntimeConfig::new();
        apply_persisted_chat_authoring_config(&db.conn, &fresh).await;
        assert!(fresh.automations_enabled().await);
        assert!(!fresh.work_tasks_enabled().await);
    }

    use crate::acp::connection::PREFERRED_PROFILE_CONFIG_KEY;
    use crate::commands::claude_profile::{claude_profile_upsert_core, LAUNCH_MODEL_CONFIG_KEY};
    use crate::models::claude_profile::{ClaudeProfileKind, ClaudeProfileUpsert};

    fn seed_managed_profile(data_dir: &Path, id: &str, token: &str) {
        claude_profile_upsert_core(
            data_dir,
            ClaudeProfileUpsert {
                id: id.to_string(),
                label: format!("Managed {id}"),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: Some("https://relay.example/v1".into()),
                auth_token: Some(token.to_string()),
                model: None,
                settings_json: None,
                env: None,
                expect_new: false,
            },
        )
        .expect("upsert managed profile");
    }

    #[tokio::test]
    async fn work_task_profile_without_agent_type_pins_claude_code() {
        let (db, access, _cfg, data) = harness(false, true).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();
        seed_managed_profile(data.path(), "api", "sk-super-secret-token-xyz");

        let mut spec = work_task_spec();
        spec.profile = Some("api".into());
        let out = access.create_work_task(ctx_at("/repo/app"), spec).await;
        assert!(out.created, "note: {:?}", out.note);

        let row = work_task_service::get(&db.conn, out.id.unwrap())
            .await
            .unwrap();
        let cfg: WorkTaskConfig = serde_json::from_value(row.config).unwrap();
        assert_eq!(cfg.agent_type.as_deref(), Some("claude_code"));
        assert_eq!(
            cfg.config_values
                .get(PREFERRED_PROFILE_CONFIG_KEY)
                .map(String::as_str),
            Some("api")
        );
    }

    #[tokio::test]
    async fn work_task_profile_and_model_land_in_config_values() {
        let (db, access, _cfg, data) = harness(false, true).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();
        seed_managed_profile(data.path(), "api", "sk-super-secret-token-xyz");

        let mut spec = work_task_spec();
        spec.profile = Some("api".into());
        spec.model = Some("claude-opus-4".into());
        spec.agent_type = Some("claude_code".into());
        let out = access.create_work_task(ctx_at("/repo/app"), spec).await;
        assert!(out.created, "note: {:?}", out.note);

        let row = work_task_service::get(&db.conn, out.id.unwrap())
            .await
            .unwrap();
        let cfg: WorkTaskConfig = serde_json::from_value(row.config).unwrap();
        assert_eq!(
            cfg.config_values
                .get(PREFERRED_PROFILE_CONFIG_KEY)
                .map(String::as_str),
            Some("api")
        );
        assert_eq!(
            cfg.config_values
                .get(LAUNCH_MODEL_CONFIG_KEY)
                .map(String::as_str),
            Some("claude-opus-4")
        );
    }

    #[tokio::test]
    async fn unknown_work_task_profile_is_rejected_and_writes_nothing() {
        let (db, access, _cfg, data) = harness(false, true).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();
        seed_managed_profile(data.path(), "api", "sk-super-secret-token-xyz");

        let mut spec = work_task_spec();
        spec.profile = Some("does-not-exist".into());
        spec.model = Some("claude-opus-4".into());
        let out = access.create_work_task(ctx_at("/repo/app"), spec).await;
        assert!(!out.created);
        let note = out.note.unwrap();
        assert!(note.contains("does-not-exist"), "{note}");
        assert!(note.contains("follow-default"), "{note}");
        assert!(note.contains("api"), "{note}");
        assert!(work_task_service::list(&db.conn, None)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn work_task_profile_rejected_for_explicit_non_claude_agent() {
        let (db, access, _cfg, data) = harness(false, true).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();
        seed_managed_profile(data.path(), "api", "sk-super-secret-token-xyz");

        let mut spec = work_task_spec();
        spec.profile = Some("api".into());
        spec.agent_type = Some("codex".into());
        let out = access.create_work_task(ctx_at("/repo/app"), spec).await;
        assert!(!out.created);
        let note = out.note.unwrap();
        assert!(note.contains("Claude Code"), "{note}");
        assert!(note.contains("codex"), "{note}");
        assert!(work_task_service::list(&db.conn, None)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn automation_launch_session_profile_lands_in_config_values() {
        let (db, access, _cfg, data) = harness(true, false).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();
        seed_managed_profile(data.path(), "api", "sk-super-secret-token-xyz");

        let mut spec = automation_spec();
        spec.profile = Some("api".into());
        spec.model = Some("claude-sonnet-4".into());
        let out = access.create_automation(ctx_at("/repo/app"), spec).await;
        assert!(out.created, "note: {:?}", out.note);

        let row = automation_service::get(&db.conn, out.id.unwrap())
            .await
            .unwrap();
        let cfg: AutomationConfig = serde_json::from_value(row.config).unwrap();
        assert_eq!(cfg.action, crate::models::AutomationAction::LaunchSession);
        assert_eq!(
            cfg.config_values
                .get(PREFERRED_PROFILE_CONFIG_KEY)
                .map(String::as_str),
            Some("api")
        );
        assert_eq!(
            cfg.config_values
                .get(LAUNCH_MODEL_CONFIG_KEY)
                .map(String::as_str),
            Some("claude-sonnet-4")
        );
    }

    #[tokio::test]
    async fn unknown_automation_profile_is_rejected_and_writes_nothing() {
        let (db, access, _cfg, _data) = harness(true, false).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();

        let mut spec = automation_spec();
        spec.profile = Some("ghost".into());
        let out = access.create_automation(ctx_at("/repo/app"), spec).await;
        assert!(!out.created);
        let note = out.note.unwrap();
        assert!(note.contains("ghost"), "{note}");
        assert!(note.contains("follow-default"), "{note}");
        assert!(automation_service::list(&db.conn).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_profiles_returns_ids_without_tokens() {
        let (_db, access, _cfg, data) = harness(true, true).await;
        let secret = "sk-super-secret-token-xyz-do-not-leak";
        seed_managed_profile(data.path(), "api", secret);

        let out = access.list_profiles(None).await;
        assert!(out.note.is_none(), "note: {:?}", out.note);
        let dumped = serde_json::to_string(&out).unwrap();
        assert!(!dumped.contains(secret), "{dumped}");
        assert!(!dumped.contains("authToken"), "{dumped}");
        assert!(!dumped.contains("auth_token"), "{dumped}");
        assert!(!dumped.contains("authTokenMasked"), "{dumped}");
        assert!(out.profiles.iter().any(|p| p.id == "follow-default"));
        let api = out.profiles.iter().find(|p| p.id == "api").unwrap();
        assert!(api.destination.contains("https://relay.example/v1"));
        assert!(!api.destination.contains(secret));

        let other = access.list_profiles(Some("codex".into())).await;
        assert!(other.profiles.is_empty());
        assert!(other.note.unwrap().contains("Claude Code"));
    }

    async fn stamp_status(
        conn: &sea_orm::DatabaseConnection,
        id: i32,
        status: crate::db::entities::work_task::WorkTaskStatus,
    ) {
        use crate::db::entities::work_task;
        use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set};
        let row = work_task::Entity::find_by_id(id)
            .one(conn)
            .await
            .unwrap()
            .unwrap();
        let mut am = row.into_active_model();
        am.status = Set(status);
        am.updated_at = Set(chrono::Utc::now());
        am.update(conn).await.unwrap();
    }

    #[tokio::test]
    async fn list_tasks_defaults_to_the_caller_project_and_hops_off_a_worktree() {
        let (db, access, _cfg, _data) = harness(false, true).await;
        let root = folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();
        folder_service::add_folder_with_parent(&db.conn, "/repo/app-task-1", Some(root.id))
            .await
            .unwrap();
        folder_service::add_folder(&db.conn, "/repo/other")
            .await
            .unwrap();
        assert!(
            access
                .create_work_task(ctx_at("/repo/app"), work_task_spec())
                .await
                .created
        );
        let mut other_spec = work_task_spec();
        other_spec.title = "Other project".into();
        other_spec.folder_path = Some("/repo/other".into());
        assert!(
            access
                .create_work_task(ctx_at("/repo/other"), other_spec)
                .await
                .created
        );
        assert_eq!(
            work_task_service::list(&db.conn, None).await.unwrap().len(),
            2
        );

        let out = access
            .list_tasks(ctx_at("/repo/app-task-1/src"), ListTasksQuery::default())
            .await;
        assert!(out.note.is_none(), "note: {:?}", out.note);
        assert_eq!(out.total, 1);
        assert!(!out.truncated);
        assert_eq!(out.tasks.len(), 1);
        assert_eq!(out.tasks[0].title, "Fix the flake");
        assert_eq!(out.tasks[0].status, "todo");
        let dumped = serde_json::to_value(&out).unwrap();
        let row = &dumped["tasks"][0];
        assert!(row.get("prompt").is_none(), "{row}");
        assert!(row.get("display_text").is_none(), "{row}");
        assert!(row.get("prompt_blocks").is_none(), "{row}");
        assert!(row.get("last_error").is_none(), "{row}");
        assert!(row.get("config").is_none(), "{row}");
        assert!(row.get("config_values").is_none(), "{row}");
    }

    #[tokio::test]
    async fn list_tasks_folder_path_all_returns_every_project_newest_first() {
        let (db, access, _cfg, _data) = harness(false, true).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();
        folder_service::add_folder(&db.conn, "/repo/other")
            .await
            .unwrap();
        let mut first = work_task_spec();
        first.title = "Older".into();
        access.create_work_task(ctx_at("/repo/app"), first).await;
        let mut second = work_task_spec();
        second.title = "Newer".into();
        second.folder_path = Some("/repo/other".into());
        access.create_work_task(ctx_at("/repo/other"), second).await;

        let out = access
            .list_tasks(
                ctx_at("/repo/app"),
                ListTasksQuery {
                    folder_path: Some("all".into()),
                    ..Default::default()
                },
            )
            .await;
        assert_eq!(out.total, 2);
        assert_eq!(out.tasks.len(), 2);
        assert_eq!(out.tasks[0].title, "Newer");
        assert_eq!(out.tasks[1].title, "Older");
    }

    #[tokio::test]
    async fn list_tasks_attention_filter_matches_frontend_column() {
        let (db, access, _cfg, _data) = harness(false, true).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();
        use crate::db::entities::work_task::WorkTaskStatus;
        let mut ids = Vec::new();
        for title in [
            "todo",
            "queued",
            "preparing",
            "running",
            "awaiting_input",
            "review",
            "merging",
            "failed",
            "done",
            "canceled",
        ] {
            let mut spec = work_task_spec();
            spec.title = title.into();
            let out = access.create_work_task(ctx_at("/repo/app"), spec).await;
            ids.push((title, out.id.unwrap()));
        }
        let statuses = [
            WorkTaskStatus::Todo,
            WorkTaskStatus::Queued,
            WorkTaskStatus::Preparing,
            WorkTaskStatus::Running,
            WorkTaskStatus::AwaitingInput,
            WorkTaskStatus::Review,
            WorkTaskStatus::Merging,
            WorkTaskStatus::Failed,
            WorkTaskStatus::Done,
            WorkTaskStatus::Canceled,
        ];
        for ((_, id), status) in ids.iter().zip(statuses) {
            stamp_status(&db.conn, *id, status).await;
        }

        // Pin: src/components/tasks/board-columns.ts STATUSES_BY_COLUMN.attention
        // = awaiting_input, review, merging, failed.
        let out = access
            .list_tasks(
                ctx_at("/repo/app"),
                ListTasksQuery {
                    status: Some("attention".into()),
                    ..Default::default()
                },
            )
            .await;
        let mut got: Vec<&str> = out.tasks.iter().map(|t| t.status.as_str()).collect();
        got.sort_unstable();
        assert_eq!(
            got,
            vec!["awaiting_input", "failed", "merging", "review"],
            "attention column must match board-columns.ts"
        );
        assert_eq!(out.total, 4);

        let raw = access
            .list_tasks(
                ctx_at("/repo/app"),
                ListTasksQuery {
                    status: Some("awaiting_input".into()),
                    ..Default::default()
                },
            )
            .await;
        assert_eq!(raw.total, 1);
        assert_eq!(raw.tasks[0].status, "awaiting_input");
    }

    #[tokio::test]
    async fn list_tasks_clamps_limit_and_sets_truncated() {
        let (db, access, _cfg, _data) = harness(false, true).await;
        folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();
        for i in 0..5 {
            let mut spec = work_task_spec();
            spec.title = format!("card {i}");
            assert!(
                access
                    .create_work_task(ctx_at("/repo/app"), spec)
                    .await
                    .created
            );
        }
        let page = access
            .list_tasks(
                ctx_at("/repo/app"),
                ListTasksQuery {
                    limit: Some(2),
                    ..Default::default()
                },
            )
            .await;
        assert_eq!(page.total, 5);
        assert_eq!(page.tasks.len(), 2);
        assert!(page.truncated);

        assert_eq!(clamp_task_list_limit(None), 30);
        assert_eq!(clamp_task_list_limit(Some(150)), 100);
        assert_eq!(clamp_task_list_limit(Some(0)), 1);

        for i in 5..101 {
            work_task_service::create(
                &db.conn,
                WorkTaskDraft {
                    folder_id: work_task_service::list(&db.conn, None).await.unwrap()[0].folder_id,
                    title: format!("extra {i}"),
                    config: serde_json::json!({
                        "display_text": "x",
                        "prompt_blocks": [{ "type": "text", "text": "x" }],
                    }),
                },
            )
            .await
            .unwrap();
        }
        let capped = access
            .list_tasks(
                ctx_at("/repo/app"),
                ListTasksQuery {
                    limit: Some(150),
                    ..Default::default()
                },
            )
            .await;
        assert_eq!(capped.total, 101);
        assert_eq!(capped.tasks.len(), 100);
        assert!(capped.truncated);
    }

    #[tokio::test]
    async fn get_task_truncates_and_omits_profile_secrets() {
        let (db, access, _cfg, _data) = harness(false, true).await;
        let folder = folder_service::add_folder(&db.conn, "/repo/app")
            .await
            .unwrap();
        let secret = "sk-super-secret-token-xyz-do-not-leak";
        let long_prompt: String = "你".repeat(TASK_PROMPT_EXCERPT_CHARS + 8);
        let long_error: String = "E".repeat(TASK_LAST_ERROR_CHARS + 20);
        let long_event: String = "M".repeat(TASK_EVENT_SUMMARY_CHARS + 20);
        let created = work_task_service::create(
            &db.conn,
            WorkTaskDraft {
                folder_id: folder.id,
                title: "Secret card".into(),
                config: serde_json::json!({
                    "display_text": long_prompt,
                    "prompt_blocks": [{ "type": "text", "text": long_prompt }],
                    "agent_type": "claude_code",
                    "config_values": {
                        "__codeg_profile__": "api",
                        "model": "claude-opus-4",
                        "authToken": secret,
                        "baseUrl": "https://evil.example/v1",
                    }
                }),
            },
        )
        .await
        .unwrap();
        work_task_service::fail(
            &db.conn,
            created.id,
            &[crate::db::entities::work_task::WorkTaskStatus::Todo],
            None,
            "agent_error",
            Some(long_error.clone()),
        )
        .await
        .unwrap();
        for i in 0..12 {
            work_task_service::record_event(
                &db.conn,
                created.id,
                "agent_progress",
                "agent",
                Some(serde_json::json!({ "message": format!("{long_event}-{i}") })),
            )
            .await
            .unwrap();
        }

        let out = access.get_task(created.id).await;
        assert!(out.found, "note: {:?}", out.note);
        assert_eq!(out.profile.as_deref(), Some("api"));
        assert_eq!(out.model.as_deref(), Some("claude-opus-4"));
        assert_eq!(out.agent_type.as_deref(), Some("claude_code"));
        let excerpt = out.prompt_excerpt.as_ref().unwrap();
        assert_eq!(excerpt.chars().count(), TASK_PROMPT_EXCERPT_CHARS);
        let last_error = out.last_error.as_ref().unwrap();
        assert_eq!(last_error.chars().count(), TASK_LAST_ERROR_CHARS);
        assert_eq!(out.recent_events.len(), TASK_RECENT_EVENT_LIMIT as usize);
        for event in &out.recent_events {
            assert!(event.summary.chars().count() <= TASK_EVENT_SUMMARY_CHARS);
        }
        let dumped = serde_json::to_string(&out).unwrap();
        assert!(!dumped.contains(secret), "{dumped}");
        assert!(!dumped.contains("authToken"), "{dumped}");
        assert!(!dumped.contains("baseUrl"), "{dumped}");
        assert!(!dumped.contains("https://evil.example"), "{dumped}");
        assert!(!dumped.contains("prompt_blocks"), "{dumped}");
        assert!(!access.get_task(999_999).await.found);
    }
}
