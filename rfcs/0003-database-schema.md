# RFC 0003: Database Schema & State Management

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-02-24
- **Updated:** 2026-02-24

## Overview

Define the complete database schema, SeaORM integration, and state management strategy for Kennel. This includes the full table definitions with indexes and constraints, state transition logic, port allocation, startup reconciliation, and preview database management.

## Motivation

The database is the source of truth for all Kennel state: which projects are registered, what deployments are active, which ports are allocated, and the status of builds. Getting the schema right is critical because:

1. **Every component depends on it.** The webhook receiver, builder, deployer, router, and API all query and mutate this state
1. **Concurrency is inherent.** Multiple webhooks can arrive simultaneously, builds run in parallel, deployments happen while builds are queued
1. **Crash recovery is essential.** Kennel must reconcile its database state against the actual system state (running systemd units, allocated ports, symlinks) on startup
1. **Migrations are hard to change.** Once deployed, schema changes require careful migration planning

## Goals

- Complete PostgreSQL schema with indexes, constraints, and enum types
- SeaORM entity generation and migration workflow
- State transition diagrams for builds and deployments
- Port allocation algorithm with conflict detection
- Preview database creation and teardown
- Startup reconciliation logic with detailed pseudocode
- Data retention and cleanup policies

## Non-Goals

- Query optimization (that comes after profiling with real workloads)
- Sharding or horizontal scaling (single PostgreSQL instance is sufficient for MVP)
- Multi-tenancy at the database level (all projects share one database)
- Audit logging (future enhancement)

## Detailed Design

### Database Schema

#### Enum Types

```sql
-- Repository types
CREATE TYPE repo_type AS ENUM ('forgejo', 'github');

-- Service types
CREATE TYPE service_type AS ENUM ('service', 'static', 'image');

-- Deployment status
CREATE TYPE deployment_status AS ENUM (
    'pending',
    'building',
    'active',
    'failed',
    'tearing_down',
    'torn_down'
);

-- Build status
CREATE TYPE build_status AS ENUM (
    'queued',
    'building',
    'success',
    'failed',
    'cancelled'
);

-- Build result status
CREATE TYPE build_result_status AS ENUM (
    'pending',
    'building',
    'success',
    'skipped',
    'failed'
);
```

#### Core Tables

