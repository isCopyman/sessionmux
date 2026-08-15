use crate::app_error::AppCommandError;
use crate::db::service::workbench_service;
#[cfg(feature = "tauri-runtime")]
use crate::db::AppDatabase;
use crate::models::{ConversationWorkbenchRef, WorkbenchInfo};

pub async fn list_workbenches_core(
    conn: &sea_orm::DatabaseConnection,
) -> Result<Vec<WorkbenchInfo>, AppCommandError> {
    workbench_service::list(conn)
        .await
        .map_err(AppCommandError::from)
}

pub async fn list_conversation_workbench_refs_core(
    conn: &sea_orm::DatabaseConnection,
    conversation_ids: Vec<i32>,
) -> Result<Vec<ConversationWorkbenchRef>, AppCommandError> {
    workbench_service::list_conversation_refs(conn, conversation_ids)
        .await
        .map_err(AppCommandError::from)
}

pub async fn create_workbench_core(
    conn: &sea_orm::DatabaseConnection,
    name: Option<String>,
) -> Result<WorkbenchInfo, AppCommandError> {
    workbench_service::create(conn, name)
        .await
        .map_err(AppCommandError::from)
}

pub async fn rename_workbench_core(
    conn: &sea_orm::DatabaseConnection,
    id: i32,
    name: String,
) -> Result<WorkbenchInfo, AppCommandError> {
    workbench_service::rename(conn, id, name)
        .await
        .map_err(AppCommandError::from)
}

pub async fn set_workbench_pinned_core(
    conn: &sea_orm::DatabaseConnection,
    id: i32,
    is_pinned: bool,
) -> Result<WorkbenchInfo, AppCommandError> {
    workbench_service::set_pinned(conn, id, is_pinned)
        .await
        .map_err(AppCommandError::from)
}

pub async fn duplicate_workbench_core(
    conn: &sea_orm::DatabaseConnection,
    source_id: i32,
    name: Option<String>,
) -> Result<WorkbenchInfo, AppCommandError> {
    workbench_service::duplicate(conn, source_id, name)
        .await
        .map_err(AppCommandError::from)
}

pub async fn reorder_workbenches_core(
    conn: &sea_orm::DatabaseConnection,
    ordered_ids: Vec<i32>,
) -> Result<Vec<WorkbenchInfo>, AppCommandError> {
    workbench_service::reorder(conn, ordered_ids)
        .await
        .map_err(AppCommandError::from)
}

