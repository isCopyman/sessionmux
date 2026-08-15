use crate::app_error::AppCommandError;
use crate::db::service::collection_service;
#[cfg(feature = "tauri-runtime")]
use crate::db::AppDatabase;
use crate::models::{CollectionInfo, ConversationCollectionRef};

pub async fn list_collections_core(
    conn: &sea_orm::DatabaseConnection,
) -> Result<Vec<CollectionInfo>, AppCommandError> {
    collection_service::list(conn)
        .await
        .map_err(AppCommandError::from)
}

pub async fn list_conversation_collection_refs_core(
    conn: &sea_orm::DatabaseConnection,
    conversation_ids: Vec<i32>,
) -> Result<Vec<ConversationCollectionRef>, AppCommandError> {
    collection_service::list_conversation_refs(conn, conversation_ids)
        .await
        .map_err(AppCommandError::from)
}

pub async fn create_collection_core(
    conn: &sea_orm::DatabaseConnection,
    name: String,
    parent_id: Option<i32>,
) -> Result<CollectionInfo, AppCommandError> {
    collection_service::create(conn, name, parent_id)
        .await
        .map_err(AppCommandError::from)
}

pub async fn rename_collection_core(
    conn: &sea_orm::DatabaseConnection,
    id: i32,
    name: String,
) -> Result<CollectionInfo, AppCommandError> {
    collection_service::rename(conn, id, name)
        .await
        .map_err(AppCommandError::from)
}

pub async fn move_collection_core(
    conn: &sea_orm::DatabaseConnection,
    id: i32,
    parent_id: Option<i32>,
) -> Result<CollectionInfo, AppCommandError> {
    collection_service::move_to(conn, id, parent_id)
        .await
        .map_err(AppCommandError::from)
}

pub async fn delete_collection_core(
    conn: &sea_orm::DatabaseConnection,
    id: i32,
) -> Result<(), AppCommandError> {
    collection_service::delete(conn, id)
        .await
        .map_err(AppCommandError::from)
}

pub async fn assign_conversations_to_collection_core(
    conn: &sea_orm::DatabaseConnection,
    conversation_ids: Vec<i32>,
    collection_id: Option<i32>,
) -> Result<Vec<ConversationCollectionRef>, AppCommandError> {
    collection_service::assign_conversations(conn, conversation_ids, collection_id)
        .await
        .map_err(AppCommandError::from)
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn list_collections(
    db: tauri::State<'_, AppDatabase>,
) -> Result<Vec<CollectionInfo>, AppCommandError> {
    list_collections_core(&db.conn).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn list_conversation_collection_refs(
    db: tauri::State<'_, AppDatabase>,
    conversation_ids: Vec<i32>,
) -> Result<Vec<ConversationCollectionRef>, AppCommandError> {
    list_conversation_collection_refs_core(&db.conn, conversation_ids).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn create_collection(
    db: tauri::State<'_, AppDatabase>,
    name: String,
    parent_id: Option<i32>,
) -> Result<CollectionInfo, AppCommandError> {
    create_collection_core(&db.conn, name, parent_id).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn rename_collection(
    db: tauri::State<'_, AppDatabase>,
    id: i32,
    name: String,
) -> Result<CollectionInfo, AppCommandError> {
    rename_collection_core(&db.conn, id, name).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn move_collection(
    db: tauri::State<'_, AppDatabase>,
    id: i32,
    parent_id: Option<i32>,
) -> Result<CollectionInfo, AppCommandError> {
    move_collection_core(&db.conn, id, parent_id).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn delete_collection(
    db: tauri::State<'_, AppDatabase>,
    id: i32,
) -> Result<(), AppCommandError> {
    delete_collection_core(&db.conn, id).await
}

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn assign_conversations_to_collection(
    db: tauri::State<'_, AppDatabase>,
    conversation_ids: Vec<i32>,
    collection_id: Option<i32>,
) -> Result<Vec<ConversationCollectionRef>, AppCommandError> {
    assign_conversations_to_collection_core(&db.conn, conversation_ids, collection_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::service::conversation_service;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
    use crate::models::AgentType;

    #[tokio::test]
    async fn collections_nest_but_cannot_form_cycles() {
        let db = fresh_in_memory_db().await;
        let root = create_collection_core(&db.conn, "Research".into(), None)
            .await
            .expect("root");
        let child = create_collection_core(&db.conn, "Sources".into(), Some(root.id))
            .await
            .expect("child");
        let grandchild = create_collection_core(&db.conn, "Papers".into(), Some(child.id))
            .await
            .expect("grandchild");

        assert!(move_collection_core(&db.conn, root.id, Some(grandchild.id))
            .await
            .is_err());
        let items = list_collections_core(&db.conn).await.expect("list");
        assert_eq!(
            items
                .iter()
                .find(|item| item.id == child.id)
                .unwrap()
                .parent_id,
            Some(root.id)
        );
    }

    #[tokio::test]
    async fn moving_sessions_is_unique_and_deleting_collection_is_nondestructive() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-collection-membership").await;
        let conversation = seed_conversation(&db, folder, AgentType::Codex).await;
        let first = create_collection_core(&db.conn, "First".into(), None)
            .await
            .expect("first");
        let second = create_collection_core(&db.conn, "Second".into(), None)
            .await
            .expect("second");

        assign_conversations_to_collection_core(&db.conn, vec![conversation], Some(first.id))
            .await
            .expect("assign first");
        let refs =
            assign_conversations_to_collection_core(&db.conn, vec![conversation], Some(second.id))
                .await
                .expect("move second");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].collection_id, second.id);

        delete_collection_core(&db.conn, second.id)
            .await
            .expect("delete collection");
        assert!(
            list_conversation_collection_refs_core(&db.conn, vec![conversation])
                .await
                .expect("refs after delete")
                .is_empty()
        );
        assert!(conversation_service::get_by_id(&db.conn, conversation)
            .await
            .is_ok());
    }
}