```sql
-- Projects: registered Git repositories
CREATE TABLE projects (
    name            TEXT PRIMARY KEY,
    repo_url        TEXT NOT NULL,
    repo_type       repo_type NOT NULL,
    webhook_secret  TEXT NOT NULL,
    default_branch  TEXT NOT NULL DEFAULT 'main',
    created_at      TIMESTAMP NOT NULL DEFAULT now(),
    updated_at      TIMESTAMP NOT NULL DEFAULT now()
);

CREATE INDEX idx_projects_updated_at ON projects(updated_at);

-- Services: parsed from kennel.toml and cached
CREATE TABLE services (
    id              SERIAL PRIMARY KEY,
    project_name    TEXT NOT NULL REFERENCES projects(name) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    type            service_type NOT NULL,
    package         TEXT NOT NULL,
    health_check    TEXT,
    custom_domain   TEXT,
    spa             BOOLEAN NOT NULL DEFAULT false,
    created_at      TIMESTAMP NOT NULL DEFAULT now(),
    updated_at      TIMESTAMP NOT NULL DEFAULT now(),
    UNIQUE(project_name, name)
);

CREATE INDEX idx_services_project ON services(project_name);
CREATE INDEX idx_services_custom_domain ON services(custom_domain) WHERE custom_domain IS NOT NULL;

-- Deployments: running instances of services at specific refs
CREATE TABLE deployments (
    id              SERIAL PRIMARY KEY,
    project_name    TEXT NOT NULL,
    service_name    TEXT NOT NULL,
    branch          TEXT NOT NULL,
    branch_slug     TEXT NOT NULL,
    environment     TEXT NOT NULL,
    git_ref         TEXT NOT NULL,
    store_path      TEXT,
    port            INTEGER,
    status          deployment_status NOT NULL DEFAULT 'pending',
    domain          TEXT NOT NULL,
    created_at      TIMESTAMP NOT NULL DEFAULT now(),
    updated_at      TIMESTAMP NOT NULL DEFAULT now(),
    last_activity   TIMESTAMP NOT NULL DEFAULT now(),
    UNIQUE(project_name, service_name, branch),
    FOREIGN KEY (project_name, service_name)
        REFERENCES services(project_name, name) ON DELETE CASCADE
);

CREATE INDEX idx_deployments_lookup ON deployments(project_name, service_name, branch);
CREATE INDEX idx_deployments_domain ON deployments(domain);
CREATE INDEX idx_deployments_status ON deployments(status);
CREATE INDEX idx_deployments_activity ON deployments(last_activity) WHERE status != 'torn_down';
CREATE INDEX idx_deployments_port ON deployments(port) WHERE port IS NOT NULL;

-- Builds: per-push build jobs
CREATE TABLE builds (
    id              SERIAL PRIMARY KEY,
    project_name    TEXT NOT NULL REFERENCES projects(name) ON DELETE CASCADE,
    branch          TEXT NOT NULL,
    git_ref         TEXT NOT NULL,
    status          build_status NOT NULL DEFAULT 'queued',
    started_at      TIMESTAMP,
    finished_at     TIMESTAMP,
    created_at      TIMESTAMP NOT NULL DEFAULT now(),
    updated_at      TIMESTAMP NOT NULL DEFAULT now()
);

CREATE INDEX idx_builds_project_branch ON builds(project_name, branch);
CREATE INDEX idx_builds_status ON builds(status);
CREATE INDEX idx_builds_created_at ON builds(created_at DESC);

-- Build results: per-service results within a build
CREATE TABLE build_results (
    id              SERIAL PRIMARY KEY,
    build_id        INTEGER NOT NULL REFERENCES builds(id) ON DELETE CASCADE,
    service_name    TEXT NOT NULL,
    store_path      TEXT,
    status          build_result_status NOT NULL DEFAULT 'pending',
    changed         BOOLEAN NOT NULL DEFAULT true,
    log_path        TEXT,
    started_at      TIMESTAMP,
    finished_at     TIMESTAMP,
    created_at      TIMESTAMP NOT NULL DEFAULT now()
);

CREATE INDEX idx_build_results_build ON build_results(build_id);
CREATE INDEX idx_build_results_status ON build_results(build_id, status);

-- Port allocations: track which ports are in use
CREATE TABLE port_allocations (
    port            INTEGER PRIMARY KEY CHECK (port >= 18000 AND port <= 19999),
    deployment_id   INTEGER REFERENCES deployments(id) ON DELETE SET NULL,
    project_name    TEXT,
    service_name    TEXT,
    branch          TEXT,
    allocated_at    TIMESTAMP NOT NULL DEFAULT now()
);

CREATE INDEX idx_port_allocations_deployment ON port_allocations(deployment_id);

-- Preview databases: ephemeral databases for preview deployments
CREATE TABLE preview_databases (
    id              SERIAL PRIMARY KEY,
    project_name    TEXT NOT NULL REFERENCES projects(name) ON DELETE CASCADE,
    branch          TEXT NOT NULL,
    database_name   TEXT NOT NULL UNIQUE,
    valkey_db       INTEGER CHECK (valkey_db >= 0 AND valkey_db <= 15),
    created_at      TIMESTAMP NOT NULL DEFAULT now(),
    UNIQUE(project_name, branch)
);

CREATE INDEX idx_preview_databases_project ON preview_databases(project_name);
```

#### Triggers

