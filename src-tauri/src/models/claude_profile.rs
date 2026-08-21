use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How a Claude launch profile supplies the process's `CLAUDE_CONFIG_DIR`
/// and connection env.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaudeProfileKind {
    /// Do not set `CLAUDE_CONFIG_DIR` (today's behaviour: SDK uses `~/.claude`).
    FollowDefault,
    /// Force `ANTHROPIC_BASE_URL=https://api.anthropic.com` and clear the
    /// auth token so the CLI falls back to config-dir OAuth. Virtual; no file.
    OfficialDirect,
    /// Point `CLAUDE_CONFIG_DIR` at an existing directory the user maintains.
    ConfigDir,
    /// codeg-owned directory under the data dir, with a generated `settings.json`.
    Managed,
}

/// On-disk record under `<data_dir>/claude-profiles/<id>.json`.
///
/// `authToken` is stored in the file (0600 on Unix) so managed profiles can
/// materialize `settings.json`. It is never returned on the wire.
/// `env` defaults to empty so pre-O66 files without the field still load.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeProfileRecord {
    pub id: String,
    pub label: String,
    pub kind: ClaudeProfileKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Wire DTO for list/upsert. Tokens are masked; there is no raw `authToken`.
/// Secret-looking `env` values (key name contains TOKEN / KEY / SECRET) are
/// masked the same way as `authTokenMasked`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeProfileInfo {
    pub id: String,
    pub label: String,
    pub kind: ClaudeProfileKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub auth_token_masked: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub is_virtual: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Create/update payload from the frontend. `authToken` is write-only:
/// omitted on update keeps the stored value; empty string clears it.
/// `env` is whole-map replace: omitted = leave stored env; `{}` = clear.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeProfileUpsert {
    pub id: String,
    pub label: String,
    pub kind: ClaudeProfileKind,
    #[serde(default)]
    pub config_dir: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
}

/// Result of binding a conversation to a profile (or unbinding with `null`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationClaudeProfileResult {
    pub conversation_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    /// 1 if a live ACP connection for this conversation was marked stale.
    pub affected_running_sessions: usize,
}
