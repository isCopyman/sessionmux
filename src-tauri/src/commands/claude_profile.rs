//! Per-session Claude launch profiles.
//!
//! Profiles live as files under `<data_dir>/claude-profiles/`, not in SQLite.
//! Binding a conversation to a profile is stored in
//! `conversation.preferred_config_values["__codeg_profile__"]`.
//!
//! `codeg-mcp` is **not** written into a managed profile's `settings.json`.
//! The companion is injected over the ACP wire (`session/new.mcpServers`) by
//! `inject_codeg_mcp` in `acp/connection.rs`. A managed profile takes effect
//! as `_meta.claudeCode.options.extraArgs.settings` (equivalent to
//! `claude --settings <absolute path>`), not as a second `CLAUDE_CONFIG_DIR`.
//! `configDir` profiles still set `CLAUDE_CONFIG_DIR`.

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
use crate::db::service::{agent_setting_service, conversation_service};
use crate::db::AppDatabase;
use crate::models::agent::AgentType;
use crate::models::claude_profile::{
    ClaudeProfileInfo, ClaudeProfileKind, ClaudeProfileRecord, ClaudeProfileUpsert,
    ClaudeSettingsReadResult, ConversationClaudeProfileResult,
};
use crate::models::model_provider::mask_api_key;

/// Virtual profile id: do not set `CLAUDE_CONFIG_DIR`. Users cannot create a
/// file with this id. Follows the CLI / agent `env_json` as today.
pub const FOLLOW_DEFAULT_PROFILE_ID: &str = "follow-default";

/// Virtual profile id (Monet `official-direct`): force the official Anthropic
/// endpoint and clear `ANTHROPIC_AUTH_TOKEN` so the CLI falls back to OAuth
/// in the config directory. No file on disk. Users cannot create this id.
pub const OFFICIAL_DIRECT_PROFILE_ID: &str = "official-direct";

/// Official Anthropic API host forced by `official-direct`.
pub const OFFICIAL_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

/// Agent-setting `env_json` key for the default Claude profile (no extra DB
/// column). Same store as `CLAUDE_AUTH_MODE`.
pub const CODEG_CLAUDE_PROFILE_ENV_KEY: &str = "CODEG_CLAUDE_PROFILE";

/// One-shot marker stored beside `CODEG_CLAUDE_PROFILE` in Claude's existing
/// `agent_setting.env_json`. It deliberately needs no schema column.
pub const CODEG_CLAUDE_PROFILE_MIGRATED_ENV_KEY: &str = "CODEG_CLAUDE_PROFILE_MIGRATED";

/// Stashed on `runtime_env` by [`apply_claude_profile_env`] for a `managed`
/// profile so `session/new` can put the path on
/// `_meta.claudeCode.options.extraArgs.settings`.
///
/// `spawn_agent_connection` removes this key before the child is spawned
/// and before the config fingerprint is hashed. It is never a process
/// environment variable of the agent. Path bytes are stored verbatim
/// (no shell quoting).
pub const CLAUDE_SETTINGS_OVERLAY_ENV_KEY: &str = "CODEG_CLAUDE_SETTINGS_OVERLAY";

const PROFILES_DIR_NAME: &str = "claude-profiles";
const VIRTUAL_CREATED_AT: &str = "1970-01-01T00:00:00Z";

/// Connection / auth / model-routing keys a launch profile owns.
///
/// When the resolved profile is **not** `follow-default`, agent-global
/// `env_json` (and the rest of `build_session_runtime_env`'s overlays:
/// native `~/.claude/settings.json`, a bound `model_provider`) must not
/// inject these. Otherwise a "subscription" profile still burns API:
/// Claude Code's SDK credential order (`Du()`) is `ANTHROPIC_AUTH_TOKEN`
/// env > … > stored OAuth, so a leftover global token silently wins.
///
/// Non-connection keys (`CLAUDE_CODE_GIT_BASH_PATH`, `DISABLE_TELEMETRY`,
/// `ENABLE_TOOL_SEARCH`, `CLAUDE_CODE_SCROLL_SPEED`, …) stay injected —
/// they have nothing to do with which account the process uses.
///
/// `follow-default` does **not** sweep (that *is* "follow the CLI").
/// Claude Code only.
pub const PROFILE_OWNED_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "CLAUDE_AUTH_MODE",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

/// Prefix families that a launch profile also owns.
/// `ANTHROPIC_CUSTOM_MODEL_OPTION*` (name / description suffixes included).
/// `ANTHROPIC_DEFAULT_*_MODEL` is matched separately (prefix + `_MODEL` suffix)
/// so an unrelated `ANTHROPIC_DEFAULT_FOO` is left alone.
pub const PROFILE_OWNED_ENV_PREFIXES: &[&str] = &["ANTHROPIC_CUSTOM_MODEL_OPTION"];

/// Resolved launch profile: which directory (if any) is materialized or used
/// as `CLAUDE_CONFIG_DIR`, plus the profile-owned connection env to overlay
/// after the defense sweep. Managed profiles keep `config_dir` as the
/// materialized folder but do **not** set `CLAUDE_CONFIG_DIR`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedClaudeProfile {
    pub id: String,
    pub kind: ClaudeProfileKind,
    /// Materialized managed directory, or the user-maintained `configDir`.
    /// `None` for virtual profiles. Only `configDir` copies this into
    /// `CLAUDE_CONFIG_DIR`.
    pub config_dir: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub base_url: Option<String>,
    pub auth_token: Option<String>,
    pub model: Option<String>,
}

impl ResolvedClaudeProfile {
    fn follow_default() -> Self {
        Self {
            id: FOLLOW_DEFAULT_PROFILE_ID.to_string(),
            kind: ClaudeProfileKind::FollowDefault,
            config_dir: None,
            env: BTreeMap::new(),
            base_url: None,
            auth_token: None,
            model: None,
        }
    }

    fn official_direct() -> Self {
        Self {
            id: OFFICIAL_DIRECT_PROFILE_ID.to_string(),
            kind: ClaudeProfileKind::OfficialDirect,
            config_dir: None,
            env: BTreeMap::new(),
            base_url: None,
            auth_token: None,
            model: None,
        }
    }
}

pub fn is_virtual_profile_id(id: &str) -> bool {
    id == FOLLOW_DEFAULT_PROFILE_ID || id == OFFICIAL_DIRECT_PROFILE_ID
}

/// Single source of truth for "does this env key belong to the profile,
/// not to agent-global `env_json`?"
pub fn is_profile_owned_env_key(key: &str) -> bool {
    if PROFILE_OWNED_ENV_KEYS.contains(&key) {
        return true;
    }
    if PROFILE_OWNED_ENV_PREFIXES
        .iter()
        .any(|prefix| key.starts_with(prefix))
    {
        return true;
    }
    key.starts_with("ANTHROPIC_DEFAULT_") && key.ends_with("_MODEL")
}

/// Drop profile-owned keys from the assembled spawn env. Called only when
/// the resolved Claude profile is not `follow-default`.
pub fn strip_profile_owned_agent_env(runtime_env: &mut BTreeMap<String, String>) {
    runtime_env.retain(|k, _| !is_profile_owned_env_key(k));
}

/// Absolute path of a managed profile's materialized `settings.json`.
pub fn managed_settings_json_path(data_dir: &Path, id: &str) -> PathBuf {
    managed_config_dir(data_dir, id).join("settings.json")
}

/// Pull the managed `--settings` overlay path out of `runtime_env`.
///
/// Used at spawn so the path can ride on `session/new` `_meta` without
/// becoming a child-process environment variable.
pub fn take_claude_settings_overlay(runtime_env: &mut BTreeMap<String, String>) -> Option<PathBuf> {
    runtime_env
        .remove(CLAUDE_SETTINGS_OVERLAY_ENV_KEY)
        .map(PathBuf::from)
}

fn is_secret_env_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.contains("TOKEN") || upper.contains("KEY") || upper.contains("SECRET")
}

