use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::process::Command;

use crate::app_error::AppCommandError;
use crate::db::service::conversation_service;
#[cfg(feature = "tauri-runtime")]
use crate::db::AppDatabase;
use crate::models::{
    AgentType, DbConversationSummary, SessionContentSearchHit, SessionContentSearchResponse,
};

const CTX_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESULTS: usize = 50;
const MAX_CTX_RESULTS: usize = 200;
const MAX_SNIPPET_CHARS: usize = 700;

#[derive(Debug, Deserialize)]
struct CtxSearchEnvelope {
    #[serde(default)]
    results: Vec<CtxSearchResult>,
}

#[derive(Debug, Deserialize)]
struct CtxSearchResult {
    provider: String,
    provider_session_id: Option<String>,
    #[serde(default)]
    snippet: String,
    timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    more_matches_in_session: u32,
}

fn ctx_provider(agent_type: AgentType) -> Option<&'static str> {
    match agent_type {
        AgentType::ClaudeCode => Some("claude"),
        AgentType::Codex => Some("codex"),
        AgentType::OpenCode => Some("opencode"),
        AgentType::Gemini => Some("gemini"),
        AgentType::OpenClaw => Some("openclaw"),
        AgentType::Cline => Some("cline"),
        AgentType::Hermes => Some("hermes"),
        AgentType::CodeBuddy => Some("codebuddy"),
        AgentType::KimiCode => Some("kimi"),
        AgentType::Pi => Some("pi"),
        AgentType::Grok => Some("grok"),
        AgentType::Cursor => Some("cursor"),
        AgentType::Custom(_) => None,
    }
}

fn agent_from_ctx_provider(provider: &str) -> Option<AgentType> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "claude_code" => Some(AgentType::ClaudeCode),
        "codex" => Some(AgentType::Codex),
        "opencode" | "open-code" | "open_code" => Some(AgentType::OpenCode),
        "gemini" | "gemini-cli" | "gemini_cli" => Some(AgentType::Gemini),
        "openclaw" | "open-claw" | "open_claw" => Some(AgentType::OpenClaw),
        "cline" => Some(AgentType::Cline),
        "hermes" => Some(AgentType::Hermes),
        "codebuddy" | "code-buddy" | "code_buddy" => Some(AgentType::CodeBuddy),
        "kimi" | "kimi-code" | "kimi_code" => Some(AgentType::KimiCode),
        "pi" => Some(AgentType::Pi),
        "grok" => Some(AgentType::Grok),
        "cursor" => Some(AgentType::Cursor),
        _ => None,
    }
}

fn clamp_snippet(value: &str) -> String {
    let normalized = value.trim().replace('\0', "");
    if normalized.chars().count() <= MAX_SNIPPET_CHARS {
        return normalized;
    }
    let mut truncated: String = normalized.chars().take(MAX_SNIPPET_CHARS).collect();
    truncated.push('…');
    truncated
}

fn map_ctx_results(
    rows: Vec<CtxSearchResult>,
    conversations: Vec<DbConversationSummary>,
    limit: usize,
) -> Vec<SessionContentSearchHit> {
    let mut sessions = HashMap::new();
    for conversation in conversations {
        let Some(external_id) = conversation.external_id.as_deref() else {
            continue;
        };
        sessions
            .entry((conversation.agent_type, external_id.to_string()))
            .or_insert(conversation);
    }

    let mut seen = HashSet::new();
    let mut mapped = Vec::new();
    for row in rows {
        let Some(agent_type) = agent_from_ctx_provider(&row.provider) else {
            continue;
        };
        let Some(provider_session_id) = row.provider_session_id else {
            continue;
        };
        let Some(conversation) = sessions.get(&(agent_type, provider_session_id)).cloned() else {
            continue;
        };
        if !seen.insert(conversation.id) {
            continue;
        }
        mapped.push(SessionContentSearchHit {
            conversation,
            snippet: clamp_snippet(&row.snippet),
            matched_at: row.timestamp,
            more_matches: row.more_matches_in_session,
        });
        if mapped.len() >= limit {
            break;
        }
    }
    mapped
}

async fn find_ctx() -> Option<PathBuf> {
    tokio::task::spawn_blocking(|| which::which("ctx").ok())
        .await
        .ok()
        .flatten()
}

