//! Shared domain types for Codeg's progressive Host Control gateway.
//!
//! The MCP companion exposes only `codeg_help` and `codeg_use`. The listener
//! authenticates the per-launch token, derives the current Session from the
//! parent ACP connection, and hands one of these typed requests to the Host
//! Core. Model-supplied input therefore never carries a source/current Session
//! identity.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

pub const HOST_CONTROL_CATALOG_VERSION: &str = "2026-08-17.v5";

/// Trusted caller context derived from the companion token. This type is never
/// part of an MCP tool schema and is never populated from model arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostControlCaller {
    pub current_session_id: i32,
    pub working_dir: PathBuf,
    /// Bound from the parent Session's host-tools policy at MCP injection time.
    /// Read actions remain available under the restricted policy; writes do not.
    pub writes_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostControlAccessLevel {
    Read,
    Write,
}

/// One entry in the server-owned capability catalog. `input_schema` is
/// descriptive discovery data; execution still parses the input into an
/// action-specific `#[serde(deny_unknown_fields)]` type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostControlCapability {
    pub action: String,
    pub description: String,
    pub access: HostControlAccessLevel,
    pub input_schema: Value,
    pub result_stages: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostControlHelpOutcome {
    pub available: bool,
    pub catalog_version: String,
    pub capabilities: Vec<HostControlCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl HostControlHelpOutcome {
    pub fn unavailable(note: impl Into<String>) -> Self {
        Self {
            available: false,
            catalog_version: HOST_CONTROL_CATALOG_VERSION.to_string(),
            capabilities: Vec::new(),
            note: Some(note.into()),
        }
    }
}

/// Result of one `codeg_use` action. The stage names are deliberately explicit:
/// a persisted rename is not the same thing as a queued prompt or a UI ACK.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostControlUseOutcome {
    pub accepted: bool,
    pub request_id: String,
    pub action: String,
    pub stage: String,
    pub replayed: bool,
    #[serde(default)]
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl HostControlUseOutcome {
    pub fn rejected(
        request_id: impl Into<String>,
        action: impl Into<String>,
        note: impl Into<String>,
    ) -> Self {
        Self {
            accepted: false,
            request_id: request_id.into(),
            action: action.into(),
            stage: "rejected".to_string(),
            replayed: false,
            data: Value::Null,
            note: Some(note.into()),
        }
    }
}

#[async_trait]
pub trait HostControlAccess: Send + Sync {
    async fn help(
        &self,
        caller: HostControlCaller,
        query: Option<String>,
        action: Option<String>,
    ) -> HostControlHelpOutcome;

    async fn use_action(
        &self,
        caller: HostControlCaller,
        request_id: String,
        action: String,
        input: Value,
    ) -> HostControlUseOutcome;
}

/// Hot-swappable feature gate. Injection reads it to decide whether to expose
/// the gateway; the Host Core reads it again for every help/use call so turning
/// it off also stops companions that were launched earlier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostControlConfig {
    pub enabled: bool,
    pub writes_enabled: bool,
}

impl Default for HostControlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            writes_enabled: true,
        }
    }
}

#[derive(Clone, Default)]
pub struct HostControlRuntimeConfig {
    inner: Arc<RwLock<HostControlConfig>>,
}

impl HostControlRuntimeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn snapshot(&self) -> HostControlConfig {
        self.inner.read().await.clone()
    }

    pub async fn set(&self, config: HostControlConfig) {
        *self.inner.write().await = config;
    }

    pub async fn is_enabled(&self) -> bool {
        self.inner.read().await.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runtime_config_defaults_on_and_updates_live() {
        let config = HostControlRuntimeConfig::new();
        assert!(config.is_enabled().await);
        config
            .set(HostControlConfig {
                enabled: false,
                writes_enabled: false,
            })
            .await;
        assert!(!config.is_enabled().await);
        assert!(!config.snapshot().await.writes_enabled);
    }
}
