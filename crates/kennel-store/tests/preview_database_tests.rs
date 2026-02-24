use entity::{projects, sea_orm_active_enums::RepoType};
use kennel_store::Store;
use sea_orm::{Database, DbErr, Set};

async fn setup_test_db() -> Result<Store, DbErr> {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://127.0.0.1:5432/kennel".to_string());

    let db = Database::connect(&db_url).await?;
    Ok(Store::new(db))
}

async fn create_test_project(store: &Store, name: &str) -> Result<(), DbErr> {
    let proj = projects::ActiveModel {
        name: Set(name.to_string()),
        repo_url: Set(format!("https://github.com/{}", name)),
        repo_type: Set(RepoType::Github),
        webhook_secret: Set("secret".to_string()),
        default_branch: Set("main".to_string()),
        ..Default::default()
    };

    let _ = store.projects().create(proj).await.ok();
    Ok(())
}

async fn cleanup(store: &Store, project: &str) {
    let _ = store.projects().delete(project).await;
}

#[tokio::test]
async fn test_create_preview_database() {
    let store = setup_test_db().await.expect("Failed to connect");

    create_test_project(&store, "preview-test1")
        .await
        .expect("Failed to create project");

    let preview_db = store
        .preview_databases()
        .create_preview_database("preview-test1", "pr-123", "kennel_preview_test1_pr_123")
        .await
        .expect("Failed to create preview db");

    assert_eq!(preview_db.project_name, "preview-test1");
    assert_eq!(preview_db.branch, "pr-123");
    assert!(preview_db.valkey_db.is_some());

    let valkey_db = preview_db.valkey_db.unwrap();
    assert!(valkey_db >= 0 && valkey_db <= 15);

    cleanup(&store, "preview-test1").await;
}

#[tokio::test]
async fn test_find_by_project_and_branch() {
    let store = setup_test_db().await.expect("Failed to connect");

    create_test_project(&store, "preview-test2")
        .await
        .expect("Failed to create project");

    let created = store
        .preview_databases()
        .create_preview_database("preview-test2", "feature", "kennel_preview_test2_feature")
        .await
        .expect("Failed to create");

    let found = store
        .preview_databases()
        .find_by_project_and_branch("preview-test2", "feature")
        .await
        .expect("Failed to find")
        .expect("Should exist");

    assert_eq!(found.id, created.id);
    assert_eq!(found.database_name, "kennel_preview_test2_feature");

    cleanup(&store, "preview-test2").await;
}

#[tokio::test]
async fn test_allocate_multiple_valkey_dbs() {
    let store = setup_test_db().await.expect("Failed to connect");

    create_test_project(&store, "preview-test3a")
        .await
        .expect("Failed to create project 1");
    create_test_project(&store, "preview-test3b")
        .await
        .expect("Failed to create project 2");

    let db1 = store
        .preview_databases()
        .create_preview_database("preview-test3a", "b1", "kennel_preview_test3a_b1")
        .await
        .expect("Failed to create db1");

    let db2 = store
        .preview_databases()
        .create_preview_database("preview-test3b", "b2", "kennel_preview_test3b_b2")
        .await
        .expect("Failed to create db2");

    let valkey1 = db1.valkey_db.expect("Should have valkey");
    let valkey2 = db2.valkey_db.expect("Should have valkey");

    assert_ne!(valkey1, valkey2);

    cleanup(&store, "preview-test3a").await;
    cleanup(&store, "preview-test3b").await;
}

#[tokio::test]
async fn test_delete_by_project_and_branch() {
    let store = setup_test_db().await.expect("Failed to connect");

    create_test_project(&store, "preview-test4")
        .await
        .expect("Failed to create project");

    store
        .preview_databases()
        .create_preview_database("preview-test4", "temp", "kennel_preview_test4_temp")
        .await
        .expect("Failed to create");

    let found_before = store
        .preview_databases()
        .find_by_project_and_branch("preview-test4", "temp")
        .await
        .expect("Failed to find");

    assert!(found_before.is_some());

    store
        .preview_databases()
        .delete_by_project_and_branch("preview-test4", "temp")
        .await
        .expect("Failed to delete");

    let found_after = store
        .preview_databases()
        .find_by_project_and_branch("preview-test4", "temp")
        .await
        .expect("Failed to check");

    assert!(found_after.is_none());

    cleanup(&store, "preview-test4").await;
}

#[tokio::test]
async fn test_valkey_db_range_validation() {
    use kennel_store::preview_databases::PreviewDatabaseRepository;

    assert!(PreviewDatabaseRepository::is_valkey_db_in_range(0));
    assert!(PreviewDatabaseRepository::is_valkey_db_in_range(8));
    assert!(PreviewDatabaseRepository::is_valkey_db_in_range(15));
    assert!(!PreviewDatabaseRepository::is_valkey_db_in_range(-1));
    assert!(!PreviewDatabaseRepository::is_valkey_db_in_range(16));
}
