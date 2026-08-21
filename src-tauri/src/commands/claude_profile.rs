//! Per-session Claude launch profiles.
//!
//! Profiles live as files under `<data_dir>/claude-profiles/`, not in SQLite.
//! Binding a conversation to a profile is stored in
//! `conversation.preferred_config_values["__codeg_profile__"]`.
//!
//! `codeg-mcp` is **not** written into a managed profile's `settings.json`.
//! The companion is injected over the ACP wire (`session/new.mcpServers`) by
//! `inject_codeg_mcp` in `acp/connection.rs`, independent of
//! `CLAUDE_CONFIG_DIR`. See CLAUDE-PROFILE-BACKEND-REPORT.md.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Deserialize;

use crate::acp::connection::PREFERRED_PROFILE_CONFIG_KEY;
use crate::acp::error::AcpError;
use crate::acp::manager::ConnectionManager;
use crate::acp::types::ConfigStaleKind;
use crate::app_error::AppCommandError;
use crate::db::service::conversation_service;
use crate::db::AppDatabase;
use crate::models::agent::AgentType;
use crate::models::claude_profile::{
    ClaudeProfileInfo, ClaudeProfileKind, ClaudeProfileRecord, ClaudeProfileUpsert,
    ConversationClaudeProfileResult,
};
use crate::models::model_provider::mask_api_key;

/// Virtual profile id: do not set `CLAUDE_CONFIG_DIR`. Users cannot create a
/// file with this id.
pub const FOLLOW_DEFAULT_PROFILE_ID: &str = "follow-default";

/// Agent-setting `env_json` key for the default Claude profile (no extra DB
/// column). Same store as `CLAUDE_AUTH_MODE`.
pub const CODEG_CLAUDE_PROFILE_ENV_KEY: &str = "CODEG_CLAUDE_PROFILE";

const PROFILES_DIR_NAME: &str = "claude-profiles";
const VIRTUAL_CREATED_AT: &str = "1970-01-01T00:00:00Z";

/// Resolved launch profile: which directory (if any) becomes `CLAUDE_CONFIG_DIR`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedClaudeProfile {
    pub id: String,
    pub kind: ClaudeProfileKind,
    /// `None` for `follow-default` — the env key must not be set.
    pub config_dir: Option<PathBuf>,
}

impl ResolvedClaudeProfile {
    fn follow_default() -> Self {
        Self {
            id: FOLLOW_DEFAULT_PROFILE_ID.to_string(),
            kind: ClaudeProfileKind::FollowDefault,
            config_dir: None,
        }
    }
}

pub fn claude_profiles_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(PROFILES_DIR_NAME)
}

fn profile_record_path(data_dir: &Path, id: &str) -> PathBuf {
    claude_profiles_dir(data_dir).join(format!("{id}.json"))
}

/// Materialized Claude config directory for a `managed` profile.
/// Sits beside `<id>.json` (`foo.json` and `foo/` are distinct names).
pub fn managed_config_dir(data_dir: &Path, id: &str) -> PathBuf {
    claude_profiles_dir(data_dir).join(id)
}

pub fn is_valid_profile_id(id: &str) -> bool {
    let len = id.len();
    (1..=64).contains(&len)
        && id
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
}

fn follow_default_info() -> ClaudeProfileInfo {
    ClaudeProfileInfo {
        id: FOLLOW_DEFAULT_PROFILE_ID.to_string(),
        label: "Follow default".to_string(),
        kind: ClaudeProfileKind::FollowDefault,
        config_dir: None,
        base_url: None,
        auth_token_masked: String::new(),
        model: None,
        created_at: VIRTUAL_CREATED_AT.to_string(),
        updated_at: VIRTUAL_CREATED_AT.to_string(),
    }
}

fn record_to_info(record: &ClaudeProfileRecord) -> ClaudeProfileInfo {
    ClaudeProfileInfo {
        id: record.id.clone(),
        label: record.label.clone(),
        kind: record.kind,
        config_dir: record.config_dir.clone(),
        base_url: record.base_url.clone(),
        auth_token_masked: record
            .auth_token
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(mask_api_key)
            .unwrap_or_default(),
        model: record.model.clone(),
        created_at: record.created_at.clone(),
        updated_at: record.updated_at.clone(),
    }
}

fn write_owner_only_file(path: &Path, contents: &str) -> Result<(), AppCommandError> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt as _;
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(parent)
                    .map_err(AppCommandError::io)?;
            }
            #[cfg(not(unix))]
            fs::create_dir_all(parent).map_err(AppCommandError::io)?;
        }
    }

    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        // Fresh path (or dangling symlink): create owner-only 0600.
        if fs::metadata(path).is_err() {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)
                .map_err(AppCommandError::io)?;
            return file
                .write_all(contents.as_bytes())
                .map_err(AppCommandError::io);
        }
        fs::write(path, contents).map_err(AppCommandError::io)?;
        use std::os::unix::fs::PermissionsExt as _;
        if let Ok(meta) = fs::metadata(path) {
            if meta.permissions().mode() & 0o007 != 0 {
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
            }
        }
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        // Windows has no POSIX 0600; ACLs follow the parent directory. Same
        // policy as `write_hermes_secret_file`.
        fs::write(path, contents).map_err(AppCommandError::io)
    }
}

