use crate::app_error::AppCommandError;
use crate::db::service::workbench_service;
#[cfg(feature = "tauri-runtime")]
use crate::db::AppDatabase;
use crate::models::WorkbenchInfo;

pub async fn list_workbenches_core(
    conn: &sea_orm::DatabaseConnection,
) -> Result<Vec<WorkbenchInfo>, AppCommandError> {
    workbench_service::list(conn)
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
        assert!(stale_save.to_string().contains("Workbench"));
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
        assert!(error.to_string().contains("last workbench"));
    }
}
