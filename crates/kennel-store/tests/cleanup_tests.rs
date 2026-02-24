use chrono::{Duration, Utc};
use entity::{builds, deployments, projects, sea_orm_active_enums::*, services};
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

async fn create_test_service(store: &Store, project: &str, service: &str) -> Result<(), DbErr> {
    let svc = services::ActiveModel {
        project_name: Set(project.to_string()),
        name: Set(service.to_string()),
        r#type: Set(ServiceType::Service),
        package: Set("default".to_string()),
        ..Default::default()
    };

    let _ = store.services().create(svc).await.ok();
    Ok(())
}

async fn cleanup(store: &Store, project: &str) {
    let _ = store.projects().delete(project).await;
}

#[tokio::test]
async fn test_find_expired_deployments() {
    let store = setup_test_db().await.expect("Failed to connect");

    create_test_project(&store, "cleanup-test1")
        .await
        .expect("Failed to create project");
    create_test_service(&store, "cleanup-test1", "web")
        .await
        .expect("Failed to create service");

    let old_activity = Utc::now().naive_utc() - Duration::days(10);

    let deployment = deployments::ActiveModel {
        project_name: Set("cleanup-test1".to_string()),
        service_name: Set("web".to_string()),
        branch: Set("old-branch".to_string()),
        branch_slug: Set("old_branch".to_string()),
        environment: Set("dev".to_string()),
        git_ref: Set("abc123".to_string()),
        domain: Set("old.test.com".to_string()),
        status: Set(DeploymentStatus::Active),
        last_activity: Set(old_activity),
        ..Default::default()
    };

    let created = store
        .deployments()
        .create(deployment)
        .await
        .expect("Failed to create deployment");

    let expired = store
        .find_expired_deployments(7)
        .await
        .expect("Failed to find expired");

    assert!(
        expired.iter().any(|d| d.id == created.id),
        "Should find deployment inactive for 10 days when threshold is 7"
    );

    cleanup(&store, "cleanup-test1").await;
}

#[tokio::test]
async fn test_find_expired_excludes_prod() {
    let store = setup_test_db().await.expect("Failed to connect");

    create_test_project(&store, "cleanup-test2")
        .await
        .expect("Failed to create project");
    create_test_service(&store, "cleanup-test2", "api")
        .await
        .expect("Failed to create service");

    let old_activity = Utc::now().naive_utc() - Duration::days(10);

    let deployment = deployments::ActiveModel {
        project_name: Set("cleanup-test2".to_string()),
        service_name: Set("api".to_string()),
        branch: Set("main".to_string()),
        branch_slug: Set("main".to_string()),
        environment: Set("prod".to_string()),
        git_ref: Set("def456".to_string()),
        domain: Set("prod.test.com".to_string()),
        status: Set(DeploymentStatus::Active),
        last_activity: Set(old_activity),
        ..Default::default()
    };

    let created = store
        .deployments()
        .create(deployment)
        .await
        .expect("Failed to create prod deployment");

    let expired = store
        .find_expired_deployments(7)
        .await
        .expect("Failed to find expired");

    assert!(
        !expired.iter().any(|d| d.id == created.id),
        "Prod deployments should be excluded from expiry"
    );

    cleanup(&store, "cleanup-test2").await;
}

#[tokio::test]
async fn test_find_old_builds() {
    let store = setup_test_db().await.expect("Failed to connect");

    create_test_project(&store, "cleanup-test3")
        .await
        .expect("Failed to create project");

    let old_finish_time = Utc::now().naive_utc() - Duration::days(35);

    let build = builds::ActiveModel {
        project_name: Set("cleanup-test3".to_string()),
        branch: Set("old-build".to_string()),
        git_ref: Set("xyz789".to_string()),
        status: Set(BuildStatus::Success),
        finished_at: Set(Some(old_finish_time)),
        ..Default::default()
    };

    let created = store
        .builds()
        .create(build)
        .await
        .expect("Failed to create build");

    let old_builds = store
        .find_old_builds(30)
        .await
        .expect("Failed to find old builds");

    assert!(
        old_builds.iter().any(|b| b.id == created.id),
        "Should find build finished 35 days ago when threshold is 30"
    );

    cleanup(&store, "cleanup-test3").await;
}

#[tokio::test]
async fn test_find_old_builds_excludes_unfinished() {
    let store = setup_test_db().await.expect("Failed to connect");

    create_test_project(&store, "cleanup-test4")
        .await
        .expect("Failed to create project");

    let build = builds::ActiveModel {
        project_name: Set("cleanup-test4".to_string()),
        branch: Set("running".to_string()),
        git_ref: Set("abc".to_string()),
        status: Set(BuildStatus::Building),
        finished_at: Set(None),
        ..Default::default()
    };

    let created = store
        .builds()
        .create(build)
        .await
        .expect("Failed to create running build");

    let old_builds = store
        .find_old_builds(30)
        .await
        .expect("Failed to find old builds");

    assert!(
        !old_builds.iter().any(|b| b.id == created.id),
        "Unfinished builds should be excluded from cleanup"
    );

    cleanup(&store, "cleanup-test4").await;
}