```sql
-- Auto-update updated_at timestamps
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_projects_updated_at BEFORE UPDATE ON projects
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_services_updated_at BEFORE UPDATE ON services
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_deployments_updated_at BEFORE UPDATE ON deployments
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_builds_updated_at BEFORE UPDATE ON builds
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Update last_activity on git_ref change
CREATE OR REPLACE FUNCTION update_deployment_activity()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.git_ref != OLD.git_ref THEN
        NEW.last_activity = now();
    END IF;
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_deployments_activity BEFORE UPDATE ON deployments
    FOR EACH ROW EXECUTE FUNCTION update_deployment_activity();
```

### SeaORM Integration

#### Migration Structure

Migrations live in `crates/migration/` following SeaORM conventions:

```
crates/migration/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs
│   ├── m20260224_193925_create_enums.rs
│   ├── m20260224_193940_create_projects.rs
│   ├── m20260224_193940_create_services.rs
│   ├── m20260224_193940_create_deployments.rs
│   ├── m20260224_193940_create_builds.rs
│   ├── m20260224_195047_create_build_results.rs
│   ├── m20260224_193940_create_port_allocations.rs
│   ├── m20260224_193940_create_preview_databases.rs
│   └── m20260224_194026_create_triggers.rs
```

Migrations are generated using `sea-orm-cli`:

```bash
sea-orm-cli migrate generate -d crates/migration <migration_name>
```

Each migration implements `MigrationTrait`. PostgreSQL enums use SeaORM's native `Type::create()`:

```rust
use sea_orm_migration::prelude::*;
use sea_orm_migration::prelude::extension::postgres::Type;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create enum type using native SeaORM API
        manager
            .create_type(
                Type::create()
                    .as_enum(Alias::new("repo_type"))
                    .values(vec![Alias::new("forgejo"), Alias::new("github")])
                    .to_owned(),
            )
            .await?;

        // Create table using SeaORM schema builder
        manager
            .create_table(
                Table::create()
                    .table(Projects::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Projects::Name)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Projects::RepoUrl).text().not_null())
                    .col(
                        ColumnDef::new(Projects::RepoType)
                            .custom(Alias::new("repo_type"))
                            .not_null(),
                    )
                    .col(ColumnDef::new(Projects::WebhookSecret).text().not_null())
                    .col(
                        ColumnDef::new(Projects::DefaultBranch)
                            .text()
                            .not_null()
                            .default("main"),
                    )
                    .col(
                        ColumnDef::new(Projects::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Projects::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // Create index
        manager
            .create_index(
                Index::create()
                    .name("idx_projects_updated_at")
                    .table(Projects::Table)
                    .col(Projects::UpdatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Projects::Table).to_owned())
            .await?;

        manager
            .drop_type(Type::drop().name(Alias::new("repo_type")).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Name,
    RepoUrl,
    RepoType,
    WebhookSecret,
    DefaultBranch,
    CreatedAt,
    UpdatedAt,
}
```

For PostgreSQL-specific features like triggers and functions that don't have SeaORM abstractions, raw SQL is used via `Statement::from_string()`. See `m20260224_194026_create_triggers.rs` for examples.

Migrations are run automatically on Kennel startup via:

```rust
use migration::{Migrator, MigratorTrait};

Migrator::up(&db, None).await?;
```

#### Entity Generation

After schema changes, regenerate entities:

```bash
sea-orm-cli generate entity \
    -u postgres:///kennel?host=/run/postgresql \
    -o crates/entity/src \
    --with-serde both
```

This produces `crates/entity/src/projects.rs`, `deployments.rs`, etc. Entities are checked into Git so builds don't require a running database.

#### Repository Layer

The `kennel-store` crate provides a repository layer that abstracts database access:

```rust
// crates/kennel-store/src/projects.rs
use sea_orm::*;
use entity::projects;

pub struct ProjectRepository {
    db: DatabaseConnection,
}

impl ProjectRepository {
    pub async fn find_by_name(&self, name: &str) -> Result<Option<projects::Model>> {
        projects::Entity::find_by_id(name)
            .one(&self.db)
            .await
    }

    pub async fn create(&self, project: projects::ActiveModel) -> Result<projects::Model> {
        project.insert(&self.db).await
    }

    pub async fn list_all(&self) -> Result<Vec<projects::Model>> {
        projects::Entity::find()
            .order_by_asc(projects::Column::Name)
            .all(&self.db)
            .await
    }

    // ... etc
}
```

