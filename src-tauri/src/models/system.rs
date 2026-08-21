use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemProxySettings {
    pub enabled: bool,
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppLocale {
    #[default]
    En,
    ZhCn,
    ZhTw,
    Ja,
    Ko,
    Es,
    De,
    Fr,
    Pt,
    Ar,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LanguageMode {
    #[default]
    System,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SystemLanguageSettings {
    pub mode: LanguageMode,
    pub language: AppLocale,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SystemTerminalSettings {
    pub default_shell: Option<String>,
}

/// One row in the "default shell" picker. Backend owns the option list so the
/// frontend doesn't have to know which shells are available on which platform.
/// Labels are not localized server-side: `label_key` points at a frontend i18n
/// key under `GeneralSettings.*`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalShellOption {
    /// Stable identifier the dropdown uses as its <option value>.
    pub id: String,
    /// i18n key resolved by the frontend (`GeneralSettings.<label_key>`).
    pub label_key: String,
    /// Concrete value persisted into `SystemTerminalSettings.default_shell`.
    /// `None` for `system` (use `resolve_shell()`) and `custom` (user supplies path).
    pub value: Option<String>,
    /// Whether this shell is currently resolvable on the host. `false` lets
    /// the UI mark the option as "not installed" without preventing selection.
    pub exists: bool,
    /// True for the `custom` row — the UI should render a path input next to
    /// the dropdown when this option is selected.
    pub accepts_custom_path: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableTerminalShells {
    pub options: Vec<TerminalShellOption>,
    /// What `resolve_shell()` would currently fall back to. Surfaced read-only
    /// in the UI so users can see what "system default" actually maps to.
    pub resolved_shell: String,
}

#[cfg(feature = "tauri-runtime")]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SystemRenderingSettings {
    pub disable_hardware_acceleration: bool,
}

// --- Version Control ---

/// Explicit credentials for a single git remote operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDetectResult {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GitSettings {
    pub custom_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAccount {
    pub id: String,
    pub server_url: String,
    pub username: String,
    pub scopes: Vec<String>,
    pub avatar_url: Option<String>,
    pub is_default: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GitHubAccountsSettings {
    pub accounts: Vec<GitHubAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubTokenValidation {
    pub success: bool,
    pub username: Option<String>,
    pub scopes: Vec<String>,
    pub avatar_url: Option<String>,
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_proxy_settings_persisted_json_shape_is_pinned() {
        let legacy = r#"{"enabled": true, "proxy_url": "http://127.0.0.1:7890"}"#;
        let s: SystemProxySettings =
            serde_json::from_str(legacy).expect("legacy system_proxy_settings decodes");
        assert!(s.enabled);
        assert_eq!(s.proxy_url.as_deref(), Some("http://127.0.0.1:7890"));

        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("proxy_url").is_some());
        assert!(v.get("proxyUrl").is_none());
    }

    #[test]
    fn system_language_settings_persisted_json_shape_is_pinned() {
        let legacy = r#"{"mode": "manual", "language": "zh_cn"}"#;
        let s: SystemLanguageSettings =
            serde_json::from_str(legacy).expect("legacy system_language_settings decodes");
        assert_eq!(s.mode, LanguageMode::Manual);
        assert_eq!(s.language, AppLocale::ZhCn);

        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["mode"], "manual");
        assert_eq!(v["language"], "zh_cn");
        assert_eq!(serde_json::to_value(AppLocale::ZhTw).unwrap(), "zh_tw");
        assert_eq!(
            serde_json::to_value(LanguageMode::System).unwrap(),
            "system"
        );
        assert!(serde_json::from_str::<AppLocale>("\"zhCn\"").is_err());
        assert!(serde_json::from_str::<LanguageMode>("\"System\"").is_err());
    }

    #[test]
    fn system_terminal_settings_persisted_json_shape_is_pinned() {
        let legacy = r#"{"default_shell": "pwsh"}"#;
        let s: SystemTerminalSettings =
            serde_json::from_str(legacy).expect("legacy system_terminal_settings decodes");
        assert_eq!(s.default_shell.as_deref(), Some("pwsh"));

        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("default_shell").is_some());
        assert!(v.get("defaultShell").is_none());
    }

    #[test]
    fn git_settings_persisted_json_shape_is_pinned() {
        let legacy = r#"{"custom_path": "/usr/bin/git"}"#;
        let s: GitSettings = serde_json::from_str(legacy).expect("legacy git_settings decodes");
        assert_eq!(s.custom_path.as_deref(), Some("/usr/bin/git"));

        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("custom_path").is_some());
        assert!(v.get("customPath").is_none());
    }

    #[test]
    fn github_account_persisted_json_shape_is_pinned() {
        let legacy = r#"{
            "id": "acc-1",
            "server_url": "https://github.com",
            "username": "octocat",
            "scopes": ["repo"],
            "avatar_url": "https://example.com/a.png",
            "is_default": true,
            "created_at": "2026-01-02T03:04:05Z"
        }"#;
        let a: GitHubAccount =
            serde_json::from_str(legacy).expect("legacy github_accounts row decodes");
        assert_eq!(a.id, "acc-1");
        assert_eq!(a.server_url, "https://github.com");
        assert_eq!(a.username, "octocat");
        assert_eq!(a.scopes, vec!["repo"]);
        assert_eq!(a.avatar_url.as_deref(), Some("https://example.com/a.png"));
        assert!(a.is_default);
        assert_eq!(a.created_at, "2026-01-02T03:04:05Z");

        let v = serde_json::to_value(&a).unwrap();
        assert!(v.get("server_url").is_some());
        assert!(v.get("serverUrl").is_none());
        assert!(v.get("avatar_url").is_some());
        assert!(v.get("avatarUrl").is_none());
        assert!(v.get("is_default").is_some());
        assert!(v.get("isDefault").is_none());
        assert!(v.get("created_at").is_some());
        assert!(v.get("createdAt").is_none());
    }
}