fn read_record(data_dir: &Path, id: &str) -> Result<Option<ClaudeProfileRecord>, AppCommandError> {
    let path = profile_record_path(data_dir, id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(AppCommandError::io)?;
    let record = serde_json::from_str::<ClaudeProfileRecord>(&raw).map_err(|e| {
        AppCommandError::configuration_invalid(format!(
            "invalid Claude profile {}: {e}",
            path.display()
        ))
    })?;
    Ok(Some(record))
}

fn trim_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Write `claude-profiles/<id>/settings.json` for a managed profile.
/// Only env keys with a non-empty value are included. Replace semantics
/// (idempotent): the file is rewritten from the record, not merged.
pub fn materialize_managed_profile(
    data_dir: &Path,
    record: &ClaudeProfileRecord,
) -> Result<PathBuf, AppCommandError> {
    let dir = managed_config_dir(data_dir, &record.id);
    fs::create_dir_all(&dir).map_err(AppCommandError::io)?;

    let mut env = serde_json::Map::new();
    if let Some(url) = trim_non_empty(record.base_url.as_deref()) {
        env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            serde_json::Value::String(url),
        );
    }
    if let Some(token) = trim_non_empty(record.auth_token.as_deref()) {
        env.insert(
            "ANTHROPIC_AUTH_TOKEN".to_string(),
            serde_json::Value::String(token),
        );
    }
    if let Some(model) = trim_non_empty(record.model.as_deref()) {
        env.insert(
            "ANTHROPIC_MODEL".to_string(),
            serde_json::Value::String(model),
        );
    }
    let settings = serde_json::json!({ "env": env });
    let body = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppCommandError::configuration_invalid(e.to_string()))?;
    write_owner_only_file(&dir.join("settings.json"), &format!("{body}\n"))?;
    Ok(dir)
}

fn load_record_or_err(data_dir: &Path, id: &str) -> Result<ClaudeProfileRecord, AppCommandError> {
    read_record(data_dir, id)?
        .ok_or_else(|| AppCommandError::not_found(format!("Claude profile '{id}' not found")))
}

fn config_dir_for_record(
    data_dir: &Path,
    record: &ClaudeProfileRecord,
) -> Result<Option<PathBuf>, AppCommandError> {
    match record.kind {
        ClaudeProfileKind::FollowDefault => Ok(None),
        ClaudeProfileKind::ConfigDir => {
            let raw = trim_non_empty(record.config_dir.as_deref()).ok_or_else(|| {
                AppCommandError::configuration_invalid(format!(
                    "profile '{}' is configDir but has no configDir",
                    record.id
                ))
            })?;
            Ok(Some(PathBuf::from(raw)))
        }
        ClaudeProfileKind::Managed => {
            let dir = materialize_managed_profile(data_dir, record)?;
            Ok(Some(dir))
        }
    }
}

fn env_json_profile_id(env_json: Option<&str>) -> Option<String> {
    let raw = env_json.map(str::trim).filter(|s| !s.is_empty())?;
    let map: BTreeMap<String, String> = serde_json::from_str(raw).ok()?;
    trim_non_empty(map.get(CODEG_CLAUDE_PROFILE_ENV_KEY).map(String::as_str))
}

fn resolve_id_to_profile(
    data_dir: &Path,
    id: &str,
) -> Result<ResolvedClaudeProfile, AppCommandError> {
    if id == FOLLOW_DEFAULT_PROFILE_ID {
        return Ok(ResolvedClaudeProfile::follow_default());
    }
    match read_record(data_dir, id) {
        Ok(Some(record)) => {
            let config_dir = config_dir_for_record(data_dir, &record)?;
            Ok(ResolvedClaudeProfile {
                id: record.id,
                kind: record.kind,
                config_dir,
            })
        }
        Ok(None) => {
            tracing::warn!(
                "[claude-profile] bound profile '{id}' is missing on disk; \
                 falling back to follow-default"
            );
            Ok(ResolvedClaudeProfile::follow_default())
        }
        Err(err) => {
            tracing::warn!(
                "[claude-profile] failed to load bound profile '{id}': {err}; \
                 falling back to follow-default"
            );
            Ok(ResolvedClaudeProfile::follow_default())
        }
    }
}

/// Single source of truth for "which Claude profile does this spawn use".
///
/// Order: conversation `__codeg_profile__` → agent `env_json`
/// `CODEG_CLAUDE_PROFILE` → `follow-default`.
pub async fn resolve_claude_profile(
    db: &AppDatabase,
    data_dir: &Path,
    conversation_id: Option<i32>,
    claude_agent_env_json: Option<&str>,
) -> Result<ResolvedClaudeProfile, AppCommandError> {
    if let Some(cid) = conversation_id {
        let (_mode, values) = conversation_service::selector_prefs(&db.conn, cid)
            .await
            .map_err(AppCommandError::from)?;
        if let Some(id) = values
            .get(PREFERRED_PROFILE_CONFIG_KEY)
            .map(String::as_str)
            .and_then(|s| trim_non_empty(Some(s)))
        {
            return resolve_id_to_profile(data_dir, &id);
        }
    }

    if let Some(id) = env_json_profile_id(claude_agent_env_json) {
        return resolve_id_to_profile(data_dir, &id);
    }

    Ok(ResolvedClaudeProfile::follow_default())
}

