//! Chat-authoring domain types — backing the `create_automation` and
//! `create_work_task` MCP tools.
//!
//! These are the first codeg-mcp tools that *write* app state: an agent talking
//! to the user in an ordinary chat can park recurring work as an automation, or
//! queue a task on the work-task board, without the user leaving the
//! conversation to fill in a form.
//!
//! This module holds the layer-shared pieces (mirroring
//! [`crate::acp::session_info`]):
//!   * [`NewAutomationSpec`] / [`NewWorkTaskSpec`] — the validated request the
//!     companion parsed out of the tool arguments.
//!   * [`AuthoringContext`] — who is asking (resolved by the listener from the
//!     per-launch token) so the target folder can default to the caller's own
//!     session.
//!   * [`AuthoringOutcome`] — the self-describing result delivered back over the
//!     broker socket, rendered by the companion without re-querying.
//!   * [`ChatAuthoringAccess`] — the listener-facing trait the production
//!     `DbChatAuthoring` (in `crate::commands::chat_authoring`) implements.
//!   * [`ChatAuthoringRuntimeConfig`] — the hot-swappable "is the feature on?"
//!     pair of flags, read BOTH at MCP injection time and again at call time
//!     (see the note on that type).

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::models::claude_profile::ClaudeProfileKind;
use crate::models::work_task::{WorkTaskBusinessStatus, WorkTaskPriority};
use crate::models::AutomationAction;

/// Cap on an automation name / task title. Long enough for a descriptive
/// sentence, short enough that a board card and the automations list stay
/// readable. Over-long input is truncated, never rejected — the LLM's intent is
/// still honored.
pub const MAX_TITLE_CHARS: usize = 120;

/// Cap on the stored prompt body. Generous (a full task briefing is welcome)
/// but bounded so a runaway generation can't push a multi-megabyte blob into
/// the config JSON.
pub const MAX_PROMPT_CHARS: usize = 20_000;

/// Who is calling, resolved by the listener from the per-launch token. Both
/// fields are hints for defaulting the target folder — an explicit
/// `folder_path` on the spec wins over either.
#[derive(Debug, Clone)]
pub struct AuthoringContext {
    /// The caller's current conversation (via `ParentSessionLookup`). `None`
    /// when the parent connection has no conversation yet.
    pub conversation_id: Option<i32>,
    /// The working directory the companion was launched with.
    pub working_dir: PathBuf,
}

/// A validated `create_automation` request. The companion has already checked
/// the required strings are non-empty; ranges/enums are re-checked by the
/// automation service at save.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAutomationSpec {
    pub name: String,
    pub prompt: String,
    /// 5- or 6-field cron. `None` → a manual-trigger automation (run from the
    /// Automations page on demand).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    /// IANA zone name. `None` → the host's detected zone (see
    /// [`local_timezone`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// What firing does: start a headless session, or queue a board task.
    #[serde(default)]
    pub action: AutomationAction,
    /// Agent wire slug. `None` → the target folder's default agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// Absolute path of the target project. `None` → the caller's own folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_path: Option<String>,
    /// Claude launch profile id. Written into `config_values["__codeg_profile__"]`
    /// so a `launch_session` fire (and `enqueue_task`) reuse the existing
    /// preferred-config path. `None` keeps today's empty `config_values`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Model id. Written into the existing `config_values["model"]` key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub enabled: bool,
}

/// A validated `create_work_task` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWorkTaskSpec {
    pub title: String,
    pub prompt: String,
    /// Where the neutral card first appears. `None` preserves the historical
    /// Todo default; may target any of the seven fixed board states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_status: Option<WorkTaskBusinessStatus>,
    /// Business importance only; does not alter execution or interrupt order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<WorkTaskPriority>,
    /// Per-task agent override. `None` → inherit the board's settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// Absolute path of the target project. `None` → the caller's own folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_path: Option<String>,
    /// Claude launch profile id. Written into `config_values["__codeg_profile__"]`.
    /// `None` keeps today's empty `config_values`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Model id. Written into the existing `config_values["model"]` key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// The outcome handed back to the tool. A refusal (`created: false`) is a SOFT
/// result carrying a `note` the LLM reads and can act on — never a tool error,
/// so a disabled feature or an unresolvable folder doesn't derail the turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthoringOutcome {
    pub created: bool,
    /// What was created: `"automation"` or `"work_task"`. Always set, so the
    /// companion can phrase its text without knowing which arm it rendered.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// The stored cron, when the automation is scheduled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// First fire time, computed by the same evaluator the scheduler uses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<DateTime<Utc>>,
    /// Why it was refused, or an advisory on a successful create.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One Claude launch profile as shown to an LLM. Tokens (plain or masked) are
