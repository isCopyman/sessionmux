use sea_orm::DatabaseConnection;

use crate::app_error::AppCommandError;
use crate::models::SystemProxySettings;

const PROXY_ENV_KEYS: [&str; 6] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
];

fn names_a_host(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|host| !host.is_empty())
}

fn needs_http_prefix(value: &str) -> bool {
    if value.contains("://") {
        return false;
    }
    !reqwest::Url::parse(value).is_ok_and(|url| names_a_host(&url))
}

/// Normalize the abbreviated `host:port` form accepted by the settings UI into
/// a URL that Node/npm and every spawned Harness can parse consistently.
pub(crate) fn normalize_proxy_url(raw: &str) -> Result<String, AppCommandError> {
    let trimmed = raw.trim();
    let normalized = if needs_http_prefix(trimmed) {
        format!("http://{trimmed}")
    } else {
        trimmed.to_string()
    };

    let parsed = reqwest::Url::parse(&normalized).map_err(|error| {
        AppCommandError::configuration_invalid("Invalid proxy URL").with_detail(error.to_string())
    })?;
    if !names_a_host(&parsed) {
        return Err(AppCommandError::configuration_invalid("Invalid proxy URL")
            .with_detail("a proxy address must include a host"));
    }
    reqwest::Proxy::all(&normalized).map_err(|error| {
        AppCommandError::configuration_invalid("Invalid proxy URL").with_detail(error.to_string())
    })?;
    Ok(normalized)
}

pub(crate) fn proxy_env_value(
    settings: &SystemProxySettings,
) -> Result<Option<String>, AppCommandError> {
    if !settings.enabled {
        return Ok(None);
    }
    let value = settings
        .proxy_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppCommandError::configuration_missing("Proxy URL is required when proxy is enabled")
        })?;
    normalize_proxy_url(value).map(Some)
}

pub fn apply_system_proxy_settings(settings: &SystemProxySettings) -> Result<(), AppCommandError> {
    match proxy_env_value(settings)? {
        Some(proxy_url) => {
            for key in PROXY_ENV_KEYS {
                unsafe {
                    std::env::set_var(key, &proxy_url);
                }
            }
        }
        None => clear_proxy_env(),
    }

    Ok(())
}

pub fn clear_proxy_env() {
    for key in PROXY_ENV_KEYS {
        unsafe {
            std::env::remove_var(key);
        }
    }
}

/// Load persisted proxy settings from the DB and apply them to process env.
/// Must run before the first reqwest client is built — otherwise that client
/// caches the proxy-less config and ignores the user's choice for its lifetime.
/// Errors are logged and dropped: a misconfigured proxy must not block startup.
///
/// Only writes env vars when the DB explicitly stores `enabled=true`. A fresh
/// install or an explicit disable in the UI leaves externally-set HTTP_PROXY
/// alone, so docker `-e` and systemd `Environment=` keep working. Runtime
/// disable through `update_system_proxy_settings` still clears env — that path
/// is the user's explicit intent, not a default.
pub async fn init_proxy_from_db(conn: &DatabaseConnection) {
    match crate::commands::system_settings::load_system_proxy_settings(conn).await {
        Ok(settings) if settings.enabled => {
            if let Err(err) = apply_system_proxy_settings(&settings) {
                tracing::error!("[Settings] failed to apply system proxy settings: {err}");
            }
        }
        Ok(_) => {}
        Err(err) => {
            tracing::error!("[Settings] failed to load system proxy settings: {err}");
        }
    }
}

pub fn current_proxy_env_vars() -> Vec<(String, String)> {
    PROXY_ENV_KEYS
        .iter()
        .filter_map(|key| {
            std::env::var(key).ok().and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(((*key).to_string(), trimmed.to_string()))
                }
            })
        })
        .collect()
}