/// Inject `CLAUDE_CONFIG_DIR` last for Claude Code. A user-explicit
/// `CLAUDE_CONFIG_DIR` in `env_json` wins and is warned; `follow-default`
/// does not set the key.
pub async fn apply_claude_profile_env(
    db: &AppDatabase,
    data_dir: &Path,
    conversation_id: Option<i32>,
    claude_agent_env_json: Option<&str>,
    runtime_env: &mut BTreeMap<String, String>,
) -> Result<(), AcpError> {
    let resolved = resolve_claude_profile(db, data_dir, conversation_id, claude_agent_env_json)
        .await
        .map_err(|e| AcpError::protocol(e.to_string()))?;
    let Some(dir) = resolved.config_dir else {
        return Ok(());
    };
    let env_json_has_key = claude_agent_env_json
        .and_then(|raw| serde_json::from_str::<BTreeMap<String, String>>(raw).ok())
        .is_some_and(|map| map.contains_key("CLAUDE_CONFIG_DIR"));
    if env_json_has_key {
        tracing::warn!(
            "[claude-profile] env_json already sets CLAUDE_CONFIG_DIR; \
             not overriding with profile '{}' ({})",
            resolved.id,
            dir.display()
        );
        return Ok(());
    }
    runtime_env.insert(
        "CLAUDE_CONFIG_DIR".to_string(),
        dir.to_string_lossy().into_owned(),
    );
    Ok(())
}

pub fn claude_profile_list_core(
    data_dir: &Path,
) -> Result<Vec<ClaudeProfileInfo>, AppCommandError> {
    let mut out = vec![follow_default_info()];
    let dir = claude_profiles_dir(data_dir);
    if !dir.exists() {
        return Ok(out);
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(AppCommandError::io)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    for path in files {
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) => {
                tracing::warn!("[claude-profile] skip unreadable {}: {e}", path.display());
                continue;
            }
        };
        match serde_json::from_str::<ClaudeProfileRecord>(&raw) {
            Ok(record) => {
                if record.id == FOLLOW_DEFAULT_PROFILE_ID {
                    continue;
                }
                out.push(record_to_info(&record));
            }
            Err(e) => {
                tracing::warn!("[claude-profile] skip invalid {}: {e}", path.display());
            }
        }
    }
    Ok(out)
}

/// Existing work-task / automation `config_values` key for the model pin.
/// Consumed by `work_task::engine` (`config_values.get("model")`) and applied
/// as `preferred_config_values["model"]` on spawn. Do not invent a second key.
pub const LAUNCH_MODEL_CONFIG_KEY: &str = "model";

/// One-line destination for an LLM. Never includes tokens (plain or masked).
pub fn profile_destination_summary(info: &ClaudeProfileInfo) -> String {
    match info.kind {
        ClaudeProfileKind::FollowDefault => {
            "follow-default: the host's default Claude configuration \
             (subscription login); CLAUDE_CONFIG_DIR is not set"
                .to_string()
        }
        ClaudeProfileKind::ConfigDir => match info.config_dir.as_deref() {
            Some(path) => format!("configDir: {path}"),
            None => "configDir: (unset)".to_string(),
        },
        ClaudeProfileKind::Managed => match info.base_url.as_deref() {
            Some(url) => format!("managed: {url}"),
            None => "managed: (no baseUrl)".to_string(),
        },
    }
}

/// Reject unknown ids. Error text lists every currently valid id so the LLM
/// can retry; never silently fall back to follow-default.
pub fn require_known_claude_profile(data_dir: &Path, id: &str) -> Result<(), String> {
    let list = claude_profile_list_core(data_dir).map_err(|e| e.to_string())?;
    if list.iter().any(|p| p.id == id) {
        return Ok(());
    }
    let ids: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
    Err(format!(
        "unknown Claude launch profile '{id}'. Valid ids: {}. \
         Use list_profiles to inspect them; omitting `profile` uses the default. \
         Invalid ids are rejected — they do not silently fall back.",
        ids.join(", ")
    ))
}

/// Build the `config_values` map that becomes the new session's
/// `preferred_config_values`. Both omitted → empty map (today's bytes).
/// Unknown `profile` → error, nothing inserted.
pub fn launch_config_values(
    data_dir: &Path,
    profile: Option<&str>,
    model: Option<&str>,
) -> Result<BTreeMap<String, String>, String> {
    let mut values = BTreeMap::new();
    if let Some(id) = profile {
        require_known_claude_profile(data_dir, id)?;
        values.insert(PREFERRED_PROFILE_CONFIG_KEY.to_string(), id.to_string());
    }
    if let Some(model) = model {
        values.insert(LAUNCH_MODEL_CONFIG_KEY.to_string(), model.to_string());
    }
    Ok(values)
}