Repositories are injected into handlers via `axum::Extension<Arc<Repositories>>`.

### State Transitions

#### Build Lifecycle

```
queued ──> building ──> success
                   └──> failed
                   └──> cancelled (manual)
```

State transitions are enforced in code, not in the database schema. The `builds.status` column allows any transition, but the builder enforces the valid paths.

#### Deployment Lifecycle

```
pending ──> building ──> active ──> tearing_down ──> torn_down
                    └──> failed ──> tearing_down ──> torn_down
```

Deployments start as `pending` when first created. When a build completes successfully and the store path changed, the deployer transitions to `building`, then to `active` or `failed`. Teardown is a terminal state.

**Concurrency note**: Only one build per `(project, branch)` can be in `building` state at a time. If a webhook arrives while a build is already running, the webhook handler creates a new `queued` build but doesn't start it. The builder picks up queued builds in FIFO order.

### Port Allocation

#### Algorithm

Kennel maintains a pool of ports from 18000 to 19999 (2000 ports). When a new service deployment is created:

1. Query `port_allocations` for the lowest unallocated port in range
1. Insert a row with `(port, deployment_id, project_name, service_name, branch)`
1. Return the allocated port

Sequential allocation with reuse:

```rust
pub async fn allocate_port(
    &self,
    deployment_id: i32,
    project: &str,
    service: &str,
    branch: &str,
) -> Result<i32> {
    const PORT_MIN: i32 = 18000;
    const PORT_MAX: i32 = 19999;

    // Find first free port
    let allocated_ports: Vec<i32> = port_allocations::Entity::find()
        .select_only()
        .column(port_allocations::Column::Port)
        .into_tuple()
        .all(&self.db)
        .await?;

    let allocated_set: HashSet<i32> = allocated_ports.into_iter().collect();

    let port = (PORT_MIN..=PORT_MAX)
        .find(|p| !allocated_set.contains(p))
        .ok_or_else(|| anyhow!("Port pool exhausted"))?;

    // Insert allocation
    port_allocations::ActiveModel {
        port: Set(port),
        deployment_id: Set(Some(deployment_id)),
        project_name: Set(Some(project.to_string())),
        service_name: Set(Some(service.to_string())),
        branch: Set(Some(branch.to_string())),
        ..Default::default()
    }
    .insert(&self.db)
    .await?;

    Ok(port)
}

pub async fn release_port(&self, port: i32) -> Result<()> {
    port_allocations::Entity::delete_by_id(port)
        .exec(&self.db)
        .await?;
    Ok(())
}
```

#### Conflict Detection

On startup reconciliation, verify that no external process is listening on Kennel-allocated ports:

```rust
use std::net::TcpListener;

pub fn is_port_in_use(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_err()
}
```

If a port is in use but not listed in `port_allocations`, log a warning. If a port is allocated but not in use, investigate the deployment status.

### Preview Databases

#### Creation

When deploying a preview environment for a project with `postgres = true`:

```rust
pub async fn create_preview_database(
    &self,
    project: &str,
    branch: &str,
) -> Result<String> {
    let branch_slug = slugify(branch); // e.g., "pr-42" -> "pr_42"
    let db_name = format!("kennel_{}_{}", project, branch_slug);

    // Create database via raw SQL (SeaORM doesn't support CREATE DATABASE)
    self.db.execute_unprepared(&format!(
        "CREATE DATABASE {} OWNER kennel",
        db_name
    )).await?;

    // Allocate Valkey DB number (0-15)
    let valkey_db = self.allocate_valkey_db().await?;

    // Record in preview_databases table
    preview_databases::ActiveModel {
        project_name: Set(project.to_string()),
        branch: Set(branch.to_string()),
        database_name: Set(db_name.clone()),
        valkey_db: Set(Some(valkey_db)),
        ..Default::default()
    }
    .insert(&self.db)
    .await?;

    Ok(db_name)
}

fn slugify(branch: &str) -> String {
    branch
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .to_lowercase()
}
```