/// never included — a profile decides which Claude configuration a session
/// launches with, not which credential to copy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileListEntry {
    pub id: String,
    pub label: String,
    pub kind: ClaudeProfileKind,
    pub destination: String,
}

/// Outcome of `list_profiles`. An empty `profiles` plus a `note` is how a
/// non-Claude `agent_type` or a disk error is reported — never a token leak.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileListOutcome {
    pub profiles: Vec<ProfileListEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Default / hard cap for `list_tasks`. Over the cap is clamped, never rejected.
pub const DEFAULT_TASK_LIST_LIMIT: u32 = 30;
pub const MAX_TASK_LIST_LIMIT: u32 = 100;

/// Character caps for `get_task`. Truncation is character-safe (`truncate_chars`).
pub const TASK_PROMPT_EXCERPT_CHARS: usize = 1000;
pub const TASK_EVENT_SUMMARY_CHARS: usize = 200;
pub const TASK_LAST_ERROR_CHARS: usize = 500;
pub const TASK_RECENT_EVENT_LIMIT: u64 = 10;

/// Validated `list_tasks` arguments. Every field is optional; the host fills
/// defaults (caller's project, no status filter, limit 30).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListTasksQuery {
    /// Absolute project path, `"all"` for every live project, or `None` to use
    /// the caller's own project (worktree cwd hops to the project root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_path: Option<String>,
    /// Board column (`backlog` / `todo` / `in_progress` / `review` / `done` /
    /// `blocked` / `canceled`) or a raw
    /// `WorkTaskStatus` value (`awaiting_input`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One compact board row for `list_tasks`. Deliberately omits prompt body,
/// display_text, timeline, and error stacks — those blow up an LLM context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskListRow {
    pub id: i32,
    pub title: String,
    /// Six-state user workflow (`todo` / `in_progress` / `blocked` / `review`
    /// / `done` / `canceled`).
    pub task_status: String,
    /// Business importance only. It never changes dispatcher order by itself.
    pub priority: crate::models::work_task::WorkTaskPriority,
    /// Lower-level execution lifecycle retained for diagnostics.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub has_worktree: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

/// Outcome of `list_tasks`. `total` is the filtered count before `limit`;
/// `truncated` is true when the LLM is only seeing a prefix.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskListOutcome {
    pub tasks: Vec<TaskListRow>,
    pub total: u64,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One timeline event as shown by `get_task`. `summary` is already truncated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEventRow {
    pub kind: String,
    pub at: DateTime<Utc>,
    pub summary: String,
}

/// Outcome of `get_task`. Absent optional fields are omitted (never a token,
/// baseUrl, or the untruncated prompt). `found: false` is a soft miss.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskDetailOutcome {
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<crate::models::work_task::WorkTaskPriority>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_worktree: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_events: Vec<TaskEventRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl TaskListOutcome {
    pub fn rejected(note: impl Into<String>) -> Self {
        Self {
            note: Some(note.into()),
            ..Default::default()
        }
    }
}

impl TaskDetailOutcome {
    pub fn rejected(note: impl Into<String>) -> Self {
        Self {
            found: false,
            note: Some(note.into()),
            ..Default::default()
        }
    }
}

impl AuthoringOutcome {
    /// A soft refusal: nothing was created, and `note` explains why in terms the
    /// LLM can act on (turn the setting on, pass `folder_path`, fix the cron…).
    pub fn rejected(kind: &str, note: impl Into<String>) -> Self {
        Self {
            created: false,
            kind: kind.to_string(),
            note: Some(note.into()),
            ..Default::default()
        }
    }
}

/// Listener-facing access for the two authoring tools. The production impl
/// (`crate::commands::chat_authoring::DbChatAuthoring`) re-checks the feature
/// flags, resolves the target folder, and writes through the same `*_create_core`
/// helpers the UI uses (so the board / automations list get their broadcasts and
/// the work-task pump its nudge). Kept as a trait so the listener stays
/// decoupled from the DB and tests can stub it. Mirrors
/// [`crate::acp::work_task_tools::WorkTaskToolAccess`].
#[async_trait]
pub trait ChatAuthoringAccess: Send + Sync {
    async fn create_automation(
        &self,
        ctx: AuthoringContext,
        spec: NewAutomationSpec,
    ) -> AuthoringOutcome;