fn validate_upsert(input: &ClaudeProfileUpsert) -> Result<(), AppCommandError> {
    if !is_valid_profile_id(&input.id) {
        return Err(AppCommandError::invalid_input(
            "profile id must match [a-z0-9-_]{1,64}",
        ));
    }
    if input.id == FOLLOW_DEFAULT_PROFILE_ID {
        return Err(AppCommandError::invalid_input(
            "profile id 'follow-default' is reserved",
        ));
    }
    let label = input.label.trim();
    if label.is_empty() {
        return Err(AppCommandError::invalid_input("profile label is required"));
    }
    if label.len() > 128 {
        return Err(AppCommandError::invalid_input(
            "profile label must be 128 characters or less",
        ));
    }
    match input.kind {
        ClaudeProfileKind::FollowDefault => {
            return Err(AppCommandError::invalid_input(
                "cannot persist the virtual follow-default profile",
            ));
        }
        ClaudeProfileKind::ConfigDir => {
            let dir = trim_non_empty(input.config_dir.as_deref()).ok_or_else(|| {
                AppCommandError::invalid_input("configDir is required for kind=configDir")
            })?;
            if !Path::new(&dir).is_absolute() {
                return Err(AppCommandError::invalid_input(
                    "configDir must be an absolute path",
                ));
            }
        }
        ClaudeProfileKind::Managed => {}
    }
    Ok(())
}

pub fn claude_profile_upsert_core(
    data_dir: &Path,
    input: ClaudeProfileUpsert,
) -> Result<ClaudeProfileInfo, AppCommandError> {
    validate_upsert(&input)?;
    let now = Utc::now().to_rfc3339();
    let existing = read_record(data_dir, &input.id)?;
    let created_at = existing
        .as_ref()
        .map(|r| r.created_at.clone())
        .unwrap_or_else(|| now.clone());
    let auth_token = match input.auth_token.as_deref() {
        None => existing.and_then(|r| r.auth_token),
        Some(raw) => trim_non_empty(Some(raw)),
    };
    let record = ClaudeProfileRecord {
        id: input.id.clone(),
        label: input.label.trim().to_string(),
        kind: input.kind,
        config_dir: match input.kind {
            ClaudeProfileKind::ConfigDir => trim_non_empty(input.config_dir.as_deref()),
            _ => None,
        },
        base_url: match input.kind {
            ClaudeProfileKind::Managed => trim_non_empty(input.base_url.as_deref()),
            _ => None,
        },
        auth_token: match input.kind {
            ClaudeProfileKind::Managed => auth_token,
            _ => None,
        },
        model: match input.kind {
            ClaudeProfileKind::Managed => trim_non_empty(input.model.as_deref()),
            _ => None,
        },
        created_at,
        updated_at: now,
    };
    let serialized = serde_json::to_string_pretty(&record)
        .map_err(|e| AppCommandError::configuration_invalid(e.to_string()))?;
    write_owner_only_file(
        &profile_record_path(data_dir, &record.id),
        &format!("{serialized}\n"),
    )?;
    if record.kind == ClaudeProfileKind::Managed {
        materialize_managed_profile(data_dir, &record)?;
    }
    Ok(record_to_info(&record))
}

pub fn claude_profile_delete_core(data_dir: &Path, id: &str) -> Result<(), AppCommandError> {
    if id == FOLLOW_DEFAULT_PROFILE_ID {
        return Err(AppCommandError::invalid_input(
            "cannot delete the virtual follow-default profile",
        ));
    }
    if !is_valid_profile_id(id) {
        return Err(AppCommandError::invalid_input(
            "profile id must match [a-z0-9-_]{1,64}",
        ));
    }
    let path = profile_record_path(data_dir, id);
    if !path.exists() {
        return Err(AppCommandError::not_found(format!(
            "Claude profile '{id}' not found"
        )));
    }
    fs::remove_file(&path).map_err(AppCommandError::io)?;
    let managed_dir = managed_config_dir(data_dir, id);
    if managed_dir.is_dir() {
        fs::remove_dir_all(&managed_dir).map_err(AppCommandError::io)?;
    }
    Ok(())
}