fn mask_env_map(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    env.iter()
        .map(|(k, v)| {
            if is_secret_env_key(k) {
                (k.clone(), mask_api_key(v))
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect()
}

fn secret_value_text(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn mask_settings_object(root: &mut serde_json::Value) -> Vec<String> {
    let Some(env) = root
        .get_mut("env")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Vec::new();
    };
    let mut masked_keys = Vec::new();
    for (key, value) in env {
        if !is_secret_env_key(key) {
            continue;
        }
        let raw = secret_value_text(value);
        if !raw.is_empty() {
            masked_keys.push(key.clone());
        }
        *value = serde_json::Value::String(mask_api_key(&raw));
    }
    masked_keys.sort();
    masked_keys
}

fn parse_settings_object(text: &str, context: &str) -> Result<serde_json::Value, AppCommandError> {
    let value = serde_json::from_str::<serde_json::Value>(text).map_err(|e| {
        AppCommandError::invalid_input(format!("{context} must be valid JSON: {e}"))
    })?;
    if !value.is_object() {
        return Err(AppCommandError::invalid_input(format!(
            "{context} must have a JSON object at the top level"
        )));
    }
    Ok(value)
}

fn pretty_settings_object(value: &serde_json::Value) -> Result<String, AppCommandError> {
    serde_json::to_string_pretty(value)
        .map_err(|e| AppCommandError::configuration_invalid(e.to_string()))
}

fn masked_settings_json(text: Option<&str>) -> Result<Option<String>, AppCommandError> {
    let Some(text) = text else {
        return Ok(None);
    };
    let mut value = parse_settings_object(text, "stored settingsJson")?;
    mask_settings_object(&mut value);
    pretty_settings_object(&value).map(Some)
}

/// Recognize exactly the shapes emitted by `mask_api_key`: either 1-8 bullets,
/// or four visible edge characters around 1-12 bullets (maximum output length
/// 20 even when the original secret was longer).
fn is_mask_api_key_shape(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    if chars.is_empty() || chars.len() > 20 {
        return false;
    }
    if chars.len() <= 8 {
        return chars.iter().all(|ch| *ch == '\u{2022}');
    }
    chars[4..chars.len() - 4].iter().all(|ch| *ch == '\u{2022}')
}

/// Resolve the `authToken` column the same way [`sanitized_settings_json`]
/// resolves a secret `env` key, so the two editors of the same credential obey
/// one rule instead of two.
///
/// | incoming | meaning |
/// | --- | --- |
/// | omitted | keep (a caller that does not manage the field at all) |
/// | the stored value's mask | keep — the field was shown and left alone |
/// | empty | clear |
/// | some other mask shape | clear, never store it as a literal |
/// | anything else | replace |
///
/// The mask case is what makes "the box is empty" trustworthy: the API only
/// ever hands back a mask, so before this the UI had to spend a blank field on
/// "keep", leaving no way at all to say "drop this token" — a profile switched
/// to the official subscription went on billing the gateway it had been given.
fn resolve_incoming_auth_token(incoming: Option<&str>, stored: Option<&str>) -> Option<String> {
    let Some(raw) = incoming else {
        return stored.map(str::to_string);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(stored) = stored {
        if trimmed == mask_api_key(stored) {
            return Some(stored.to_string());
        }
    }
    // A mask that matches nothing stored is a stale echo (a copied profile, a
    // reordered save). Storing it would mint a credential made of bullets.
    if is_mask_api_key_shape(trimmed) {
        return None;
    }
    Some(trimmed.to_string())
}

fn sanitized_settings_json(
    incoming: &str,
    existing: Option<&ClaudeProfileRecord>,
) -> Result<Option<String>, AppCommandError> {
    if incoming.trim().is_empty() {
        return Ok(None);
    }

    let mut value = parse_settings_object(incoming, "settingsJson")?;
    let existing_value = existing
        .and_then(|record| record.settings_json.as_deref())
        .map(|text| parse_settings_object(text, "stored settingsJson"))
        .transpose()?;
    let existing_env = existing_value
        .as_ref()
        .and_then(|root| root.get("env"))
        .and_then(serde_json::Value::as_object);

    if let Some(env) = value
        .get_mut("env")
        .and_then(serde_json::Value::as_object_mut)
    {
        env.retain(|key, incoming_value| {
            if !is_secret_env_key(key) {
                return true;
            }
            let Some(incoming_text) = incoming_value.as_str() else {
                return true;
            };
            if let Some(stored_value) = existing_env.and_then(|stored| stored.get(key)) {
                if incoming_text == mask_api_key(&secret_value_text(stored_value)) {
                    *incoming_value = stored_value.clone();
                    return true;
                }
            }
            !is_mask_api_key_shape(incoming_text)
        });
    }

    pretty_settings_object(&value).map(Some)
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

fn virtual_info(id: &str, label: &str, kind: ClaudeProfileKind) -> ClaudeProfileInfo {
    ClaudeProfileInfo {
        id: id.to_string(),
        label: label.to_string(),
        kind,
        config_dir: None,
        base_url: None,
        auth_token_masked: String::new(),
        model: None,
        settings_json: None,
        env: BTreeMap::new(),
        is_virtual: true,
        created_at: VIRTUAL_CREATED_AT.to_string(),
        updated_at: VIRTUAL_CREATED_AT.to_string(),
    }
}

fn follow_default_info() -> ClaudeProfileInfo {
    virtual_info(
        FOLLOW_DEFAULT_PROFILE_ID,
        "Follow default",
        ClaudeProfileKind::FollowDefault,
    )
}

fn record_to_info(record: &ClaudeProfileRecord) -> Result<ClaudeProfileInfo, AppCommandError> {
    Ok(ClaudeProfileInfo {
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
        settings_json: masked_settings_json(record.settings_json.as_deref())?,
        env: mask_env_map(&record.env),
        is_virtual: false,
        created_at: record.created_at.clone(),
        updated_at: record.updated_at.clone(),
    })
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
/// `settingsJson` is the base object. Replace semantics (idempotent): the file
/// is rewritten from the record, not merged with whatever was on disk.
///
/// Merge order: the raw settings object, then the O66 structured `record.env`
/// compatibility overlay, then dedicated `baseUrl` / `authToken` / `model`.
/// Dedicated fields win on conflict. An empty / absent dedicated field neither
/// writes nor removes its same-named key from the raw settings base.
pub fn materialize_managed_profile(
    data_dir: &Path,
    record: &ClaudeProfileRecord,
) -> Result<PathBuf, AppCommandError> {
    let dir = managed_config_dir(data_dir, &record.id);
    fs::create_dir_all(&dir).map_err(AppCommandError::io)?;

    let mut settings = match record.settings_json.as_deref() {
        Some(text) => parse_settings_object(text, "stored settingsJson")?,
        None => serde_json::json!({}),
    };
    let root = settings.as_object_mut().ok_or_else(|| {
        AppCommandError::configuration_invalid("stored settingsJson must be a JSON object")
    })?;
    let needs_env_object = !record.env.is_empty()
        || trim_non_empty(record.base_url.as_deref()).is_some()
        || trim_non_empty(record.auth_token.as_deref()).is_some()
        || trim_non_empty(record.model.as_deref()).is_some();
    if needs_env_object && !root.get("env").is_some_and(serde_json::Value::is_object) {
        root.insert("env".to_string(), serde_json::json!({}));
    }
    if let Some(env) = root
        .get_mut("env")
        .and_then(serde_json::Value::as_object_mut)
    {
        for (key, value) in &record.env {
            if let Some(trimmed) = trim_non_empty(Some(value)) {
                env.insert(key.clone(), serde_json::Value::String(trimmed));
            }
        }
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
    }
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
        ClaudeProfileKind::FollowDefault | ClaudeProfileKind::OfficialDirect => Ok(None),
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
    if id == OFFICIAL_DIRECT_PROFILE_ID {
        return Ok(ResolvedClaudeProfile::official_direct());
    }
    match read_record(data_dir, id) {
        Ok(Some(record)) => {
            let config_dir = config_dir_for_record(data_dir, &record)?;
            Ok(ResolvedClaudeProfile {
                id: record.id,
                kind: record.kind,
                config_dir,
                env: record.env,
                base_url: record.base_url,
                auth_token: record.auth_token,
                model: record.model,
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

/// Overlay the profile's own connection env after the defense sweep.
///
/// Used by `official-direct` (forced official URL + empty tokens) and by
/// `configDir` profiles (`record.env`; they have no `--settings` file).
/// Managed profiles do **not** call this: dedicated fields and `record.env`
/// live in the materialized `settings.json` that `--settings` loads.
fn apply_profile_connection_env(
    resolved: &ResolvedClaudeProfile,
    runtime_env: &mut BTreeMap<String, String>,
) {
    if resolved.id == OFFICIAL_DIRECT_PROFILE_ID {
        // Monet official-direct: force the official endpoint, then empty-string
        // the token (and API key) so the spawn layer `env_remove`s inherited
        // values and the CLI falls back to config-dir OAuth. Same empty-sentinel
        // as `apply_claude_env_policy`.
        runtime_env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            OFFICIAL_ANTHROPIC_BASE_URL.to_string(),
        );
        runtime_env.insert("ANTHROPIC_AUTH_TOKEN".to_string(), String::new());
        runtime_env.insert("ANTHROPIC_API_KEY".to_string(), String::new());
        return;
    }

    for (key, value) in &resolved.env {
        if let Some(trimmed) = trim_non_empty(Some(value)) {
            runtime_env.insert(key.clone(), trimmed);
        }
    }
}

/// Last step of `build_session_runtime_env` for Claude Code.
///
/// `follow-default`: today's behaviour — no sweep, no `CLAUDE_CONFIG_DIR`,
/// no `_meta` overlay.
/// Anything else: strip `PROFILE_OWNED_ENV_KEYS` (and prefix families) so
/// agent-global connection env cannot override the profile, then empty-sentinel
/// the three credential keys (`env_remove`).
///
/// `managed`: stash the materialized `settings.json` absolute path on
/// [`CLAUDE_SETTINGS_OVERLAY_ENV_KEY`] (stripped before spawn) and do **not**
/// set `CLAUDE_CONFIG_DIR` or re-inject baseUrl/token/model/`record.env`.
/// `configDir`: keep `CLAUDE_CONFIG_DIR` + `record.env` injection.
/// A user-explicit `CLAUDE_CONFIG_DIR` in `env_json` still wins for
/// `configDir` and is warned.
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
    if resolved.id == FOLLOW_DEFAULT_PROFILE_ID {
        return Ok(());
    }

    strip_profile_owned_agent_env(runtime_env);
    // A selected profile is the authentication-mode boundary. Empty values are
    // not injected into the child: sacp-tokio translates them to `env_remove`,
    // preventing credentials exported by codeg's parent shell/container from
    // overriding the selected settings. This replaces the old
    // CLAUDE_AUTH_MODE=official_subscription launch policy without carrying
    // that codeg-only selector into the profile.
    for key in [
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
    ] {
        runtime_env.insert(key.to_string(), String::new());
    }

    match resolved.kind {
        ClaudeProfileKind::FollowDefault => {}
        ClaudeProfileKind::OfficialDirect => {
            apply_profile_connection_env(&resolved, runtime_env);
        }
        ClaudeProfileKind::Managed => {
            if let Some(dir) = &resolved.config_dir {
                runtime_env.insert(
                    CLAUDE_SETTINGS_OVERLAY_ENV_KEY.to_string(),
                    dir.join("settings.json").to_string_lossy().into_owned(),
                );
            }
        }
        ClaudeProfileKind::ConfigDir => {
            apply_profile_connection_env(&resolved, runtime_env);
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
        }
    }
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
                if is_virtual_profile_id(&record.id) {
                    continue;
                }
                match record_to_info(&record) {
                    Ok(info) => out.push(info),
                    Err(e) => {
                        tracing::warn!(
                            "[claude-profile] skip invalid settingsJson in {}: {e}",
                            path.display()
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!("[claude-profile] skip invalid {}: {e}", path.display());
            }
        }
    }
    Ok(out)
}

fn default_claude_settings_path() -> PathBuf {
    crate::parsers::claude::resolve_claude_config_dir().join("settings.json")
}

/// Read a Claude settings file without ever writing it. Invalid JSON is
/// returned verbatim so the frontend can show the user what needs repair.
pub fn claude_settings_read_core(
    path: Option<String>,
) -> Result<ClaudeSettingsReadResult, AppCommandError> {
    let path = path
        .map(PathBuf::from)
        .unwrap_or_else(default_claude_settings_path);
    let rendered_path = path.to_string_lossy().into_owned();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ClaudeSettingsReadResult {
                path: rendered_path,
                text: String::new(),
                exists: false,
                dropped_secret_keys: Vec::new(),
            });
        }
        Err(error) => return Err(AppCommandError::io(error)),
    };

    let (text, dropped_secret_keys) = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(mut value) if value.is_object() => {
            let dropped = mask_settings_object(&mut value);
            if dropped.is_empty() {
                (raw, dropped)
            } else {
                (pretty_settings_object(&value)?, dropped)
            }
        }
        // A malformed file (or a valid non-object JSON value) is a read-only
        // preview: preserve its exact bytes for diagnosis.
        _ => (raw, Vec::new()),
    };

    Ok(ClaudeSettingsReadResult {
        path: rendered_path,
        text,
        exists: true,
        dropped_secret_keys,
    })
}

fn next_imported_profile_id(data_dir: &Path) -> String {
    for suffix in 1usize.. {
        let id = if suffix == 1 {
            "imported".to_string()
        } else {
            format!("imported-{suffix}")
        };
        if !profile_record_path(data_dir, &id).exists()
            && !managed_config_dir(data_dir, &id).exists()
        {
            return id;
        }
    }
    unreachable!("usize profile suffix space is finite but cannot be exhausted in practice")
}

fn imported_profile_label(base_url: Option<&str>) -> String {
    base_url
        .and_then(|raw| reqwest::Url::parse(raw).ok())
        .and_then(|url| url.host_str().map(str::to_string))
        .filter(|host| !host.trim().is_empty())
        .unwrap_or_else(|| "Imported".to_string())
}

/// Move Claude's legacy agent-global connection env into one managed profile.
/// The profile file is durably written before the single DB-row update removes
/// anything, so a write failure cannot destroy the user's connection settings.
/// Returns the created profile id, or `None` when no migration was needed.
pub async fn migrate_claude_profile_owned_env(
    db: &AppDatabase,
    data_dir: &Path,
) -> Result<Option<String>, AppCommandError> {
    let Some(setting) = agent_setting_service::get_by_agent_type(&db.conn, AgentType::ClaudeCode)
        .await
        .map_err(AppCommandError::from)?
    else {
        return Ok(None);
    };
    let mut env: BTreeMap<String, String> = match setting.env_json.as_deref() {
        Some(raw) if !raw.trim().is_empty() => serde_json::from_str(raw).map_err(|error| {
            AppCommandError::configuration_invalid(format!(
                "cannot migrate Claude agent env_json because it is invalid: {error}"
            ))
        })?,
        _ => BTreeMap::new(),
    };
    if env
        .get(CODEG_CLAUDE_PROFILE_MIGRATED_ENV_KEY)
        .is_some_and(|value| value == "1")
    {
        return Ok(None);
    }
    if !env.keys().any(|key| is_profile_owned_env_key(key)) {
        return Ok(None);
    }

    let id = next_imported_profile_id(data_dir);
    let base_url = env.get("ANTHROPIC_BASE_URL").cloned();
    let auth_token = env.get("ANTHROPIC_AUTH_TOKEN").cloned();
    let model = env.get("ANTHROPIC_MODEL").cloned();
    let mut imported_env = serde_json::Map::new();
    for (key, value) in &env {
        if is_profile_owned_env_key(key)
            && key != "ANTHROPIC_BASE_URL"
            && key != "ANTHROPIC_AUTH_TOKEN"
            && key != "ANTHROPIC_MODEL"
            && key != "CLAUDE_AUTH_MODE"
        {
            imported_env.insert(key.clone(), serde_json::Value::String(value.clone()));
        }
    }
    let settings_json = pretty_settings_object(&serde_json::json!({ "env": imported_env }))?;
    claude_profile_upsert_core(
        data_dir,
        ClaudeProfileUpsert {
            id: id.clone(),
            label: imported_profile_label(base_url.as_deref()),
            kind: ClaudeProfileKind::Managed,
            config_dir: None,
            base_url,
            auth_token,
            model,
            settings_json: Some(settings_json),
            env: None,
        },
    )?;

    env.retain(|key, _| !is_profile_owned_env_key(key));
    env.insert(CODEG_CLAUDE_PROFILE_ENV_KEY.to_string(), id.clone());
    env.insert(
        CODEG_CLAUDE_PROFILE_MIGRATED_ENV_KEY.to_string(),
        "1".to_string(),
    );
    let env_json = serde_json::to_string(&env)
        .map_err(|error| AppCommandError::configuration_invalid(error.to_string()))?;
    agent_setting_service::update(
        &db.conn,
        AgentType::ClaudeCode,
        agent_setting_service::AgentSettingsUpdate {
            enabled: setting.enabled,
            env_json: Some(env_json),
            model_provider_id: setting.model_provider_id,
        },
    )
    .await
    .map_err(AppCommandError::from)?;

    Ok(Some(id))
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
        ClaudeProfileKind::OfficialDirect => {
            "official-direct: force ANTHROPIC_BASE_URL=https://api.anthropic.com \
             and clear ANTHROPIC_AUTH_TOKEN so the CLI falls back to config-dir OAuth"
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
    if is_virtual_profile_id(&input.id) {
        return Err(AppCommandError::invalid_input(format!(
            "profile id '{}' is reserved",
            input.id
        )));
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
        ClaudeProfileKind::OfficialDirect => {
            return Err(AppCommandError::invalid_input(
                "cannot persist the virtual official-direct profile",
            ));
        }
        ClaudeProfileKind::ConfigDir => {
            if input.settings_json.is_some() {
                return Err(AppCommandError::invalid_input(
                    "settingsJson is only valid for kind=managed; configDir profiles use the user's own directory",
                ));
            }
            let dir = trim_non_empty(input.config_dir.as_deref()).ok_or_else(|| {
                AppCommandError::invalid_input("configDir is required for kind=configDir")
            })?;
            if !Path::new(&dir).is_absolute() {
                return Err(AppCommandError::invalid_input(
                    "configDir must be an absolute path",
                ));
            }
        }
        ClaudeProfileKind::Managed => {
            if let Some(raw) = input.settings_json.as_deref() {
                if !raw.trim().is_empty() {
                    parse_settings_object(raw, "settingsJson")?;
                }
            }
        }
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
    let auth_token = resolve_incoming_auth_token(
        input.auth_token.as_deref(),
        existing.as_ref().and_then(|r| r.auth_token.as_deref()),
    );
    let settings_json = match input.settings_json.as_deref() {
        None => existing
            .as_ref()
            .and_then(|record| record.settings_json.clone()),
        Some(raw) => sanitized_settings_json(raw, existing.as_ref())?,
    };
    // Whole-map replace: omitted keeps the stored map; Some (including empty)
    // replaces it.
    let env = match input.env {
        None => existing.as_ref().map(|r| r.env.clone()).unwrap_or_default(),
        Some(map) => map,
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
        settings_json: match input.kind {
            ClaudeProfileKind::Managed => settings_json,
            _ => None,
        },
        env,
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
    record_to_info(&record)
}

/// Read-only counterpart of `conversation_set_claude_profile_core`: which
/// profile would this conversation actually launch with.
///
/// Resolves the same way a spawn does (`resolve_claude_profile`), so the chip
/// in the composer can show the profile in force rather than guessing. It used
/// to guess — it defaulted to `follow-default` on every mount, so reloading the
/// app made every session claim to be following the CLI no matter what it was
/// bound to, and a user could not tell which endpoint a session was billing.
///
/// `affected_running_sessions` is always 0: reading changes nothing. The shape
/// is shared with the setter so the two cannot describe the binding differently.
pub async fn conversation_get_claude_profile_core(
    db: &AppDatabase,
    data_dir: &Path,
    conversation_id: i32,
) -> Result<ConversationClaudeProfileResult, AppCommandError> {
    let setting = agent_setting_service::get_by_agent_type(&db.conn, AgentType::ClaudeCode)
        .await
        .map_err(AppCommandError::from)?;
    let resolved = resolve_claude_profile(
        db,
        data_dir,
        Some(conversation_id),
        setting.as_ref().and_then(|m| m.env_json.as_deref()),
    )
    .await?;
    Ok(ConversationClaudeProfileResult {
        conversation_id,
        profile_id: Some(resolved.id),
        affected_running_sessions: 0,
    })
}

pub fn claude_profile_delete_core(data_dir: &Path, id: &str) -> Result<(), AppCommandError> {
    if is_virtual_profile_id(id) {
        return Err(AppCommandError::invalid_input(format!(
            "cannot delete the virtual {id} profile"
        )));
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
        Some(id) if is_virtual_profile_id(id) => Some(id.to_string()),
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
pub fn claude_settings_read(
    path: Option<String>,
) -> Result<ClaudeSettingsReadResult, AppCommandError> {
    claude_settings_read_core(path)
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn conversation_get_claude_profile(
    db: tauri::State<'_, AppDatabase>,
    app: tauri::AppHandle,
    conversation_id: i32,
) -> Result<ConversationClaudeProfileResult, AppCommandError> {
    conversation_get_claude_profile_core(&db, &tauri_data_dir(&app), conversation_id).await
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
pub struct ClaudeSettingsReadParams {
    pub path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSetClaudeProfileParams {
    pub conversation_id: i32,
    pub profile_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationGetClaudeProfileParams {
    pub conversation_id: i32,
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
                settings_json: None,
                env: None,
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
                settings_json: None,
                env: None,
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
                settings_json: None,
                env: None,
            },
        )
        .unwrap_err();
        assert!(err.message.contains("reserved"), "{}", err.message);
        let err = claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: OFFICIAL_DIRECT_PROFILE_ID.to_string(),
                label: "nope".into(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: None,
                auth_token: None,
                model: None,
                settings_json: None,
                env: None,
            },
        )
        .unwrap_err();
        assert!(err.message.contains("reserved"), "{}", err.message);
        let err = claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "ok".into(),
                label: "nope".into(),
                kind: ClaudeProfileKind::OfficialDirect,
                config_dir: None,
                base_url: None,
                auth_token: None,
                model: None,
                settings_json: None,
                env: None,
            },
        )
        .unwrap_err();
        assert!(err.message.contains("official-direct"), "{}", err.message);
        let err = claude_profile_delete_core(data.path(), OFFICIAL_DIRECT_PROFILE_ID).unwrap_err();
        assert!(err.message.contains("virtual"), "{}", err.message);
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
                settings_json: None,
                env: None,
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
        assert!(list[0].is_virtual);
        assert!(list[0].auth_token_masked.is_empty());
        assert!(
            list.iter()
                .all(|profile| profile.id != OFFICIAL_DIRECT_PROFILE_ID),
            "official-direct is retired from discovery"
        );
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

    #[test]
    fn managed_settings_json_is_base_and_dedicated_fields_win_without_deleting_base_keys() {
        let data = tempfile::tempdir().unwrap();
        let base = serde_json::json!({
            "permissions": { "allow": ["Read"] },
            "env": {
                "ANTHROPIC_BASE_URL": "https://base.example/v1",
                "ANTHROPIC_AUTH_TOKEN": "sk-base-secret",
                "ANTHROPIC_MODEL": "base-model",
                "ANTHROPIC_DEFAULT_FABLE_MODEL": "fable-base"
            }
        });
        let info = claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "base".into(),
                label: "Base".into(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: Some("https://dedicated.example/v1".into()),
                auth_token: None,
                model: None,
                settings_json: Some(serde_json::to_string_pretty(&base).unwrap()),
                env: None,
            },
        )
        .unwrap();

        let masked: serde_json::Value =
            serde_json::from_str(info.settings_json.as_deref().unwrap()).unwrap();
        assert_ne!(
            masked["env"]["ANTHROPIC_AUTH_TOKEN"],
            serde_json::json!("sk-base-secret")
        );
        let materialized: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(managed_config_dir(data.path(), "base").join("settings.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(materialized["permissions"]["allow"][0], "Read");
        assert_eq!(
            materialized["env"]["ANTHROPIC_BASE_URL"],
            "https://dedicated.example/v1"
        );
        assert_eq!(
            materialized["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-base-secret",
            "absent dedicated token must not delete the base key"
        );
        assert_eq!(materialized["env"]["ANTHROPIC_MODEL"], "base-model");
        assert_eq!(
            materialized["env"]["ANTHROPIC_DEFAULT_FABLE_MODEL"],
            "fable-base"
        );
    }

    #[test]
    fn settings_json_validation_and_config_dir_rejection_are_frontend_readable() {
        let data = tempfile::tempdir().unwrap();
        for (raw, needle) in [
            ("{broken", "must be valid JSON"),
            (r#"["not", "an", "object"]"#, "top level"),
        ] {
            let err = claude_profile_upsert_core(
                data.path(),
                ClaudeProfileUpsert {
                    id: "bad".into(),
                    label: "Bad".into(),
                    kind: ClaudeProfileKind::Managed,
                    config_dir: None,
                    base_url: None,
                    auth_token: None,
                    model: None,
                    settings_json: Some(raw.into()),
                    env: None,
                },
            )
            .unwrap_err();
            assert!(err.message.contains(needle), "{}", err.message);
        }

        let err = claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "user".into(),
                label: "User".into(),
                kind: ClaudeProfileKind::ConfigDir,
                config_dir: Some(data.path().to_string_lossy().into_owned()),
                base_url: None,
                auth_token: None,
                model: None,
                settings_json: Some("{}".into()),
                env: None,
            },
        )
        .unwrap_err();
        assert!(err.message.contains("only valid for kind=managed"));
    }

    #[test]
    fn masked_settings_secret_round_trip_preserves_stored_plaintext() {
        let data = tempfile::tempdir().unwrap();
        claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "secret".into(),
                label: "Secret".into(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: None,
                auth_token: None,
                model: None,
                settings_json: Some(
                    r#"{"env":{"MY_GATEWAY_SECRET":"secret-value-12345678"}}"#.into(),
                ),
                env: None,
            },
        )
        .unwrap();
        let masked = claude_profile_list_core(data.path())
            .unwrap()
            .into_iter()
            .find(|profile| profile.id == "secret")
            .unwrap()
            .settings_json
            .unwrap();
        assert!(masked.contains('\u{2022}'));
        assert!(!masked.contains("secret-value-12345678"));

        claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "secret".into(),
                label: "Secret renamed".into(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: None,
                auth_token: None,
                model: None,
                settings_json: Some(masked),
                env: None,
            },
        )
        .unwrap();
        let stored = read_record(data.path(), "secret").unwrap().unwrap();
        assert!(stored
            .settings_json
            .as_deref()
            .unwrap()
            .contains("secret-value-12345678"));
    }

    #[test]
    fn settings_json_omit_and_null_keep_while_empty_string_clears() {
        let data = tempfile::tempdir().unwrap();
        claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "patch".into(),
                label: "Patch".into(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: None,
                auth_token: None,
                model: None,
                settings_json: Some(r#"{"theme":"dark"}"#.into()),
                env: None,
            },
        )
        .unwrap();

        for wire in [
            serde_json::json!({ "id": "patch", "label": "Patch", "kind": "managed" }),
            serde_json::json!({
                "id": "patch",
                "label": "Patch",
                "kind": "managed",
                "settingsJson": null
            }),
        ] {
            let payload: ClaudeProfileUpsert = serde_json::from_value(wire).unwrap();
            assert!(payload.settings_json.is_none());
            let info = claude_profile_upsert_core(data.path(), payload).unwrap();
            assert!(info.settings_json.as_deref().unwrap().contains("dark"));
        }

        let cleared = claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "patch".into(),
                label: "Patch".into(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: None,
                auth_token: None,
                model: None,
                settings_json: Some(String::new()),
                env: None,
            },
        )
        .unwrap();
        assert!(cleared.settings_json.is_none());
        let materialized: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(managed_config_dir(data.path(), "patch").join("settings.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(materialized, serde_json::json!({}));
    }

    #[test]
    fn new_profile_drops_mask_shaped_settings_secrets() {
        let data = tempfile::tempdir().unwrap();
        claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "import".into(),
                label: "Import".into(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: None,
                auth_token: None,
                model: None,
                settings_json: Some(
                    r#"{"env":{"ANTHROPIC_API_KEY":"sk-a••••••••••••7890","KEEP":"yes"}}"#.into(),
                ),
                env: None,
            },
        )
        .unwrap();
        let stored = read_record(data.path(), "import").unwrap().unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(stored.settings_json.as_deref().unwrap()).unwrap();
        assert!(parsed["env"].get("ANTHROPIC_API_KEY").is_none());
        assert_eq!(parsed["env"]["KEEP"], "yes");
    }

    #[test]
    fn settings_read_masks_secrets_reports_drops_and_preserves_invalid_text() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join("settings.json");
        fs::write(
            &path,
            r#"{"theme":"dark","env":{"ANTHROPIC_API_KEY":"sk-import-1234567890","NORMAL":"keep"}}"#,
        )
        .unwrap();
        let result = claude_settings_read_core(Some(path.to_string_lossy().into_owned())).unwrap();
        assert!(result.exists);
        assert_eq!(result.dropped_secret_keys, vec!["ANTHROPIC_API_KEY"]);
        assert!(!result.text.contains("sk-import-1234567890"));
        assert!(result.text.contains('\u{2022}'));
        assert!(result.text.contains("\"NORMAL\": \"keep\""));

        let invalid = "{ this is the broken original";
        fs::write(&path, invalid).unwrap();
        let result = claude_settings_read_core(Some(path.to_string_lossy().into_owned())).unwrap();
        assert!(result.exists);
        assert_eq!(result.text, invalid);
        assert!(result.dropped_secret_keys.is_empty());

        fs::remove_file(&path).unwrap();
        let result = claude_settings_read_core(Some(path.to_string_lossy().into_owned())).unwrap();
        assert!(!result.exists);
        assert!(result.text.is_empty());
        assert!(result.dropped_secret_keys.is_empty());
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
        assert!(
            !env.contains_key(CLAUDE_SETTINGS_OVERLAY_ENV_KEY),
            "follow-default must not set a --settings overlay: {env:?}"
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
        let expected = managed_settings_json_path(data.path(), "hosted");
        assert!(
            !env.contains_key("CLAUDE_CONFIG_DIR"),
            "managed must not set CLAUDE_CONFIG_DIR: {env:?}"
        );
        assert_eq!(
            env.get(CLAUDE_SETTINGS_OVERLAY_ENV_KEY).map(String::as_str),
            Some(expected.to_string_lossy().as_ref())
        );
        assert!(expected.is_file());
    }

    #[tokio::test]
    async fn managed_overlay_path_keeps_spaces_and_non_ascii_verbatim() {
        use crate::commands::acp::build_session_runtime_env;
        let db = fresh_in_memory_db().await;
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("Claude Profiles").join("用户 档");
        fs::create_dir_all(&data_dir).unwrap();
        seed_claude_agent(&db, None).await;
        let folder = seed_folder(&db, data_dir.to_str().unwrap()).await;
        let conv = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        upsert_managed(
            &data_dir,
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
            build_session_runtime_env(&db, AgentType::ClaudeCode, None, &data_dir, Some(conv))
                .await
                .unwrap();
        let overlay = managed_settings_json_path(&data_dir, "hosted");
        let stored = env
            .get(CLAUDE_SETTINGS_OVERLAY_ENV_KEY)
            .cloned()
            .expect("managed overlay path");
        let expected = overlay.to_string_lossy().into_owned();
        assert_eq!(stored, expected);
        assert!(
            stored.contains(' ') && stored.contains('档'),
            "path must keep spaces and non-ASCII: {stored}"
        );
        assert!(
            !stored.starts_with('"') && !stored.ends_with('"'),
            "path must not be quote-wrapped: {stored}"
        );
        assert!(!env.contains_key("CLAUDE_CONFIG_DIR"));

        let mut child_env = env.clone();
        let taken = take_claude_settings_overlay(&mut child_env);
        assert_eq!(taken.as_deref(), Some(overlay.as_path()));
        assert!(
            !child_env.contains_key(CLAUDE_SETTINGS_OVERLAY_ENV_KEY),
            "overlay key must be stripped before the child spawn: {child_env:?}"
        );
    }

    #[test]
    fn take_claude_settings_overlay_is_a_remove_not_a_read() {
        let mut env = BTreeMap::new();
        env.insert("KEEP".to_string(), "1".to_string());
        assert_eq!(take_claude_settings_overlay(&mut env), None);
        assert_eq!(env.get("KEEP").map(String::as_str), Some("1"));

        let path = PathBuf::from("/tmp/Claude Profiles/档/settings.json");
        env.insert(
            CLAUDE_SETTINGS_OVERLAY_ENV_KEY.to_string(),
            path.to_string_lossy().into_owned(),
        );
        assert_eq!(take_claude_settings_overlay(&mut env), Some(path));
        assert!(!env.contains_key(CLAUDE_SETTINGS_OVERLAY_ENV_KEY));
        assert_eq!(env.get("KEEP").map(String::as_str), Some("1"));
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
        assert!(
            !env.contains_key(CLAUDE_SETTINGS_OVERLAY_ENV_KEY),
            "Codex must not receive a Claude --settings overlay: {env:?}"
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

    // The column now obeys the rule `sanitized_settings_json` already applied
    // to secret env keys. Before this there was no way to say "drop the
    // token": blank meant keep, so a profile moved to the official
    // subscription kept billing its gateway.
    #[test]
    fn auth_token_column_follows_the_settings_json_mask_rule() {
        let stored = "sk-ant-abcdefghijklmnop";
        let mask = mask_api_key(stored);

        assert_eq!(
            resolve_incoming_auth_token(None, Some(stored)).as_deref(),
            Some(stored),
            "omitted means the caller does not manage the field"
        );
        assert_eq!(
            resolve_incoming_auth_token(Some(&mask), Some(stored)).as_deref(),
            Some(stored),
            "an untouched mask must round-trip to the secret it stands for"
        );
        assert_eq!(
            resolve_incoming_auth_token(Some(""), Some(stored)),
            None,
            "an emptied field is the only way to drop a credential"
        );
        assert_eq!(
            resolve_incoming_auth_token(Some("   "), Some(stored)),
            None,
            "whitespace is empty"
        );
        assert_eq!(
            resolve_incoming_auth_token(Some("sk-ant-brandnewvalue"), Some(stored)).as_deref(),
            Some("sk-ant-brandnewvalue")
        );
        // A mask belonging to some other profile (a copy, a stale form) must
        // never be stored as if it were the secret.
        assert_eq!(
            resolve_incoming_auth_token(Some(&mask_api_key("sk-ant-somethingelse")), Some(stored)),
            None
        );
        assert_eq!(
            resolve_incoming_auth_token(Some(&mask), None),
            None,
            "a mask with nothing stored behind it resolves to nothing"
        );
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
        assert!(!err.contains("official-direct"), "{err}");
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
        assert!(follow.is_virtual);

        assert!(
            list.iter().all(|p| p.id != OFFICIAL_DIRECT_PROFILE_ID),
            "retired virtual profile must not be advertised"
        );

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
        assert!(!config_dir.is_virtual);
    }

    fn connection_and_misc_env_json() -> (String, BTreeMap<String, String>) {
        let map = BTreeMap::from([
            (
                "ANTHROPIC_AUTH_TOKEN".to_string(),
                "sk-global-o66".to_string(),
            ),
            ("ANTHROPIC_API_KEY".to_string(), "sk-api-o66".to_string()),
            (
                "ANTHROPIC_BASE_URL".to_string(),
                "https://relay.example/v1".to_string(),
            ),
            ("ANTHROPIC_MODEL".to_string(), "claude-sonnet".to_string()),
            (
                "ANTHROPIC_CUSTOM_MODEL_OPTION".to_string(),
                "custom-opt".to_string(),
            ),
            (
                "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME".to_string(),
                "Custom".to_string(),
            ),
            (
                "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
                "sonnet-x".to_string(),
            ),
            ("CLAUDE_AUTH_MODE".to_string(), "custom".to_string()),
            ("CLAUDE_CODE_USE_BEDROCK".to_string(), "1".to_string()),
            ("CLAUDE_CODE_USE_VERTEX".to_string(), "1".to_string()),
            ("CLAUDE_CODE_USE_FOUNDRY".to_string(), "1".to_string()),
            (
                "CODEG_O66_MISC".to_string(),
                "keep-non-connection".to_string(),
            ),
            ("DISABLE_TELEMETRY".to_string(), "1".to_string()),
            ("ENABLE_TOOL_SEARCH".to_string(), "1".to_string()),
            ("CLAUDE_CODE_SCROLL_SPEED".to_string(), "2".to_string()),
        ]);
        (serde_json::to_string(&map).unwrap(), map)
    }

    const OWNED_SAMPLE_KEYS: &[&str] = &[
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ANTHROPIC_CUSTOM_MODEL_OPTION",
        "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "CLAUDE_AUTH_MODE",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ];

    // Unique key that will not appear in the developer's ~/.claude/settings.json,
    // so value equality is hermetic. Native overlays (GIT_BASH_PATH,
    // ENABLE_TOOL_SEARCH, …) are compared follow-vs-swept, not vs env_json.
    const MISC_SAMPLE_KEYS: &[&str] = &["CODEG_O66_MISC"];

    #[test]
    fn profile_owned_key_table_matches_predicate() {
        for key in PROFILE_OWNED_ENV_KEYS {
            assert!(is_profile_owned_env_key(key), "{key}");
        }
        assert!(is_profile_owned_env_key(
            "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME"
        ));
        assert!(is_profile_owned_env_key(
            "ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION"
        ));
        assert!(is_profile_owned_env_key("ANTHROPIC_DEFAULT_SONNET_MODEL"));
        assert!(is_profile_owned_env_key("ANTHROPIC_DEFAULT_HAIKU_MODEL"));
        assert!(is_profile_owned_env_key("ANTHROPIC_DEFAULT_FABLE_MODEL"));
        assert!(!is_profile_owned_env_key("ANTHROPIC_DEFAULT_FOO"));
        assert!(!is_profile_owned_env_key("ANTHROPIC_REASONING_MODEL"));
        assert!(!is_profile_owned_env_key("CLAUDE_CODE_GIT_BASH_PATH"));
        assert!(!is_profile_owned_env_key("CODEG_O66_MISC"));
        assert!(!is_profile_owned_env_key("DISABLE_TELEMETRY"));
        assert!(!is_profile_owned_env_key("ENABLE_TOOL_SEARCH"));
        assert!(!is_profile_owned_env_key("CLAUDE_CODE_SCROLL_SPEED"));
        assert!(!is_profile_owned_env_key("CLAUDE_CONFIG_DIR"));
    }

    #[test]
    fn old_profile_json_without_env_or_settings_json_deserializes() {
        let raw = r#"{
            "id": "legacy",
            "label": "Legacy",
            "kind": "managed",
            "baseUrl": "https://old.example/v1",
            "createdAt": "2026-08-21T00:00:00Z",
            "updatedAt": "2026-08-21T00:00:00Z"
        }"#;
        let record: ClaudeProfileRecord = serde_json::from_str(raw).unwrap();
        assert!(record.env.is_empty());
        assert!(record.settings_json.is_none());
        assert_eq!(record.id, "legacy");
        assert_eq!(record.kind, ClaudeProfileKind::Managed);
        assert_eq!(record.base_url.as_deref(), Some("https://old.example/v1"));
    }

    #[test]
    fn upsert_env_omit_keeps_empty_map_replaces_and_masks_secrets() {
        let data = tempfile::tempdir().unwrap();
        let mut env = BTreeMap::new();
        env.insert(
            "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
            "haiku-x".to_string(),
        );
        env.insert(
            "MY_GATEWAY_TOKEN".to_string(),
            "secret-value-12345678".to_string(),
        );
        let info = claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "api".to_string(),
                label: "API".to_string(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: Some("https://example.test/v1".into()),
                auth_token: None,
                model: None,
                settings_json: None,
                env: Some(env),
            },
        )
        .unwrap();
        assert_eq!(
            info.env
                .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .map(String::as_str),
            Some("haiku-x")
        );
        let masked = info.env.get("MY_GATEWAY_TOKEN").expect("masked token");
        assert_ne!(masked, "secret-value-12345678");
        assert!(masked.contains('\u{2022}'), "{masked}");

        let kept = upsert_managed(
            data.path(),
            "api",
            Some("https://example.test/v1"),
            None,
            None,
        );
        assert_eq!(
            kept.env
                .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .map(String::as_str),
            Some("haiku-x"),
            "omitted env must keep the stored map"
        );

        let cleared = claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "api".to_string(),
                label: "API".to_string(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: Some("https://example.test/v1".into()),
                auth_token: None,
                model: None,
                settings_json: None,
                env: Some(BTreeMap::new()),
            },
        )
        .unwrap();
        assert!(
            cleared.env.is_empty(),
            "empty map must replace (clear) stored env"
        );
    }

    #[test]
    fn managed_env_loses_to_dedicated_fields_on_conflict() {
        let data = tempfile::tempdir().unwrap();
        let mut env = BTreeMap::new();
        env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            "https://from-env.example/v1".to_string(),
        );
        env.insert(
            "ANTHROPIC_AUTH_TOKEN".to_string(),
            "sk-from-env".to_string(),
        );
        env.insert("ANTHROPIC_MODEL".to_string(), "from-env-model".to_string());
        env.insert(
            "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
            "haiku-from-env".to_string(),
        );
        claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "gw".to_string(),
                label: "GW".to_string(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: Some("https://from-field.example/v1".into()),
                auth_token: Some("sk-from-field".into()),
                model: Some("from-field-model".into()),
                settings_json: None,
                env: Some(env),
            },
        )
        .unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(managed_config_dir(data.path(), "gw").join("settings.json"))
                .unwrap(),
        )
        .unwrap();
        let block = settings.get("env").and_then(|v| v.as_object()).unwrap();
        assert_eq!(
            block.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()),
            Some("https://from-field.example/v1")
        );
        assert_eq!(
            block.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()),
            Some("sk-from-field")
        );
        assert_eq!(
            block.get("ANTHROPIC_MODEL").and_then(|v| v.as_str()),
            Some("from-field-model")
        );
        assert_eq!(
            block
                .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(|v| v.as_str()),
            Some("haiku-from-env")
        );
    }

    #[tokio::test]
    async fn follow_default_keeps_agent_connection_env_key_for_key() {
        use crate::commands::acp::build_session_runtime_env;
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        let (env_json, expected) = connection_and_misc_env_json();
        seed_claude_agent(&db, Some(&env_json)).await;
        let env = build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), None)
            .await
            .unwrap();
        for key in OWNED_SAMPLE_KEYS.iter().chain(MISC_SAMPLE_KEYS) {
            assert_eq!(
                env.get(*key),
                expected.get(*key),
                "follow-default must keep {key}"
            );
        }
        assert!(!env.contains_key("CLAUDE_CONFIG_DIR"));
        assert!(!env.contains_key(CLAUDE_SETTINGS_OVERLAY_ENV_KEY));
    }

    #[tokio::test]
    async fn non_follow_default_sweeps_owned_keys_and_keeps_misc() {
        use crate::commands::acp::build_session_runtime_env;
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        let (env_json, expected) = connection_and_misc_env_json();
        seed_claude_agent(&db, Some(&env_json)).await;
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

        let follow = build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), None)
            .await
            .unwrap();
        let swept =
            build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), Some(conv))
                .await
                .unwrap();

        for key in OWNED_SAMPLE_KEYS {
            assert!(
                follow.contains_key(*key),
                "follow-default regression lost {key}"
            );
            if [
                "ANTHROPIC_BASE_URL",
                "ANTHROPIC_AUTH_TOKEN",
                "ANTHROPIC_API_KEY",
            ]
            .contains(key)
            {
                assert_eq!(
                    swept.get(*key).map(String::as_str),
                    Some(""),
                    "configDir must env_remove inherited credential {key}: {swept:?}"
                );
            } else {
                assert!(
                    !swept.contains_key(*key),
                    "configDir profile must not inject owned key {key}: {swept:?}"
                );
            }
        }
        for key in MISC_SAMPLE_KEYS {
            assert_eq!(
                swept.get(*key),
                follow.get(*key),
                "non-connection key {key} must survive the sweep"
            );
            assert_eq!(swept.get(*key), expected.get(*key));
        }
    }

    #[tokio::test]
    async fn official_direct_forces_official_url_and_clears_token() {
        use crate::commands::acp::build_session_runtime_env;
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        let (env_json, expected) = connection_and_misc_env_json();
        seed_claude_agent(&db, Some(&env_json)).await;
        let folder = seed_folder(&db, data.path().to_str().unwrap()).await;
        let conv = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        conversation_service::merge_selector_config_value(
            &db.conn,
            conv,
            PREFERRED_PROFILE_CONFIG_KEY,
            OFFICIAL_DIRECT_PROFILE_ID,
        )
        .await
        .unwrap();

        let env =
            build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), Some(conv))
                .await
                .unwrap();
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL").map(String::as_str),
            Some(OFFICIAL_ANTHROPIC_BASE_URL)
        );
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").map(String::as_str),
            Some("")
        );
        assert_eq!(env.get("ANTHROPIC_API_KEY").map(String::as_str), Some(""));
        assert!(!env.contains_key("CLAUDE_CONFIG_DIR"));
        assert!(!env.contains_key(CLAUDE_SETTINGS_OVERLAY_ENV_KEY));
        assert!(!env.contains_key("CLAUDE_AUTH_MODE"));
        assert!(!env.contains_key("ANTHROPIC_MODEL"));
        assert!(!env.contains_key("ANTHROPIC_DEFAULT_SONNET_MODEL"));
        assert!(!env.contains_key("ANTHROPIC_CUSTOM_MODEL_OPTION"));
        assert!(!env.contains_key("CLAUDE_CODE_USE_BEDROCK"));
        for key in MISC_SAMPLE_KEYS {
            assert_eq!(env.get(*key), expected.get(*key), "{key}");
        }

        let mgr = ConnectionManager::new();
        let result = conversation_set_claude_profile_core(
            &db,
            &mgr,
            data.path(),
            conv,
            Some(OFFICIAL_DIRECT_PROFILE_ID.into()),
        )
        .await
        .unwrap();
        assert_eq!(
            result.profile_id.as_deref(),
            Some(OFFICIAL_DIRECT_PROFILE_ID)
        );
    }

    #[tokio::test]
    async fn profile_env_is_injected_and_dedicated_fields_win_at_spawn() {
        use crate::commands::acp::build_session_runtime_env;
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        let (env_json, _) = connection_and_misc_env_json();
        seed_claude_agent(&db, Some(&env_json)).await;
        let folder = seed_folder(&db, data.path().to_str().unwrap()).await;
        let conv = seed_conversation(&db, folder, AgentType::ClaudeCode).await;

        let mut env = BTreeMap::new();
        env.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            "https://from-env.example/v1".to_string(),
        );
        env.insert(
            "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
            "haiku-profile".to_string(),
        );
        claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "gw".to_string(),
                label: "GW".to_string(),
                kind: ClaudeProfileKind::Managed,
                config_dir: None,
                base_url: Some("https://from-field.example/v1".into()),
                auth_token: Some("sk-from-field".into()),
                model: Some("from-field-model".into()),
                settings_json: None,
                env: Some(env),
            },
        )
        .unwrap();
        conversation_service::merge_selector_config_value(
            &db.conn,
            conv,
            PREFERRED_PROFILE_CONFIG_KEY,
            "gw",
        )
        .await
        .unwrap();

        let runtime =
            build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), Some(conv))
                .await
                .unwrap();
        let overlay = managed_settings_json_path(data.path(), "gw");
        assert_eq!(
            runtime
                .get(CLAUDE_SETTINGS_OVERLAY_ENV_KEY)
                .map(String::as_str),
            Some(overlay.to_string_lossy().as_ref())
        );
        assert!(!runtime.contains_key("CLAUDE_CONFIG_DIR"));
        for key in [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
        ] {
            assert_eq!(
                runtime.get(key).map(String::as_str),
                Some(""),
                "managed must env_remove {key}, not re-inject it: {runtime:?}"
            );
        }
        assert!(
            !runtime.contains_key("ANTHROPIC_MODEL"),
            "managed dedicated fields ride on --settings, not process env: {runtime:?}"
        );
        assert!(
            !runtime.contains_key("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
            "managed record.env rides on --settings, not process env: {runtime:?}"
        );
        assert_eq!(
            runtime.get("DISABLE_TELEMETRY").map(String::as_str),
            Some("1")
        );
        assert!(!runtime.contains_key("CLAUDE_AUTH_MODE"));

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&overlay).unwrap()).unwrap();
        let block = settings.get("env").and_then(|v| v.as_object()).unwrap();
        assert_eq!(
            block.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()),
            Some("https://from-field.example/v1")
        );
        assert_eq!(
            block.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()),
            Some("sk-from-field")
        );
        assert_eq!(
            block.get("ANTHROPIC_MODEL").and_then(|v| v.as_str()),
            Some("from-field-model")
        );
        assert_eq!(
            block
                .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(|v| v.as_str()),
            Some("haiku-profile")
        );
    }

    #[tokio::test]
    async fn config_dir_profile_env_is_injected_at_spawn_not_written_to_user_dir() {
        use crate::commands::acp::build_session_runtime_env;
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        let (env_json, _) = connection_and_misc_env_json();
        seed_claude_agent(&db, Some(&env_json)).await;
        let folder = seed_folder(&db, data.path().to_str().unwrap()).await;
        let conv = seed_conversation(&db, folder, AgentType::ClaudeCode).await;
        let user_dir = data.path().join("user-claude");
        fs::create_dir_all(&user_dir).unwrap();
        let mut env = BTreeMap::new();
        env.insert(
            "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
            "sonnet-profile".to_string(),
        );
        claude_profile_upsert_core(
            data.path(),
            ClaudeProfileUpsert {
                id: "user".to_string(),
                label: "User dir".to_string(),
                kind: ClaudeProfileKind::ConfigDir,
                config_dir: Some(user_dir.to_string_lossy().into_owned()),
                base_url: None,
                auth_token: None,
                model: None,
                settings_json: None,
                env: Some(env),
            },
        )
        .unwrap();
        conversation_service::merge_selector_config_value(
            &db.conn,
            conv,
            PREFERRED_PROFILE_CONFIG_KEY,
            "user",
        )
        .await
        .unwrap();

        let runtime =
            build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), Some(conv))
                .await
                .unwrap();
        assert_eq!(
            runtime
                .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                .map(String::as_str),
            Some("sonnet-profile")
        );
        assert_eq!(
            runtime.get("CLAUDE_CONFIG_DIR").map(String::as_str),
            Some(user_dir.to_string_lossy().as_ref())
        );
        assert!(
            !runtime.contains_key(CLAUDE_SETTINGS_OVERLAY_ENV_KEY),
            "configDir must not set extraArgs.settings: {runtime:?}"
        );
        assert!(
            !user_dir.join("settings.json").exists(),
            "configDir must not write the user's directory"
        );
    }

    #[tokio::test]
    async fn non_claude_keeps_anthropic_keys_and_ignores_profiles() {
        use crate::commands::acp::build_session_runtime_env;
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        seed_claude_agent(&db, None).await;
        agent_setting_service::update(
            &db.conn,
            AgentType::Codex,
            AgentSettingsUpdate {
                enabled: true,
                env_json: Some(
                    serde_json::to_string(&BTreeMap::from([
                        (
                            "ANTHROPIC_AUTH_TOKEN".to_string(),
                            "sk-codex-should-keep".to_string(),
                        ),
                        ("DISABLE_TELEMETRY".to_string(), "1".to_string()),
                    ]))
                    .unwrap(),
                ),
                model_provider_id: None,
            },
        )
        .await
        .unwrap();
        let folder = seed_folder(&db, data.path().to_str().unwrap()).await;
        let conv = seed_conversation(&db, folder, AgentType::Codex).await;
        conversation_service::merge_selector_config_value(
            &db.conn,
            conv,
            PREFERRED_PROFILE_CONFIG_KEY,
            OFFICIAL_DIRECT_PROFILE_ID,
        )
        .await
        .unwrap();
        let env = build_session_runtime_env(&db, AgentType::Codex, None, data.path(), Some(conv))
            .await
            .unwrap();
        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").map(String::as_str),
            Some("sk-codex-should-keep")
        );
        assert!(!env.contains_key("CLAUDE_CONFIG_DIR"));
        assert_ne!(
            env.get("ANTHROPIC_BASE_URL").map(String::as_str),
            Some(OFFICIAL_ANTHROPIC_BASE_URL)
        );
    }

    fn real_legacy_claude_env() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("ANTHROPIC_AUTH_TOKEN".into(), "sk-real-shape".into()),
            (
                "ANTHROPIC_BASE_URL".into(),
                "https://relay.example/v1".into(),
            ),
            (
                "ANTHROPIC_CUSTOM_MODEL_OPTION".into(),
                "relay/custom".into(),
            ),
            ("ANTHROPIC_DEFAULT_FABLE_MODEL".into(), "relay/fable".into()),
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL".into(), "relay/haiku".into()),
            ("ANTHROPIC_DEFAULT_OPUS_MODEL".into(), "relay/opus".into()),
            (
                "ANTHROPIC_DEFAULT_SONNET_MODEL".into(),
                "relay/sonnet".into(),
            ),
            ("CLAUDE_AUTH_MODE".into(), "custom".into()),
            ("CLAUDE_CODE_ATTRIBUTION_HEADER".into(), "x-codeg".into()),
            ("CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY".into(), "1".into()),
            (
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".into(),
                "1".into(),
            ),
            ("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS".into(), "1".into()),
            (
                "CLAUDE_CODE_GIT_BASH_PATH".into(),
                "C:/Program Files/Git/bin/bash.exe".into(),
            ),
            ("CLAUDE_CODE_SCROLL_SPEED".into(), "2".into()),
            ("DISABLE_AUTOUPDATER".into(), "1".into()),
            ("DISABLE_FEEDBACK_COMMAND".into(), "1".into()),
            ("DISABLE_TELEMETRY".into(), "1".into()),
            ("ENABLE_TOOL_SEARCH".into(), "1".into()),
        ])
    }

    #[tokio::test]
    async fn migration_preserves_effective_connection_config_and_misc_key_for_key() {
        use crate::commands::acp::build_session_runtime_env;

        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        let before = real_legacy_claude_env();
        seed_claude_agent(&db, Some(&serde_json::to_string(&before).unwrap())).await;

        let created = migrate_claude_profile_owned_env(&db, data.path())
            .await
            .unwrap();
        assert_eq!(created.as_deref(), Some("imported"));

        let list = claude_profile_list_core(data.path()).unwrap();
        assert_eq!(
            list.iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            vec![FOLLOW_DEFAULT_PROFILE_ID, "imported"]
        );
        assert_eq!(list[1].label, "relay.example");

        let setting = agent_setting_service::get_by_agent_type(&db.conn, AgentType::ClaudeCode)
            .await
            .unwrap()
            .unwrap();
        let after_db: BTreeMap<String, String> =
            serde_json::from_str(setting.env_json.as_deref().unwrap()).unwrap();
        assert_eq!(
            after_db
                .get(CODEG_CLAUDE_PROFILE_ENV_KEY)
                .map(String::as_str),
            Some("imported")
        );
        assert_eq!(
            after_db
                .get(CODEG_CLAUDE_PROFILE_MIGRATED_ENV_KEY)
                .map(String::as_str),
            Some("1")
        );
        for (key, value) in &before {
            if is_profile_owned_env_key(key) {
                assert!(
                    !after_db.contains_key(key),
                    "legacy connection key survived: {key}"
                );
            } else {
                assert_eq!(after_db.get(key), Some(value), "misc key changed: {key}");
            }
        }

        let settings: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(managed_config_dir(data.path(), "imported").join("settings.json"))
                .unwrap(),
        )
        .unwrap();
        let effective: BTreeMap<String, String> =
            serde_json::from_value(settings["env"].clone()).unwrap();
        assert!(!effective.contains_key("CLAUDE_AUTH_MODE"));

        let runtime =
            build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), None)
                .await
                .unwrap();
        let overlay = managed_settings_json_path(data.path(), "imported");
        assert_eq!(
            runtime
                .get(CLAUDE_SETTINGS_OVERLAY_ENV_KEY)
                .map(String::as_str),
            Some(overlay.to_string_lossy().as_ref())
        );
        assert!(!runtime.contains_key("CLAUDE_CONFIG_DIR"));
        for key in [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
        ] {
            assert_eq!(
                runtime.get(key).map(String::as_str),
                Some(""),
                "managed migration must env_remove {key}, not re-inject it"
            );
        }
        for key in [
            "ANTHROPIC_CUSTOM_MODEL_OPTION",
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
        ] {
            assert!(
                !runtime.contains_key(key),
                "settingsJson-only key must be read by the CLI, not injected: {key}"
            );
        }
        for (key, value) in &before {
            if is_profile_owned_env_key(key) && key != "CLAUDE_AUTH_MODE" {
                assert_eq!(
                    effective.get(key),
                    Some(value),
                    "settings.json env key changed: {key}"
                );
            }
        }
    }

    #[tokio::test]
    async fn migration_is_idempotent_and_does_not_create_a_second_profile() {
        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        let before = real_legacy_claude_env();
        seed_claude_agent(&db, Some(&serde_json::to_string(&before).unwrap())).await;
        assert_eq!(
            migrate_claude_profile_owned_env(&db, data.path())
                .await
                .unwrap()
                .as_deref(),
            Some("imported")
        );
        assert_eq!(
            migrate_claude_profile_owned_env(&db, data.path())
                .await
                .unwrap(),
            None
        );
        let persisted = fs::read_dir(claude_profiles_dir(data.path()))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "json"))
            .count();
        assert_eq!(persisted, 1);
    }

    #[tokio::test]
    async fn migrated_official_auth_mode_is_expressed_by_profile_credential_scrub() {
        use crate::commands::acp::build_session_runtime_env;

        let db = fresh_in_memory_db().await;
        let data = tempfile::tempdir().unwrap();
        let before = BTreeMap::from([(
            "CLAUDE_AUTH_MODE".to_string(),
            "official_subscription".to_string(),
        )]);
        seed_claude_agent(&db, Some(&serde_json::to_string(&before).unwrap())).await;
        assert_eq!(
            migrate_claude_profile_owned_env(&db, data.path())
                .await
                .unwrap()
                .as_deref(),
            Some("imported")
        );

        let runtime =
            build_session_runtime_env(&db, AgentType::ClaudeCode, None, data.path(), None)
                .await
                .unwrap();
        assert!(!runtime.contains_key("CLAUDE_AUTH_MODE"));
        for key in [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
        ] {
            assert_eq!(
                runtime.get(key).map(String::as_str),
                Some(""),
                "profile must preserve official-subscription credential scrub for {key}"
            );
        }
    }

    #[tokio::test]
    async fn migration_no_connection_keys_is_a_true_noop() {
        for source in [
            BTreeMap::<String, String>::new(),
            BTreeMap::from([
                ("CLAUDE_CODE_GIT_BASH_PATH".into(), "C:/Git/bash.exe".into()),
                ("DISABLE_TELEMETRY".into(), "1".into()),
            ]),
        ] {
            let db = fresh_in_memory_db().await;
            let data = tempfile::tempdir().unwrap();
            seed_claude_agent(&db, Some(&serde_json::to_string(&source).unwrap())).await;
            assert_eq!(
                migrate_claude_profile_owned_env(&db, data.path())
                    .await
                    .unwrap(),
                None
            );
            let setting = agent_setting_service::get_by_agent_type(&db.conn, AgentType::ClaudeCode)
                .await
                .unwrap()
                .unwrap();
            let after: BTreeMap<String, String> =
                serde_json::from_str(setting.env_json.as_deref().unwrap()).unwrap();
            assert_eq!(after, source);
            assert!(!after.contains_key(CODEG_CLAUDE_PROFILE_MIGRATED_ENV_KEY));
            assert!(!claude_profiles_dir(data.path()).exists());
        }
    }

    #[test]
    fn fingerprint_ignores_profile_owned_keys_for_claude() {
        use crate::commands::acp::fingerprint_config;
        let mut a = BTreeMap::new();
        a.insert("DISABLE_TELEMETRY".to_string(), "1".to_string());
        let mut b = a.clone();
        b.insert("ANTHROPIC_AUTH_TOKEN".to_string(), "sk-x".to_string());
        b.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            OFFICIAL_ANTHROPIC_BASE_URL.to_string(),
        );
        b.insert("CLAUDE_AUTH_MODE".to_string(), "custom".to_string());
        assert_eq!(
            fingerprint_config(AgentType::ClaudeCode, &a),
            fingerprint_config(AgentType::ClaudeCode, &b),
            "profile-owned keys must not flip the agent-level fingerprint"
        );
    }
}