The `DATABASE_URL` is injected into the deployment's environment file:

```
DATABASE_URL=postgresql:///kennel_terrier_pr_42?host=/run/postgresql
```

#### Teardown

On deployment teardown:

```rust
pub async fn drop_preview_database(&self, project: &str, branch: &str) -> Result<()> {
    let preview_db = preview_databases::Entity::find()
        .filter(preview_databases::Column::ProjectName.eq(project))
        .filter(preview_databases::Column::Branch.eq(branch))
        .one(&self.db)
        .await?
        .ok_or_else(|| anyhow!("Preview database not found"))?;

    // Drop database
    self.db.execute_unprepared(&format!(
        "DROP DATABASE IF EXISTS {}",
        preview_db.database_name
    )).await?;

    // Release Valkey DB
    if let Some(valkey_db) = preview_db.valkey_db {
        self.release_valkey_db(valkey_db).await?;
    }

    // Remove record
    preview_databases::Entity::delete_by_id(preview_db.id)
        .exec(&self.db)
        .await?;

    Ok(())
}
```

Valkey keyspace is flushed via:

```rust
let mut conn = self.valkey_client.get_connection()?;
redis::cmd("SELECT").arg(valkey_db).execute(&mut conn);
redis::cmd("FLUSHDB").execute(&mut conn);
```

### Startup Reconciliation

When Kennel starts, before accepting webhooks or processing builds, it reconciles database state against system state.

#### Pseudocode