pub async fn conversation_set_claude_profile_core(
    db: &AppDatabase,
    manager: &ConnectionManager,
    data_dir: &Path,
    conversation_id: i32,
    profile_id: Option<String>,
) -> Result<ConversationClaudeProfileResult, AppCommandError> {
    let row = conversation_service::get_by_id(&db.conn, conversation_id)
        .await
        .map_err(AppCommandError::from)?;

    let (_mode, current) = conversation_service::selector_prefs(&db.conn, conversation_id)
        .await
        .map_err(AppCommandError::from)?;
    let previous = current.get(PREFERRED_PROFILE_CONFIG_KEY).cloned();

    let new_id = match profile_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(FOLLOW_DEFAULT_PROFILE_ID) => Some(FOLLOW_DEFAULT_PROFILE_ID.to_string()),
        Some(id) => {
            if !is_valid_profile_id(id) {
                return Err(AppCommandError::invalid_input(
                    "profile id must match [a-z0-9-_]{1,64}",
                ));
            }
            load_record_or_err(data_dir, id)?;
            Some(id.to_string())
        }
    };

    if previous.as_deref() != new_id.as_deref() {
        match new_id.as_deref() {
            Some(id) => {
                conversation_service::merge_selector_config_value(
                    &db.conn,
                    conversation_id,
                    PREFERRED_PROFILE_CONFIG_KEY,
                    id,
                )
                .await
                .map_err(AppCommandError::from)?;
            }
            None => {
                conversation_service::remove_selector_config_value(
                    &db.conn,
                    conversation_id,
                    PREFERRED_PROFILE_CONFIG_KEY,
                )
                .await
                .map_err(AppCommandError::from)?;
            }
        }
    }

    let mut affected = 0usize;
    // Only a live Claude session needs the "next turn / reconnect" banner.
    // Binding is stored regardless of agent type (zero schema change).
    let is_claude = row.agent_type == AgentType::ClaudeCode;
    if is_claude
        && previous.as_deref() != new_id.as_deref()
        && manager
            .mark_conversation_config_stale(conversation_id, ConfigStaleKind::AgentConfig)
            .await
    {
        affected = 1;
    }

    Ok(ConversationClaudeProfileResult {
        conversation_id,
        profile_id: new_id,
        affected_running_sessions: affected,
    })
}

