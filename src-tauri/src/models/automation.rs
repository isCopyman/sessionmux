use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use crate::db::entities::automation::{IsolationMode, TriggerKind};
pub use crate::db::entities::automation_run::AutomationRunStatus;

/// A saved, schedulable, replayable composer launch. Wire form mirrors
/// `src/lib/types.ts` (`Automation`).
#[derive(Debug, Clone, Serialize)]
pub struct AutomationInfo {
    pub id: i32,
    pub name: String,
    pub enabled: bool,
    pub trigger_kind: TriggerKind,
    pub cron: Option<String>,
    pub timezone: String,
    pub next_run_at: Option<DateTime<Utc>>,
    pub agent_type: String,
    pub root_folder_id: Option<i32>,
    pub isolation: IsolationMode,
    pub branch: Option<String>,
    pub is_remote_branch: bool,
    /// Opaque captured composer snapshot (see `AutomationConfig`); replayed
    /// wholesale at fire, never queried.
    pub config: serde_json::Value,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_run_status: Option<String>,
    pub last_run_conversation_id: Option<i32>,
    pub unseen_failures: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One launch+settle of an automation. Wire form mirrors `AutomationRun` in
/// `types.ts`. `connection_id` is intentionally omitted (internal correlation).
#[derive(Debug, Clone, Serialize)]
pub struct AutomationRunInfo {
    pub id: i32,
    pub automation_id: i32,
    pub status: AutomationRunStatus,
    pub trigger: String,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub conversation_id: Option<i32>,
    pub worktree_folder_id: Option<i32>,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Full create/update payload — the editor loads the whole automation and saves
/// it back wholesale (a "saved composer" has no partial-patch semantics).
#[derive(Debug, Clone, Deserialize)]
pub struct AutomationDraft {
    pub name: String,
    pub enabled: bool,
    pub trigger_kind: TriggerKind,
    pub cron: Option<String>,
    pub timezone: String,
    pub agent_type: String,
    pub root_folder_id: Option<i32>,
    pub isolation: IsolationMode,
    pub branch: Option<String>,
    pub is_remote_branch: bool,
    pub config: serde_json::Value,
}

/// What firing the automation does. Lives inside the config blob (not a
/// column) so every pre-existing row deserializes as the legacy default —
/// nothing queries automations by action.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationAction {
    /// Launch a headless agent session (the original behavior).
    #[default]
    LaunchSession,
    /// Enqueue a work task (status todo) on the target folder's board; the
    /// work-task engine owns the actual execution.
    EnqueueTask,
    /// Enqueue the captured prompt into an existing Session's prompt queue
    /// (middle dispatch class, behind the user's own typing). No session is
    /// launched — the target Session's own runtime picks the item up.
    QueuePrompt,
}

/// The structured shape stored inside `automation.config`. Kept tolerant
/// (`#[serde(default)]`) so an older/newer snapshot still deserializes; the fire
/// path reads `action` + `prompt_blocks` + `mode_id` + `config_values`, the
/// rest is display.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutomationConfig {
    #[serde(default)]
    pub action: AutomationAction,
    #[serde(default)]
    pub prompt_blocks: Vec<serde_json::Value>,
    #[serde(default)]
    pub display_text: String,
    #[serde(default)]
    pub mode_id: Option<String>,
    #[serde(default)]
    pub config_values: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub label_snapshot: Option<serde_json::Value>,
    /// `queue_prompt` target: the existing Session the captured prompt is
    /// enqueued into. Unused by the other actions.
    #[serde(default)]
    pub target_conversation_id: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frozen on-disk casing for `automation.config`. Adding
    /// `rename_all = "camelCase"` (or renaming action variants) would orphan
    /// existing rows.
    #[test]
    fn automation_config_persisted_json_shape_is_pinned() {
        let legacy = r#"{
            "action": "queue_prompt",
            "prompt_blocks": [{"type":"text","text":"hi"}],
            "display_text": "hi",
            "mode_id": "default",
            "config_values": {"k": "v"},
            "label_snapshot": {"n": 1},
            "target_conversation_id": 7
        }"#;
        let cfg: AutomationConfig =
            serde_json::from_str(legacy).expect("legacy automation.config decodes");
        assert_eq!(cfg.action, AutomationAction::QueuePrompt);
        assert_eq!(cfg.display_text, "hi");
        assert_eq!(cfg.mode_id.as_deref(), Some("default"));
        assert_eq!(cfg.target_conversation_id, Some(7));
        assert_eq!(cfg.prompt_blocks.len(), 1);

        let v = serde_json::to_value(&cfg).unwrap();
        assert_eq!(v["action"], "queue_prompt");
        assert!(v.get("prompt_blocks").is_some());
        assert!(v.get("promptBlocks").is_none());
        assert!(v.get("display_text").is_some());
        assert!(v.get("displayText").is_none());
        assert!(v.get("mode_id").is_some());
        assert!(v.get("modeId").is_none());
        assert!(v.get("config_values").is_some());
        assert!(v.get("configValues").is_none());
        assert!(v.get("label_snapshot").is_some());
        assert!(v.get("labelSnapshot").is_none());
        assert!(v.get("target_conversation_id").is_some());
        assert!(v.get("targetConversationId").is_none());

        for (action, wire) in [
            (AutomationAction::LaunchSession, "launch_session"),
            (AutomationAction::EnqueueTask, "enqueue_task"),
            (AutomationAction::QueuePrompt, "queue_prompt"),
        ] {
            assert_eq!(serde_json::to_value(action).unwrap(), wire);
            assert_eq!(
                serde_json::from_str::<AutomationAction>(&format!("\"{wire}\"")).unwrap(),
                action
            );
        }
        assert!(serde_json::from_str::<AutomationAction>("\"launchSession\"").is_err());
        assert!(serde_json::from_str::<AutomationAction>("\"LaunchSession\"").is_err());
    }
}