pub async fn delete_workbench_core(
    conn: &sea_orm::DatabaseConnection,
    id: i32,
) -> Result<(), AppCommandError> {
    workbench_service::delete(conn, id)
        .await
        .map_err(AppCommandError::from)
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn list_workbenches(
    db: tauri::State<'_, AppDatabase>,
) -> Result<Vec<WorkbenchInfo>, AppCommandError> {
    list_workbenches_core(&db.conn).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn list_conversation_workbench_refs(
    db: tauri::State<'_, AppDatabase>,
    conversation_ids: Vec<i32>,
) -> Result<Vec<ConversationWorkbenchRef>, AppCommandError> {
    list_conversation_workbench_refs_core(&db.conn, conversation_ids).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn create_workbench(
    db: tauri::State<'_, AppDatabase>,
    name: Option<String>,
) -> Result<WorkbenchInfo, AppCommandError> {
    create_workbench_core(&db.conn, name).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn rename_workbench(
    db: tauri::State<'_, AppDatabase>,
    id: i32,
    name: String,
) -> Result<WorkbenchInfo, AppCommandError> {
    rename_workbench_core(&db.conn, id, name).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn set_workbench_pinned(
    db: tauri::State<'_, AppDatabase>,
    id: i32,
    is_pinned: bool,
) -> Result<WorkbenchInfo, AppCommandError> {
    set_workbench_pinned_core(&db.conn, id, is_pinned).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn duplicate_workbench(
    db: tauri::State<'_, AppDatabase>,
    source_id: i32,
    name: Option<String>,
) -> Result<WorkbenchInfo, AppCommandError> {
    duplicate_workbench_core(&db.conn, source_id, name).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn reorder_workbenches(
    db: tauri::State<'_, AppDatabase>,
    ordered_ids: Vec<i32>,
) -> Result<Vec<WorkbenchInfo>, AppCommandError> {
    reorder_workbenches_core(&db.conn, ordered_ids).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn delete_workbench(
    db: tauri::State<'_, AppDatabase>,
    id: i32,
) -> Result<(), AppCommandError> {
    delete_workbench_core(&db.conn, id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::conversations::{list_workbench_tabs_core, save_workbench_tabs_core};
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::{AgentType, OpenedTab};
    use crate::web::event_bridge::EventEmitter;

    fn tab(folder_id: i32, conversation_id: i32) -> OpenedTab {
        OpenedTab {
            id: 0,
            folder_id,
            conversation_id: Some(conversation_id),
            agent_type: AgentType::Codex,
            position: 0,
            is_active: true,
            is_pinned: true,
        }
    }

    #[tokio::test]
    async fn migration_seeds_compatibility_workbench() {
        let db = fresh_in_memory_db().await;
        let items = list_workbenches_core(&db.conn).await.expect("list");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, 1);
        assert_eq!(items[0].name, "Main");
    }

    #[tokio::test]
    async fn tab_snapshots_are_isolated_between_workbenches() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-workbench-tabs").await;
        let c1 = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let c2 = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let second = create_workbench_core(&db.conn, Some("Review".into()))
            .await
            .expect("create workbench");

        let main = save_workbench_tabs_core(
            &db.conn,
            &EventEmitter::Noop,
            1,
            vec![tab(folder_id, c1)],
            0,
            "test".into(),
        )
        .await
        .expect("save main");
        let review = save_workbench_tabs_core(
            &db.conn,
            &EventEmitter::Noop,
            second.id,
            vec![tab(folder_id, c2)],
            main.version,
            "test".into(),
        )
        .await
        .expect("save review");
        assert!(review.accepted);

        let main_tabs = list_workbench_tabs_core(&db.conn, 1).await.expect("main");
        let review_tabs = list_workbench_tabs_core(&db.conn, second.id)
            .await
            .expect("review");
        assert_eq!(main_tabs.items[0].conversation_id, Some(c1));
        assert_eq!(review_tabs.items[0].conversation_id, Some(c2));
    }

    #[tokio::test]
    async fn duplicate_workbench_copies_tab_references_without_copying_sessions() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-workbench-duplicate").await;
        let conversation_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        save_workbench_tabs_core(
            &db.conn,
            &EventEmitter::Noop,
            1,
            vec![tab(folder_id, conversation_id)],
            0,
            "test".into(),
        )
        .await
        .expect("save source");

        let copy = duplicate_workbench_core(&db.conn, 1, Some("Main Copy".into()))
            .await
            .expect("duplicate");
        let copied_tabs = list_workbench_tabs_core(&db.conn, copy.id)
            .await
            .expect("copy tabs");
        assert_eq!(copy.name, "Main Copy");
        assert_eq!(copied_tabs.items.len(), 1);
        assert_eq!(copied_tabs.items[0].conversation_id, Some(conversation_id));
        let session =
            crate::db::service::conversation_service::get_by_id(&db.conn, conversation_id)
                .await
                .expect("same session remains authoritative");
        assert_eq!(session.id, conversation_id);
    }

    #[tokio::test]
    async fn conversation_refs_report_each_saved_workbench_once() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-workbench-memberships").await;
        let conversation_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let second = create_workbench_core(&db.conn, Some("Review".into()))
            .await
            .expect("create review");

        let main = save_workbench_tabs_core(
            &db.conn,
            &EventEmitter::Noop,
            1,
            vec![tab(folder_id, conversation_id)],
            0,
            "test".into(),
        )
        .await
        .expect("save main");
        save_workbench_tabs_core(
            &db.conn,
            &EventEmitter::Noop,
            second.id,
            vec![tab(folder_id, conversation_id)],
            main.version,
            "test".into(),
        )
        .await
        .expect("save review");

        let refs =
            list_conversation_workbench_refs_core(&db.conn, vec![conversation_id, conversation_id])
                .await
                .expect("list refs");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].conversation_id, conversation_id);
        assert_eq!(refs[0].workbench_name, "Main");
        assert_eq!(refs[1].workbench_id, second.id);
        assert_eq!(refs[1].workbench_name, "Review");

        assert!(list_conversation_workbench_refs_core(&db.conn, Vec::new())
            .await
            .expect("empty query")
            .is_empty());
    }

    #[tokio::test]
    async fn reorder_workbenches_persists_one_complete_order() {
        let db = fresh_in_memory_db().await;
        let review = create_workbench_core(&db.conn, Some("Review".into()))
            .await
            .expect("create review");
        let experiments = create_workbench_core(&db.conn, Some("Experiments".into()))
            .await
            .expect("create experiments");

        let reordered = reorder_workbenches_core(&db.conn, vec![experiments.id, 1, review.id])
            .await
            .expect("reorder");
        assert_eq!(
            reordered.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![experiments.id, 1, review.id]
        );
        assert_eq!(
            reordered
                .iter()
                .map(|item| item.position)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );

        let error = reorder_workbenches_core(&db.conn, vec![1, review.id, review.id])
            .await
            .expect_err("duplicate ids are rejected");
        assert!(
            error
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("exactly once")),
            "validation detail should explain the complete-order contract"
        );
    }

    #[tokio::test]
    async fn pinned_workbenches_sort_before_unpinned_without_losing_manual_order() {
        let db = fresh_in_memory_db().await;
        let review = create_workbench_core(&db.conn, Some("Review".into()))
            .await
            .expect("create review");
        let experiments = create_workbench_core(&db.conn, Some("Experiments".into()))
            .await
            .expect("create experiments");

        set_workbench_pinned_core(&db.conn, review.id, true)
            .await
            .expect("pin review");
        let items = list_workbenches_core(&db.conn).await.expect("list");
        assert_eq!(items[0].id, review.id);
        assert!(items[0].is_pinned);
        assert_eq!(items[1].id, 1);
        assert_eq!(items[2].id, experiments.id);

        let unpinned = set_workbench_pinned_core(&db.conn, review.id, false)
            .await
            .expect("unpin review");
        assert!(!unpinned.is_pinned);
        assert_eq!(
            list_workbenches_core(&db.conn)
                .await
                .expect("list after unpin")
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec![1, review.id, experiments.id]
        );
    }

    #[tokio::test]
    async fn deleting_workbench_keeps_sessions_and_protects_last_workbench() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/codeg-workbench-delete").await;
        let conversation_id = seed_conversation(&db, folder_id, AgentType::Codex).await;
        let second = create_workbench_core(&db.conn, Some("Temporary".into()))
            .await
            .expect("create");
        let saved = save_workbench_tabs_core(
            &db.conn,
            &EventEmitter::Noop,
            second.id,
            vec![tab(folder_id, conversation_id)],
            0,
            "test".into(),
        )
        .await
        .expect("save");

        delete_workbench_core(&db.conn, second.id)
            .await
            .expect("delete workbench");
        let stale_save = save_workbench_tabs_core(
            &db.conn,
            &EventEmitter::Noop,
            second.id,
            vec![tab(folder_id, conversation_id)],
            saved.version,
            "test".into(),
        )
        .await
        .expect_err("a deleted workbench cannot be resurrected by a stale tab save");
        assert!(stale_save
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("Workbench")));
        let session =
            crate::db::service::conversation_service::get_by_id(&db.conn, conversation_id)
                .await
                .expect("query session");
        assert_eq!(
            session.id, conversation_id,
            "deleting a workbench must not delete sessions"
        );

        let error = delete_workbench_core(&db.conn, 1)
            .await
            .expect_err("last workbench is protected");
        assert!(error
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("last workbench")));
    }
}