#[cfg(feature = "tauri-runtime")]
fn tauri_data_dir(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .map(|p| crate::paths::resolve_effective_data_dir(&p))
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub fn claude_profile_list(
    app: tauri::AppHandle,
) -> Result<Vec<ClaudeProfileInfo>, AppCommandError> {
    claude_profile_list_core(&tauri_data_dir(&app))
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub fn claude_profile_upsert(
    app: tauri::AppHandle,
    payload: ClaudeProfileUpsert,
) -> Result<ClaudeProfileInfo, AppCommandError> {
    claude_profile_upsert_core(&tauri_data_dir(&app), payload)
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub fn claude_profile_delete(app: tauri::AppHandle, id: String) -> Result<(), AppCommandError> {
    claude_profile_delete_core(&tauri_data_dir(&app), &id)
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn conversation_set_claude_profile(
    conversation_id: i32,
    profile_id: Option<String>,
    db: tauri::State<'_, AppDatabase>,
    manager: tauri::State<'_, ConnectionManager>,
    app: tauri::AppHandle,
) -> Result<ConversationClaudeProfileResult, AppCommandError> {
    conversation_set_claude_profile_core(
        &db,
        &manager,
        &tauri_data_dir(&app),
        conversation_id,
        profile_id,
    )
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeProfileDeleteParams {
    pub id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSetClaudeProfileParams {
    pub conversation_id: i32,
    pub profile_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::db::service::agent_setting_service::{self, AgentDefaultInput, AgentSettingsUpdate};
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::agent::AgentType;

    async fn seed_claude_agent(db: &AppDatabase, env_json: Option<&str>) {
        agent_setting_service::ensure_defaults(
            &db.conn,
            &[
                AgentDefaultInput {
                    agent_type: AgentType::ClaudeCode,
                    registry_id: "claude_code".to_string(),
                    default_sort_order: 0,
                },
                AgentDefaultInput {
                    agent_type: AgentType::Codex,
                    registry_id: "codex".to_string(),
                    default_sort_order: 1,
                },
            ],
        )
        .await
        .expect("ensure defaults");
        if let Some(raw) = env_json {
            agent_setting_service::update(
                &db.conn,
                AgentType::ClaudeCode,
                AgentSettingsUpdate {
                    enabled: true,
                    env_json: Some(raw.to_string()),
                    model_provider_id: None,
                },
            )
            .await
            .expect("patch env_json");
        }
    }

    fn upsert_config_dir(data_dir: &Path, id: &str, dir: &Path) -> ClaudeProfileInfo {
        claude_profile_upsert_core(
            data_dir,
            ClaudeProfileUpsert {
                id: id.to_string(),
                label: format!("Label {id}"),
                kind: ClaudeProfileKind::ConfigDir,
                config_dir: Some(dir.to_string_lossy().into_owned()),
                base_url: None,
                auth_token: None,
                model: None,
            },
        )
        .expect("upsert configDir")
    }

    fn upsert_managed(
        data_dir: &Path,
        id: &str,
        base_url: Option<&str>,
        auth_token: Option<&str>,
        model: Option<&str>,
    ) -> ClaudeProfileInfo {
        claude_profile_upsert_core(
            data_dir,
            ClaudeProfileUpsert {
                id: id.to_string(),
                label: format!("Managed {id}"),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: base_url.map(str::to_string),
                auth_token: auth_token.map(str::to_string),
                model: model.map(str::to_string),
            },
        )
        .expect("upsert managed")
    }

    #[test]
    fn profile_id_rejects_reserved_and_illegal() {
        assert!(is_valid_profile_id("work"));
        assert!(is_valid_profile_id("a"));
        assert!(is_valid_profile_id(&"a".repeat(64)));
        assert!(!is_valid_profile_id(""));
        assert!(!is_valid_profile_id(&"a".repeat(65)));
        assert!(!is_valid_profile_id("Work"));
        assert!(!is_valid_profile_id("has space"));
        assert!(!is_valid_profile_id("has/slash"));
        assert!(!is_valid_profile_id("dot.json"));
        let data = tempfile::tempdir().unwrap();
        let err = claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: FOLLOW_DEFAULT_PROFILE_ID.to_string(),
                label: "nope".into(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: None,
                auth_token: None,
                model: None,
            },
        )
        .unwrap_err();
        assert!(err.message.contains("reserved"), "{}", err.message);
        let err = claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "Bad Id".into(),
                label: "nope".into(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: None,
                auth_token: None,
                model: None,
            },
        )
        .unwrap_err();
        assert!(err.message.contains("[a-z0-9-_]"), "{}", err.message);
    }

    #[test]
    fn list_always_starts_with_virtual_follow_default() {
        let data = tempfile::tempdir().unwrap();
        let list = claude_profile_list_core(data.path()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, FOLLOW_DEFAULT_PROFILE_ID);
        assert_eq!(list[0].kind, ClaudeProfileKind::FollowDefault);
        assert!(list[0].auth_token_masked.is_empty());
    }

    #[test]
    fn upsert_masks_token_and_does_not_echo_raw() {
        let data = tempfile::tempdir().unwrap();
        let info = upsert_managed(
            data.path(),
            "api",
            Some("https://example.test"),
            Some("sk-test-1234567890"),
            None,
        );
        assert!(
            !info.auth_token_masked.contains("1234567890")
                || info.auth_token_masked.contains('\u{2022}')
        );
        assert!(info.auth_token_masked.starts_with("sk-t"));
        let wire = serde_json::to_value(&info).unwrap();
        assert!(wire.get("authToken").is_none());
        assert!(wire.get("auth_token").is_none());
        assert!(wire.get("authTokenMasked").is_some());
    }

    #[test]
    fn managed_materialize_writes_only_non_empty_env_keys_and_is_idempotent() {
        let data = tempfile::tempdir().unwrap();
        upsert_managed(
            data.path(),
            "gw",
            Some("https://api.example/v1"),
            Some("sk-secret"),
            None,
        );
        let settings_path = managed_config_dir(data.path(), "gw").join("settings.json");
        let first = fs::read_to_string(&settings_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&first).unwrap();
        let env = parsed.get("env").and_then(|v| v.as_object()).unwrap();
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()),
            Some("https://api.example/v1")
        );
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()),
            Some("sk-secret")
        );
        assert!(!env.contains_key("ANTHROPIC_MODEL"));
        assert!(
            parsed.get("mcpServers").is_none(),
            "codeg-mcp is ACP-wire injected; must not be written into settings.json"
        );

        upsert_managed(
            data.path(),
            "gw",
            Some("https://api.example/v1"),
            Some("sk-secret"),
            None,
        );
        let second = fs::read_to_string(&settings_path).unwrap();
        assert_eq!(first, second, "repeat materialize must be idempotent");

        upsert_managed(
            data.path(),
            "gw",
            Some("https://api.example/v1"),
            Some(""),
            Some("claude-sonnet-4"),
        );
        let third: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        let env = third.get("env").and_then(|v| v.as_object()).unwrap();
        assert!(
            !env.contains_key("ANTHROPIC_AUTH_TOKEN"),
            "cleared token must not be written"
        );
        assert_eq!(
            env.get("ANTHROPIC_MODEL").and_then(|v| v.as_str()),
            Some("claude-sonnet-4")
        );
    }

    #[tokio::test]
    async fn resolve_order_session_then_agent_then_follow_default() {
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        seed_claude_agent(&db, Some(r#"{"CODEG_CLAUDE_PROFILE":"agent-default"}"#)).await;
        let folder = seed_folder(&db, data.path().to_str().unwrap()).await;
        let conv = seed_conversation(&db, folder, AgentType::ClaudeCode).await;

        let agent_dir = data.path().join("agent-claude");
        fs::create_dir_all(&agent_dir).unwrap();
        let session_dir = data.path().join("session-claude");
        fs::create_dir_all(&session_dir).unwrap();
        upsert_config_dir(data.path(), "agent-default", &agent_dir);
        upsert_config_dir(data.path(), "session-bound", &session_dir);

        let env_json = r#"{"CODEG_CLAUDE_PROFILE":"agent-default"}"#;

        let resolved = resolve_claude_profile(&db, data.path(), None, Some(env_json))
            .await
            .unwrap();
        assert_eq!(resolved.id, "agent-default");
        assert_eq!(resolved.config_dir.as_deref(), Some(agent_dir.as_path()));

        conversation_service::merge_selector_config_value(
            &db.conn,
            conv,
            PREFERRED_PROFILE_CONFIG_KEY,
            "session-bound",
        )
        .await
        .unwrap();
        let resolved = resolve_claude_profile(&db, data.path(), Some(conv), Some(env_json))
            .await
            .unwrap();
        assert_eq!(resolved.id, "session-bound");
        assert_eq!(resolved.config_dir.as_deref(), Some(session_dir.as_path()));

        let resolved = resolve_claude_profile(&db, data.path(), None, None)
            .await
            .unwrap();
        assert_eq!(resolved.id, FOLLOW_DEFAULT_PROFILE_ID);
        assert_eq!(resolved.config_dir, None);
    }

    #[tokio::test]
    async fn build_env_follow_default_omits_claude_config_dir() {
        use crate::commands::acp::build_session_runtime_env;
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        seed_claude_agent(&db, None).await;
        let env = build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), None)
            .await
            .unwrap();
        assert!(
            !env.contains_key("CLAUDE_CONFIG_DIR"),
            "follow-default must not set CLAUDE_CONFIG_DIR: {env:?}"
        );
    }

    #[tokio::test]
    async fn build_env_config_dir_and_managed_set_correct_path() {
        use crate::commands::acp::build_session_runtime_env;
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        seed_claude_agent(&db, None).await;
        let folder = seed_folder(&db, data.path().to_str().unwrap()).await;
        let conv = seed_conversation(&db, folder, AgentType::ClaudeCode).await;

        let user_dir = data.path().join("user-claude");
        fs::create_dir_all(&user_dir).unwrap();
        upsert_config_dir(data.path(), "user", &user_dir);
        conversation_service::merge_selector_config_value(
            &db.conn,
            conv,
            PREFERRED_PROFILE_CONFIG_KEY,
            "user",
        )
        .await
        .unwrap();
        let env =
            build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), Some(conv))
                .await
                .unwrap();
        assert_eq!(
            env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
            Some(user_dir.to_string_lossy().as_ref())
        );

        upsert_managed(
            data.path(),
            "hosted",
            Some("https://gw.example"),
            Some("sk-hosted"),
            None,
        );
        conversation_service::merge_selector_config_value(
            &db.conn,
            conv,
            PREFERRED_PROFILE_CONFIG_KEY,
            "hosted",
        )
        .await
        .unwrap();
        let env =
            build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), Some(conv))
                .await
                .unwrap();
        let expected = managed_config_dir(data.path(), "hosted");
        assert_eq!(
            env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
            Some(expected.to_string_lossy().as_ref())
        );
        assert!(expected.join("settings.json").is_file());
    }

    #[tokio::test]
    async fn build_env_non_claude_is_unaffected() {
        use crate::commands::acp::build_session_runtime_env;
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        seed_claude_agent(&db, None).await;
        let folder = seed_folder(&db, data.path().to_str().unwrap()).await;
        let conv = seed_conversation(&db, folder, AgentType::Codex).await;
        let user_dir = data.path().join("user-claude");
        fs::create_dir_all(&user_dir).unwrap();
        upsert_config_dir(data.path(), "user", &user_dir);
        conversation_service::merge_selector_config_value(
            &db.conn,
            conv,
            PREFERRED_PROFILE_CONFIG_KEY,
            "user",
        )
        .await
        .unwrap();
        let env = build_session_runtime_env(&db, AgentType::Codex, None, data.path(), Some(conv))
            .await
            .unwrap();
        assert!(
            !env.contains_key("CLAUDE_CONFIG_DIR"),
            "Codex must not receive CLAUDE_CONFIG_DIR: {env:?}"
        );
    }

    #[tokio::test]
    async fn env_json_explicit_claude_config_dir_wins() {
        use crate::commands::acp::build_session_runtime_env;
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        let explicit = data.path().join("explicit");
        fs::create_dir_all(&explicit).unwrap();
        let env_json = serde_json::to_string(&BTreeMap::from([(
            "CLAUDE_CONFIG_DIR".to_string(),
            explicit.to_string_lossy().into_owned(),
        )]))
        .unwrap();
        seed_claude_agent(&db, Some(&env_json)).await;
        let user_dir = data.path().join("profile-dir");
        fs::create_dir_all(&user_dir).unwrap();
        upsert_config_dir(data.path(), "user", &user_dir);
        let folder = seed_folder(&db, data.path().to_str().unwrap()).await;
        let conv = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        conversation_service::merge_selector_config_value(
            &db.conn,
            conv,
            PREFERRED_PROFILE_CONFIG_KEY,
            "user",
        )
        .await
        .unwrap();
        let env =
            build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), Some(conv))
                .await
                .unwrap();
        assert_eq!(
            env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
            Some(explicit.to_string_lossy().as_ref())
        );
    }

    #[tokio::test]
    async fn set_profile_writes_preferred_and_unsets() {
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        seed_claude_agent(&db, None).await;
        let folder = seed_folder(&db, data.path().to_str().unwrap()).await;
        let conv = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let user_dir = data.path().join("user-claude");
        fs::create_dir_all(&user_dir).unwrap();
        upsert_config_dir(data.path(), "user", &user_dir);
        let mgr = ConnectionManager::new();
        let result =
            conversation_set_claude_profile_core(&db, &mgr, data.path(), conv, Some("user".into()))
                .await
                .unwrap();
        assert_eq!(result.profile_id.as_deref(), Some("user"));
        let (_mode, values) = conversation_service::selector_prefs(&db.conn, conv)
            .await
            .unwrap();
        assert_eq!(
            values.get(PREFERRED_PROFILE_CONFIG_KEY).map(String::as_str),
            Some("user")
        );

        let result = conversation_set_claude_profile_core(&db, &mgr, data.path(), conv, None)
            .await
            .unwrap();
        assert_eq!(result.profile_id, None);
        let (_mode, values) = conversation_service::selector_prefs(&db.conn, conv)
            .await
            .unwrap();
        assert!(!values.contains_key(PREFERRED_PROFILE_CONFIG_KEY));
    }

    #[test]
    fn omitted_profile_and_model_yield_empty_config_values() {
        let data = tempfile::tempdir().unwrap();
        let values = launch_config_values(data.path(), None, None).unwrap();
        assert_eq!(values, BTreeMap::new());
        assert_eq!(
            serde_json::to_value(&values).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn valid_profile_writes_preferred_profile_key() {
        let data = tempfile::tempdir().unwrap();
        upsert_managed(
            data.path(),
            "api",
            Some("https://example.test/v1"),
            None,
            None,
        );
        let values = launch_config_values(data.path(), Some("api"), None).unwrap();
        assert_eq!(
            values.get(PREFERRED_PROFILE_CONFIG_KEY).map(String::as_str),
            Some("api")
        );
        assert!(!values.contains_key(LAUNCH_MODEL_CONFIG_KEY));
    }

    #[test]
    fn unknown_profile_errors_with_valid_ids_and_writes_nothing() {
        let data = tempfile::tempdir().unwrap();
        upsert_managed(
            data.path(),
            "api",
            Some("https://example.test/v1"),
            None,
            None,
        );
        let err = launch_config_values(data.path(), Some("nope"), Some("opus")).unwrap_err();
        assert!(err.contains("nope"), "{err}");
        assert!(err.contains("follow-default"), "{err}");
        assert!(err.contains("api"), "{err}");
        // Nothing to persist: the map is never returned.
        let values = launch_config_values(data.path(), None, None).unwrap();
        assert!(values.is_empty());
    }

    #[test]
    fn model_writes_the_existing_model_key() {
        let data = tempfile::tempdir().unwrap();
        let values = launch_config_values(data.path(), None, Some("claude-opus-4")).unwrap();
        assert_eq!(
            values.get(LAUNCH_MODEL_CONFIG_KEY).map(String::as_str),
            Some("claude-opus-4")
        );
        assert!(!values.contains_key(PREFERRED_PROFILE_CONFIG_KEY));
    }

    #[test]
    fn mcp_profile_list_omits_plain_and_masked_tokens() {
        let data = tempfile::tempdir().unwrap();
        let secret = "sk-super-secret-token-xyz-do-not-leak";
        upsert_managed(
            data.path(),
            "gw",
            Some("https://relay.example/v1"),
            Some(secret),
            Some("claude-sonnet-4"),
        );
        let user_dir = data.path().join("user-claude");
        fs::create_dir_all(&user_dir).unwrap();
        upsert_config_dir(data.path(), "user", &user_dir);

        let list = claude_profile_list_core(data.path()).unwrap();
        let entries: Vec<serde_json::Value> = list
            .iter()
            .map(|info| {
                serde_json::json!({
                    "id": info.id,
                    "label": info.label,
                    "kind": info.kind,
                    "destination": profile_destination_summary(info),
                })
            })
            .collect();
        let dumped = serde_json::to_string(&entries).unwrap();
        assert!(!dumped.contains(secret), "{dumped}");
        assert!(!dumped.contains("authToken"), "{dumped}");
        assert!(!dumped.contains("auth_token"), "{dumped}");
        assert!(!dumped.contains("authTokenMasked"), "{dumped}");

        let follow = list
            .iter()
            .find(|p| p.id == FOLLOW_DEFAULT_PROFILE_ID)
            .unwrap();
        let follow_dest = profile_destination_summary(follow);
        assert!(follow_dest.contains("follow-default"), "{follow_dest}");
        assert!(follow_dest.contains("default"), "{follow_dest}");

        let managed = list.iter().find(|p| p.id == "gw").unwrap();
        let managed_dest = profile_destination_summary(managed);
        assert!(
            managed_dest.contains("https://relay.example/v1"),
            "{managed_dest}"
        );
        assert!(!managed_dest.contains(secret), "{managed_dest}");

        let config_dir = list.iter().find(|p| p.id == "user").unwrap();
        let dir_dest = profile_destination_summary(config_dir);
        assert!(
            dir_dest.contains(&user_dir.to_string_lossy().into_owned()),
            "{dir_dest}"
        );
    }
}