```rust
pub async fn reconcile(&self) -> Result<ReconciliationSummary> {
    let mut summary = ReconciliationSummary::default();

    // Step 1: Reconcile systemd units
    let running_units = list_kennel_systemd_units().await?;
    let db_deployments = self.store.deployments()
        .find_active_services()
        .await?;

    for unit in &running_units {
        if !db_deployments.contains_key(&unit.name) {
            // Orphaned unit
            warn!("Found orphaned systemd unit: {}", unit.name);
            stop_and_remove_unit(&unit.name).await?;
            summary.orphaned_units += 1;
        }
    }

    for (name, deployment) in &db_deployments {
        if !running_units.contains(name) {
            // Missing unit
            if is_store_path_valid(&deployment.store_path) {
                warn!("Restarting missing deployment: {}", name);
                self.deployer.start_service(deployment).await?;
                summary.restarted += 1;
            } else {
                warn!("Marking deployment as failed (invalid store path): {}", name);
                self.store.deployments()
                    .update_status(deployment.id, DeploymentStatus::Failed)
                    .await?;
                summary.marked_failed += 1;
            }
        }
    }

    // Step 2: Reconcile static site symlinks
    let static_deployments = self.store.deployments()
        .find_active_static()
        .await?;

    for deployment in &static_deployments {
        let symlink_path = format!(
            "/var/lib/kennel/sites/{}/{}/{}",
            deployment.project_name,
            deployment.service_name,
            deployment.branch_slug
        );

        if !symlink_is_valid(&symlink_path) {
            warn!("Removing broken symlink: {}", symlink_path);
            fs::remove_file(&symlink_path).ok();
            self.store.deployments()
                .update_status(deployment.id, DeploymentStatus::Failed)
                .await?;
            summary.broken_symlinks += 1;
        }
    }

    // Step 3: Audit port allocations
    let allocations = self.store.port_allocations().list_all().await?;

    for allocation in &allocations {
        if let Some(deployment_id) = allocation.deployment_id {
            if !deployment_exists(deployment_id).await? {
                warn!("Releasing port {} (deployment {} gone)", allocation.port, deployment_id);
                self.store.port_allocations()
                    .release_port(allocation.port)
                    .await?;
                summary.released_ports += 1;
            }
        }

        if is_port_in_use(allocation.port as u16) {
            // Port is in use, this is expected
        } else {
            warn!("Port {} allocated but not in use", allocation.port);
        }
    }

    // Step 4: Verify nginx configs
    let projects = self.store.projects().list_all().await?;
    let nginx_configs = list_nginx_configs("/var/lib/kennel/nginx/conf.d")?;

    for project in &projects {
        let expected_config = format!("{}.conf", project.name);
        if !nginx_configs.contains(&expected_config) {
            warn!("Regenerating missing nginx config for {}", project.name);
            self.nginx.write_config(project).await?;
            summary.nginx_configs_regenerated += 1;
        }
    }

    for config in &nginx_configs {
        let project_name = config.trim_end_matches(".conf");
        if !projects.iter().any(|p| p.name == project_name) {
            warn!("Removing orphaned nginx config: {}", config);
            fs::remove_file(format!("/var/lib/kennel/nginx/conf.d/{}", config))?;
            summary.nginx_configs_removed += 1;
        }
    }

    if summary.nginx_configs_regenerated > 0 || summary.nginx_configs_removed > 0 {
        reload_nginx().await?;
    }

    // Step 5: Refresh secrets
    for project in &projects {
        for env in &["prod", "staging", "dev", "preview"] {
            self.secrets.refresh(project.name, env).await.ok();
        }
    }
    summary.secrets_refreshed = projects.len() * 4;

    // Step 6: Health checks
    let active_services = self.store.deployments()
        .find_active_services()
        .await?;

    for deployment in &active_services {
        if let Some(health_check) = &deployment.health_check {
            match check_health(&deployment.domain, health_check).await {
                Ok(true) => summary.healthy += 1,
                Ok(false) | Err(_) => {
                    warn!("Health check failed for {}, restarting", deployment.domain);
                    self.deployer.restart_service(deployment).await?;
                    summary.restarted += 1;
                }
            }
        }
    }

    info!("Reconciliation complete: {:?}", summary);
    Ok(summary)
}

#[derive(Debug, Default)]
pub struct ReconciliationSummary {
    pub orphaned_units: usize,
    pub restarted: usize,
    pub marked_failed: usize,
    pub broken_symlinks: usize,
    pub released_ports: usize,
    pub nginx_configs_regenerated: usize,
    pub nginx_configs_removed: usize,
    pub secrets_refreshed: usize,
    pub healthy: usize,
}
```

#### Error Handling

Reconciliation is best-effort. If a step fails, log the error and continue. The system should reach a consistent state even if some components are broken.

### Data Retention & Cleanup

#### Auto-Expiry

Preview and non-production deployments are automatically torn down after 7 days of inactivity:

```rust
pub async fn cleanup_expired_deployments(&self) -> Result<usize> {
    let cutoff = Utc::now() - chrono::Duration::days(7);

    let expired = deployments::Entity::find()
        .filter(deployments::Column::LastActivity.lt(cutoff))
        .filter(deployments::Column::Status.eq("active"))
        .filter(deployments::Column::Environment.ne("prod"))
        .filter(deployments::Column::Environment.ne("staging"))
        .all(&self.db)
        .await?;

    let count = expired.len();

    for deployment in expired {
        info!("Tearing down expired deployment: {} {} {}",
            deployment.project_name, deployment.service_name, deployment.branch);
        self.deployer.teardown(&deployment).await?;
    }

    Ok(count)
}
```

Scheduled via a tokio interval:

```rust
let store = store.clone();
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(3600)); // hourly
    loop {
        interval.tick().await;
        if let Err(e) = store.cleanup_expired_deployments().await {
            error!("Auto-expiry cleanup failed: {}", e);
        }
    }
});
```

#### Build Log Retention