    async fn create_work_task(
        &self,
        ctx: AuthoringContext,
        spec: NewWorkTaskSpec,
    ) -> AuthoringOutcome;

    /// List Claude launch profiles for `list_profiles`. `agent_type` defaults
    /// to Claude Code; other agents currently have no profiles.
    async fn list_profiles(&self, agent_type: Option<String>) -> ProfileListOutcome;

    /// Read-only board listing for `list_tasks`. Must not write any row.
    async fn list_tasks(&self, ctx: AuthoringContext, query: ListTasksQuery) -> TaskListOutcome;

    /// Read-only card detail for `get_task`. Must not write any row.
    async fn get_task(&self, task_id: i32) -> TaskDetailOutcome;
}

/// The two independently-toggled feature flags. Both default OFF: unlike the
/// read-only `get_session_info` / `ask_user_question` tools, these WRITE app
/// state and a scheduled automation goes on to spawn agents on its own, so the
/// user opts in explicitly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatAuthoringConfig {
    pub automations_enabled: bool,
    pub work_tasks_enabled: bool,
}

/// Shared, hot-swappable handle to [`ChatAuthoringConfig`]. Cloned into
/// `DelegationInjection` (read at injection, to build `--features`) and into
/// `AppState` (updated on save).
///
/// It is ALSO read again at call time by the production access impl. The
/// read-only tools get away with an injection-time-only check — their tools stay
/// listed for the life of an already-running session after the user flips the
/// setting off. For a tool that creates scheduled background work, "off" has to
/// mean off right now, so the write path re-reads this handle.
#[derive(Clone, Default)]
pub struct ChatAuthoringRuntimeConfig {
    inner: Arc<RwLock<ChatAuthoringConfig>>,
}

impl ChatAuthoringRuntimeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn snapshot(&self) -> ChatAuthoringConfig {
        self.inner.read().await.clone()
    }

    pub async fn set(&self, cfg: ChatAuthoringConfig) {
        *self.inner.write().await = cfg;
    }

    pub async fn automations_enabled(&self) -> bool {
        self.inner.read().await.automations_enabled
    }

    pub async fn work_tasks_enabled(&self) -> bool {
        self.inner.read().await.work_tasks_enabled
    }
}

/// The host's IANA time zone (e.g. `Asia/Shanghai`), falling back to `UTC` when
/// the platform can't report one. Used as the default zone for a scheduled
/// automation so "every day at 9am" means 9am where the user actually is.
pub fn local_timezone() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string())
}

/// Truncate to at most `cap` characters (character-, not byte-counted, so a
/// multi-byte name is never split mid-codepoint).
pub fn truncate_chars(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    s.chars().take(cap).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_is_soft_and_carries_note() {
        let out = AuthoringOutcome::rejected("automation", "turned off");
        assert!(!out.created);
        assert_eq!(out.kind, "automation");
        assert_eq!(out.note.as_deref(), Some("turned off"));
        assert!(out.id.is_none());
    }

    #[test]
    fn rejected_serializes_without_absent_option_fields() {
        let v = serde_json::to_value(AuthoringOutcome::rejected("work_task", "no folder")).unwrap();
        assert_eq!(v["created"], false);
        assert_eq!(v["kind"], "work_task");
        assert!(v.get("id").is_none());
        assert!(v.get("cron").is_none());
        assert!(v.get("note").is_some());
    }

    #[tokio::test]
    async fn runtime_config_round_trips_each_flag_independently() {
        let cfg = ChatAuthoringRuntimeConfig::new();
        assert!(!cfg.automations_enabled().await);
        assert!(!cfg.work_tasks_enabled().await);
        cfg.set(ChatAuthoringConfig {
            automations_enabled: true,
            work_tasks_enabled: false,
        })
        .await;
        assert!(cfg.automations_enabled().await);
        assert!(!cfg.work_tasks_enabled().await);
        assert_eq!(
            cfg.snapshot().await,
            ChatAuthoringConfig {
                automations_enabled: true,
                work_tasks_enabled: false,
            }
        );
    }

    #[test]
    fn local_timezone_is_non_empty() {
        assert!(!local_timezone().is_empty());
    }

    #[test]
    fn truncate_chars_respects_codepoints() {
        assert_eq!(truncate_chars("abc", 10), "abc");
        assert_eq!(truncate_chars("每日构建检查", 3), "每日构");
    }
}
