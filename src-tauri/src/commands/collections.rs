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
    root_folder_id: Option<i32>,
) -> Result<CollectionInfo, AppCommandError> {
    collection_service::create(conn, name, parent_id, root_folder_id)
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

pub async fn place_collection_core(
    conn: &sea_orm::DatabaseConnection,
    id: i32,
    parent_id: Option<i32>,
    position: i32,
) -> Result<Vec<CollectionInfo>, AppCommandError> {
    collection_service::place(conn, id, parent_id, position)
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
    root_folder_id: Option<i32>,
) -> Result<CollectionInfo, AppCommandError> {
    create_collection_core(&db.conn, name, parent_id, root_folder_id).await
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
pub async fn place_collection(
    db: tauri::State<'_, AppDatabase>,
    id: i32,
    parent_id: Option<i32>,
    position: i32,
) -> Result<Vec<CollectionInfo>, AppCommandError> {
    place_collection_core(&db.conn, id, parent_id, position).await
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
        let folder = seed_folder(&db, "/tmp/codeg-collection-tree").await;
        let root = create_collection_core(&db.conn, "Research".into(), None, Some(folder))
            .await
            .expect("root");
        let child = create_collection_core(&db.conn, "Sources".into(), Some(root.id), None)
            .await
            .expect("child");
        let grandchild = create_collection_core(&db.conn, "Papers".into(), Some(child.id), None)
            .await
            .expect("grandchild");

        assert_eq!(root.root_folder_id, Some(folder));
        assert_eq!(child.root_folder_id, Some(folder));

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
        let first = create_collection_core(&db.conn, "First".into(), None, Some(folder))
            .await
            .expect("first");
        let second = create_collection_core(&db.conn, "Second".into(), None, Some(folder))
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

    #[tokio::test]
    async fn rooted_collections_reject_sessions_and_children_from_other_paths() {
        let db = fresh_in_memory_db().await;
        let first_folder = seed_folder(&db, "/tmp/codeg-collection-root-a").await;
        let second_folder = seed_folder(&db, "/tmp/codeg-collection-root-b").await;
        let first = create_collection_core(&db.conn, "First path".into(), None, Some(first_folder))
            .await
            .expect("first collection");
        let second =
            create_collection_core(&db.conn, "Second path".into(), None, Some(second_folder))
                .await
                .expect("second collection");
        let foreign_session = seed_conversation(&db, second_folder, AgentType::ClaudeCode).await;

        assert!(assign_conversations_to_collection_core(
            &db.conn,
            vec![foreign_session],
            Some(first.id),
        )
        .await
        .is_err());
        assert!(move_collection_core(&db.conn, second.id, Some(first.id))
            .await
            .is_err());
        assert!(
            place_collection_core(&db.conn, second.id, Some(first.id), 0)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn exact_placement_reorders_and_reparents_atomically() {
        let db = fresh_in_memory_db().await;
        let folder = seed_folder(&db, "/tmp/codeg-collection-placement").await;
        let first = create_collection_core(&db.conn, "First".into(), None, Some(folder))
            .await
            .expect("first");
        let second = create_collection_core(&db.conn, "Second".into(), None, Some(folder))
            .await
            .expect("second");
        let third = create_collection_core(&db.conn, "Third".into(), None, Some(folder))
            .await
            .expect("third");
        let child = create_collection_core(&db.conn, "Child".into(), Some(first.id), None)
            .await
            .expect("child");

        let reordered = place_collection_core(&db.conn, third.id, None, 0)
            .await
            .expect("reorder roots");
        let roots: Vec<_> = reordered
            .iter()
            .filter(|item| item.root_folder_id == Some(folder) && item.parent_id.is_none())
            .map(|item| (item.id, item.position))
            .collect();
        assert_eq!(roots, vec![(third.id, 0), (first.id, 1), (second.id, 2)]);

        let nested = place_collection_core(&db.conn, second.id, Some(first.id), 0)
            .await
            .expect("nest at first child");
        let roots: Vec<_> = nested
            .iter()
            .filter(|item| item.root_folder_id == Some(folder) && item.parent_id.is_none())
            .map(|item| (item.id, item.position))
            .collect();
        let children: Vec<_> = nested
            .iter()
            .filter(|item| item.parent_id == Some(first.id))
            .map(|item| (item.id, item.position))
            .collect();
        assert_eq!(roots, vec![(third.id, 0), (first.id, 1)]);
        assert_eq!(children, vec![(second.id, 0), (child.id, 1)]);

        assert!(place_collection_core(&db.conn, first.id, Some(child.id), 0)
            .await
            .is_err());
        assert!(place_collection_core(&db.conn, first.id, None, -1)
            .await
            .is_err());

        let after_rejections = list_collections_core(&db.conn).await.expect("list");
        assert_eq!(after_rejections, nested, "failed placements must roll back");
    }
}
