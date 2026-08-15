use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::agent::AgentType;
use crate::db::entities::folder::FolderKind;

#[derive(Debug, Clone, Serialize)]
pub struct FolderHistoryEntry {
    pub id: i32,
    pub path: String,
    pub name: String,
    pub last_opened_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderDetail {
    pub id: i32,
    pub name: String,
    pub path: String,
    pub git_branch: Option<String>,
    pub default_agent_type: Option<AgentType>,
    pub last_opened_at: DateTime<Utc>,
    pub sort_order: i32,
    pub color: String,
    /// Root folder this one was created under (worktree folders only); NULL for
    /// top-level folders. Drives sidebar merge + worktree-branch detection.
    pub parent_id: Option<i32>,
    /// Folder classification (mirrors `folder.kind`). `chat` folders are kept in
    /// `allFolders` (so cwd / active-folder resolve) but hidden from folder
    /// lists; their conversations route to the sidebar "Chat" group.
    pub kind: FolderKind,
    /// User-supplied display alias, or NULL when unset. When present the sidebar
    /// folder header and conversation header render `alias [name]`.
    pub alias: Option<String>,
}

/// A named, user-visible arrangement of conversation tabs. Sessions themselves
/// remain global records; a workbench only owns references, focus and layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchInfo {
    pub id: i32,
    pub name: String,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A semantic folder for long-lived Session organization. Collections never
/// change a Session's execution folder, cwd, worktree, model or permissions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionInfo {
    pub id: i32,
    pub parent_id: Option<i32>,
    pub name: String,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The unique semantic home of one Session. Absence means "Unclassified";
/// Workbench membership and shortcuts are deliberately not represented here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationCollectionRef {
    pub conversation_id: i32,
    pub collection_id: i32,
}

/// One saved workbench that currently references a conversation. A session is
/// still a global record: these rows only describe where its tab is open, so
/// Session Center can explain ownership without loading every workbench tab
/// snapshot on the client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationWorkbenchRef {
    pub conversation_id: i32,
    pub workbench_id: i32,
    pub workbench_name: String,
    pub workbench_position: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenedTab {
    pub id: i32,
    pub folder_id: i32,
    pub conversation_id: Option<i32>,
    pub agent_type: AgentType,
    pub position: i32,
    pub is_active: bool,
    pub is_pinned: bool,
}

/// Response for `list_opened_tabs`: the persisted tab set plus the current
/// workspace tab version. Clients seed their compare-and-set / echo logic from
/// `version`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedTabsSnapshot {
    pub items: Vec<OpenedTab>,
    pub version: i64,
}

/// Response for `save_opened_tabs`: whether the compare-and-set was applied, the
/// authoritative version after the call, and the canonical tab set. When
/// `accepted` is false the save was stale (another client won) and `tabs` is the
/// current truth to reconcile against.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTabsOutcome {
    pub accepted: bool,
    pub version: i64,
    pub tabs: Vec<OpenedTab>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderCommandInfo {
    pub id: i32,
    pub folder_id: i32,
    pub name: String,
    pub command: String,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