Build logs are stored in `/var/lib/kennel/logs/<build_id>/<service_name>.log`. The `build_results.log_path` column points to this file.

Logs are retained for 30 days:

```rust
pub async fn cleanup_old_logs(&self) -> Result<usize> {
    let cutoff = Utc::now() - chrono::Duration::days(30);

    let old_builds = builds::Entity::find()
        .filter(builds::Column::FinishedAt.lt(cutoff))
        .all(&self.db)
        .await?;

    let mut count = 0;

    for build in old_builds {
        let log_dir = format!("/var/lib/kennel/logs/{}", build.id);
        if fs::metadata(&log_dir).is_ok() {
            fs::remove_dir_all(&log_dir)?;
            count += 1;
        }
    }

    Ok(count)
}
```

#### Nix Garbage Collection

Nix store paths are reference-counted by Nix. Kennel doesn't track which paths are in use -- that's Nix's job. Periodically run `nix-collect-garbage`:

```rust
pub async fn run_nix_gc(&self) -> Result<()> {
    tokio::process::Command::new("nix-collect-garbage")
        .arg("--delete-older-than")
        .arg("14d")
        .output()
        .await?;
    Ok(())
}
```

Scheduled weekly via systemd timer (defined in the NixOS module).

#### Deployment Row Retention

Deployments in `torn_down` status are kept for 30 days for historical reference, then hard-deleted:

```rust
pub async fn cleanup_old_deployments(&self) -> Result<usize> {
    let cutoff = Utc::now() - chrono::Duration::days(30);

    let result = deployments::Entity::delete_many()
        .filter(deployments::Column::Status.eq("torn_down"))
        .filter(deployments::Column::UpdatedAt.lt(cutoff))
        .exec(&self.db)
        .await?;

    Ok(result.rows_affected as usize)
}
```

### Schema Evolution

#### Breaking Changes

Migrations that alter existing columns or add non-null constraints require coordination:

1. **Additive migration** -- add new column as nullable
1. **Backfill data** -- populate new column via UPDATE
1. **Non-null constraint** -- alter column to NOT NULL
1. **Remove old column** -- in a subsequent migration after confirming new column works

Example:

```rust
// Migration 1: Add new column
.col(ColumnDef::new(Deployments::NewField).text().null())

// Migration 2 (after deploy + backfill): Make it non-null
.modify_column(ColumnDef::new(Deployments::NewField).text().not_null())

// Migration 3 (after another deploy): Drop old column
.drop_column(Deployments::OldField)
```

## Alternatives Considered

**Text + CHECK constraints instead of PostgreSQL enums** -- Initially considered for flexibility, but rejected. The status and type fields are core to the system and won't change frequently. PostgreSQL enums provide better type safety and clearer schema documentation. When we do need to add variants, `ALTER TYPE ... ADD VALUE` is non-blocking in PostgreSQL 12+.

**Soft delete (status = 'deleted')** -- Considered but rejected. Hard deletes with a retention period are simpler and avoid accumulating stale rows. Audit logging (future) will capture historical state.

**SERIALIZABLE isolation level** -- Would prevent race conditions but adds overhead. Instead, rely on UNIQUE constraints and database-level conflict detection for critical operations (port allocation).

**Separate database per environment** -- Adds complexity for no benefit. All environments share one database, with rows scoped by `(project, branch, environment)`.

**SQLite instead of PostgreSQL** -- SQLite doesn't support concurrent writes well. PostgreSQL is required for Kennel's concurrency model.

## Open Questions

None.

## Implementation Phases

1. Write initial migrations for all tables with indexes and constraints
1. Generate SeaORM entities via `sea-orm-cli`
1. Implement repository layer in `kennel-store` crate
1. Write port allocation logic with tests
1. Write preview database creation/teardown helpers
1. Implement startup reconciliation logic
1. Add auto-expiry cleanup job
1. Add log retention cleanup job
1. Write integration tests against real PostgreSQL
1. Document migration workflow in CLAUDE.md
