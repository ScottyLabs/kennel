use chrono::Utc;
use entity::{deployments, projects, sea_orm_active_enums::*, services};
use kennel_store::Store;
use sea_orm::{Database, DbErr, Set};

async fn setup_test_db() -> Result<Store, DbErr> {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://127.0.0.1:5432/kennel".to_string());

    let db = Database::connect(&db_url).await?;
    Ok(Store::new(db))
}

async fn create_test_deployment(
    store: &Store,
    project: &str,
    service: &str,
    branch: &str,
) -> Result<i32, DbErr> {
    let proj = projects::ActiveModel {
        name: Set(project.to_string()),
        repo_url: Set(format!("https://github.com/{}", project)),
        repo_type: Set(RepoType::Github),
        webhook_secret: Set("secret".to_string()),
        default_branch: Set("main".to_string()),
        ..Default::default()
    };

    let _ = store.projects().create(proj).await.ok();

    let svc = services::ActiveModel {
        project_name: Set(project.to_string()),
        name: Set(service.to_string()),
        r#type: Set(ServiceType::Service),
        package: Set("default".to_string()),
        ..Default::default()
    };

    let _ = store.services().create(svc).await.ok();

    let now = Utc::now().naive_utc();
    let dep = deployments::ActiveModel {
        project_name: Set(project.to_string()),
        service_name: Set(service.to_string()),
        branch: Set(branch.to_string()),
        branch_slug: Set(branch.replace("-", "_")),
        environment: Set("test".to_string()),
        git_ref: Set("abc123".to_string()),
        domain: Set(format!("{}-{}-{}.test.com", project, service, branch)),
        status: Set(DeploymentStatus::Active),
        dns_status: Set("pending".to_string()),
        created_at: Set(now),
        updated_at: Set(now),
        last_activity: Set(now),
        ..Default::default()
    };

    let created = store.deployments().create(dep).await?;
    Ok(created.id)
}

async fn cleanup(store: &Store, project: &str, port: Option<i32>) {
    if let Some(p) = port {
        let _ = store.port_allocations().release_port(p).await;
    }
    let _ = store.projects().delete(project).await;
}

#[tokio::test]
async fn test_allocate_port() {
    let store = setup_test_db().await.expect("Failed to connect");

    let deployment_id = create_test_deployment(&store, "test-alloc", "web", "main")
        .await
        .expect("Failed to create deployment");

    let port = store
        .port_allocations()
        .allocate_port(deployment_id, "test-alloc", "web", "main")
        .await
        .expect("Failed to allocate port");

    assert!(port >= 18000 && port <= 19999);

    let allocated = store
        .port_allocations()
        .find_by_port(port)
        .await
        .expect("Failed to find")
        .expect("Should exist");

    assert_eq!(allocated.port, port);
    assert_eq!(allocated.deployment_id, Some(deployment_id));

    cleanup(&store, "test-alloc", Some(port)).await;
}

#[tokio::test]
async fn test_allocate_multiple_ports() {
    let store = setup_test_db().await.expect("Failed to connect");

    let id1 = create_test_deployment(&store, "test-multi1", "web", "main")
        .await
        .expect("Failed to create deployment 1");
    let id2 = create_test_deployment(&store, "test-multi2", "api", "dev")
        .await
        .expect("Failed to create deployment 2");

    let port1 = store
        .port_allocations()
        .allocate_port(id1, "test-multi1", "web", "main")
        .await
        .expect("Failed to allocate port 1");

    let port2 = store
        .port_allocations()
        .allocate_port(id2, "test-multi2", "api", "dev")
        .await
        .expect("Failed to allocate port 2");

    assert_ne!(port1, port2);

    cleanup(&store, "test-multi1", Some(port1)).await;
    cleanup(&store, "test-multi2", Some(port2)).await;
}

#[tokio::test]
async fn test_port_reuse_after_release() {
    let store = setup_test_db().await.expect("Failed to connect");

    let deployment_id = create_test_deployment(&store, "test-reuse", "web", "main")
        .await
        .expect("Failed to create deployment");

    let port = store
        .port_allocations()
        .allocate_port(deployment_id, "test-reuse", "web", "main")
        .await
        .expect("Failed to allocate");

    store
        .port_allocations()
        .release_port(port)
        .await
        .expect("Failed to release");

    let available = store
        .port_allocations()
        .is_port_available(port)
        .await
        .expect("Failed to check");

    assert!(available);

    cleanup(&store, "test-reuse", None).await;
}

#[tokio::test]
async fn test_port_range_validation() {
    use kennel_store::port_allocations::PortAllocationRepository;

    assert!(PortAllocationRepository::is_port_in_range(18000));
    assert!(PortAllocationRepository::is_port_in_range(19000));
    assert!(PortAllocationRepository::is_port_in_range(19999));
    assert!(!PortAllocationRepository::is_port_in_range(17999));
    assert!(!PortAllocationRepository::is_port_in_range(20000));
}