pub async fn search_session_content_core(
    conn: &sea_orm::DatabaseConnection,
    query: String,
    folder_ids: Option<Vec<i32>>,
    agent_type: Option<AgentType>,
    requested_limit: Option<usize>,
) -> Result<SessionContentSearchResponse, AppCommandError> {
    let query = query.trim();
    if query.chars().count() < 2 {
        return Err(AppCommandError::invalid_input(
            "Content search requires at least two characters",
        ));
    }

    let Some(ctx_path) = find_ctx().await else {
        return Ok(SessionContentSearchResponse {
            available: false,
            reason: Some("ctx is not installed or is not available on PATH".into()),
            results: Vec::new(),
        });
    };

    let limit = requested_limit.unwrap_or(20).clamp(1, MAX_RESULTS);
    let mut command = Command::new(ctx_path);
    command
        .arg("search")
        .arg(query)
        .arg("--limit")
        .arg(MAX_CTX_RESULTS.to_string())
        .arg("--backend")
        .arg("lexical")
        .arg("--refresh")
        .arg("off")
        .arg("--json")
        .arg("--quiet")
        .env("CTX_QUIET", "1")
        .kill_on_drop(true);
    if let Some(agent_type) = agent_type {
        if let Some(provider) = ctx_provider(agent_type) {
            command.arg("--provider").arg(provider);
        }
    }

    let output = match tokio::time::timeout(CTX_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            return Ok(SessionContentSearchResponse {
                available: false,
                reason: Some(format!("Failed to start ctx: {error}")),
                results: Vec::new(),
            })
        }
        Err(_) => {
            return Ok(SessionContentSearchResponse {
                available: false,
                reason: Some("ctx content search timed out".into()),
                results: Vec::new(),
            })
        }
    };
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Ok(SessionContentSearchResponse {
            available: false,
            reason: Some(if detail.is_empty() {
                format!("ctx exited with {}", output.status)
            } else {
                detail
            }),
            results: Vec::new(),
        });
    }

    let envelope: CtxSearchEnvelope = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(error) => {
            return Ok(SessionContentSearchResponse {
                available: false,
                reason: Some(format!("ctx returned invalid JSON: {error}")),
                results: Vec::new(),
            })
        }
    };
    let conversations =
        conversation_service::list_all(conn, folder_ids, agent_type, None, None, None, false)
            .await
            .map_err(AppCommandError::from)?;

    Ok(SessionContentSearchResponse {
        available: true,
        reason: None,
        results: map_ctx_results(envelope.results, conversations, limit),
    })
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn search_session_content(
    db: tauri::State<'_, AppDatabase>,
    query: String,
    folder_ids: Option<Vec<i32>>,
    agent_type: Option<AgentType>,
    limit: Option<usize>,
) -> Result<SessionContentSearchResponse, AppCommandError> {
    search_session_content_core(&db.conn, query, folder_ids, agent_type, limit).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::entities::conversation::ConversationKind;

    fn conversation(id: i32, agent_type: AgentType, external_id: &str) -> DbConversationSummary {
        DbConversationSummary {
            id,
            folder_id: 1,
            title: Some(format!("Session {id}")),
            title_locked: false,
            agent_type,
            status: "in_progress".into(),
            kind: ConversationKind::Regular,
            model: None,
            git_branch: None,
            external_id: Some(external_id.into()),
            message_count: 1,
            child_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            pinned_at: None,
            parent_id: None,
            parent_tool_use_id: None,
            delegation_call_id: None,
            origin_cwd: None,
        }
    }

    #[test]
    fn maps_ctx_provider_sessions_to_openable_codeg_conversations() {
        let rows = vec![
            CtxSearchResult {
                provider: "codex".into(),
                provider_session_id: Some("codex-1".into()),
                snippet: "matched body".into(),
                timestamp: None,
                more_matches_in_session: 2,
            },
            CtxSearchResult {
                provider: "claude".into(),
                provider_session_id: Some("not-imported".into()),
                snippet: "must stay hidden".into(),
                timestamp: None,
                more_matches_in_session: 0,
            },
        ];
        let mapped = map_ctx_results(rows, vec![conversation(7, AgentType::Codex, "codex-1")], 20);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].conversation.id, 7);
        assert_eq!(mapped[0].snippet, "matched body");
        assert_eq!(mapped[0].more_matches, 2);
    }

    #[test]
    fn keeps_only_the_best_hit_per_conversation() {
        let rows = vec![
            CtxSearchResult {
                provider: "codex".into(),
                provider_session_id: Some("same".into()),
                snippet: "best".into(),
                timestamp: None,
                more_matches_in_session: 3,
            },
            CtxSearchResult {
                provider: "codex".into(),
                provider_session_id: Some("same".into()),
                snippet: "later duplicate".into(),
                timestamp: None,
                more_matches_in_session: 0,
            },
        ];
        let mapped = map_ctx_results(rows, vec![conversation(9, AgentType::Codex, "same")], 20);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].snippet, "best");
    }

    #[test]
    fn clamps_large_snippets_without_splitting_unicode() {
        let input = "中".repeat(MAX_SNIPPET_CHARS + 5);
        let output = clamp_snippet(&input);
        assert_eq!(output.chars().count(), MAX_SNIPPET_CHARS + 1);
        assert!(output.ends_with('…'));
    }
}
