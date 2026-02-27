# RFC 0004: Core Pipeline Architecture

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-02-24
- **Updated:** 2026-02-24

## Overview

Define the architecture for Kennel's core pipeline: the webhook receiver, builder, deployer, and router components that transform Git push events into running deployments. This RFC specifies the interfaces, data flows, error handling, and integration patterns for single-service projects with branch and PR deployments.

## Motivation

RFC 0003 established the database schema and state machine. Now we need to design the orchestration logic that drives state transitions. Getting this architecture right is critical because:

1. **Webhook integrity matters.** Missing or double-processing webhook events breaks the deploy pipeline. We need idempotency and clear error boundaries.
1. **Builds are expensive.** Nix builds can take minutes and consume CPU/RAM. Worker pools, concurrency limits, and cancellation need careful design.
1. **Deployments have side effects.** Creating systemd units, allocating ports, and generating secrets must be atomic and reversible on failure.
1. **Routing must be fast and correct.** The router handles all HTTP traffic. Incorrect routing breaks deployed apps; slow routing adds latency.
1. **Error recovery is essential.** Crashes mid-deploy leave orphaned resources. Startup reconciliation (RFC 0003) relies on clean component boundaries.

## Goals

- Single HTTP endpoint that handles Forgejo and GitHub webhooks
- Worker pool that builds Nix derivations concurrently with bounded parallelism
- Deployer that manages systemd units, static sites, ports, secrets, and preview databases
- Reverse proxy router that looks up deployments by domain and forwards traffic
- Clear error boundaries and rollback semantics for each component
- Integration with RFC 0003 database state machine and resource allocation
- Structured logging for observability

## Non-Goals

- Multi-service projects -- RFC 0004 designs for single-service only
- PR status checks integration -- PR deployments supported, but no GitHub/Forgejo status check API integration
- Dashboard UI -- API design deferred to separate RFC
- A/B testing and cookie-based routing -- router keeps this as future extension point
- Container orchestration (Kubernetes, etc.) -- systemd only
- Multi-host deployments -- single Kennel host

## Detailed Design

### Architecture Overview

Kennel runs as a single binary with four main subsystems:

1. **Webhook Receiver** - Axum HTTP server accepting POST /webhook/:project
1. **Builder** - Worker pool that builds Nix derivations
1. **Deployer** - Lifecycle manager for systemd services and static sites
1. **Router** - Reverse proxy that routes traffic to active deployments

Pipeline flow:

```
Git Push -> Webhook Receiver -> Builder -> Deployer -> Router -> Traffic
```

All components share:

- Database connection pool (from kennel-store)
- Tokio runtime
- Structured logging (tracing crate)
- Graceful shutdown signal (from kennel-config)

Communication patterns:

- Webhook Receiver -> Builder: `mpsc::channel<BuildId>`
- Builder -> Deployer: `mpsc::channel<DeploymentRequest>`
- Deployer -> Router: `broadcast::channel<RouterUpdate>`
- All components -> Database: shared `Arc<Store>`

### Webhook Receiver

#### Responsibilities

- Accept webhook POST requests from Git platforms (Forgejo, GitHub)
- Verify request signatures (HMAC-SHA256)
- Parse webhook JSON payloads
- Determine if the event should trigger a build (push to configured branches)
- Create build records in database
- Enqueue build jobs for the Builder
- Return HTTP responses to webhook sender

#### HTTP Endpoint Design

Single endpoint handles all webhook events:

```
POST /webhook/:project
```

Path parameter `project` must match `projects.name` in database. If not found, return 404.

Headers required:

- `X-Forgejo-Event` or `X-GitHub-Event` - event type (push, pull_request, etc.)
- `X-Forgejo-Signature` or `X-Hub-Signature-256` - HMAC signature

Request body: JSON payload specific to Git platform

Response codes:

- 200 OK - webhook processed, build enqueued
- 202 Accepted - webhook received but no build needed (e.g., deleted branch)
- 400 Bad Request - malformed JSON or missing headers
- 401 Unauthorized - signature verification failed
- 404 Not Found - project not found
- 500 Internal Server Error - database or internal error

#### Signature Verification

Forgejo:

```
HMAC-SHA256(secret, body) == X-Forgejo-Signature
```

GitHub:

```
"sha256=" + hex(HMAC-SHA256(secret, body)) == X-Hub-Signature-256
```

Secret is `projects.webhook_secret` from database. If signature verification fails, return 401 and log the attempt (project name, IP address, event type, timestamp).

#### Event Parsing

**Push Events:**

Forgejo JSON structure:

```json
{
  "ref": "refs/heads/main",
  "before": "abc123...",
  "after": "def456...",
  "repository": {
    "clone_url": "https://forgejo.example.com/org/repo.git"
  },
  "pusher": {
    "username": "alice"
  }
}
```

GitHub JSON structure:

```json
{
  "ref": "refs/heads/main",
  "before": "abc123...",
  "after": "def456...",
  "repository": {
    "clone_url": "https://github.com/org/repo.git"
  },
  "pusher": {
    "name": "alice"
  }
}
```

Extract:

- `git_ref` - branch name (strip "refs/heads/" prefix)
- `commit_sha` - the "after" commit (40-char hex string)
- `author` - pusher username/name

If `after` is all zeros (0000000000000000000000000000000000000000), this is a branch deletion. Mark any active deployments for this branch for teardown and return 202.

**Pull Request Events:**

Forgejo JSON structure (opened/synchronize):

```json
{
  "action": "opened",
  "number": 42,
  "pull_request": {
    "head": {
      "sha": "abc123...",
      "ref": "feature-branch"
    },
    "base": {
      "ref": "main"
    }
  }
}
```

GitHub JSON structure (opened/synchronize):

```json
{
  "action": "opened",
  "number": 42,
  "pull_request": {
    "head": {
      "sha": "abc123..."
    },
    "base": {
      "ref": "main"
    }
  }
}
```

PR actions that trigger builds:

- `opened` - PR created, trigger build
- `synchronize` (GitHub) / `synchronized` (Forgejo) - new commits pushed, trigger build
- `reopened` - PR reopened, trigger build

PR actions that trigger teardown:

- `closed` - PR closed or merged, teardown deployment

For build-triggering actions:

- Extract `pr_number`, `commit_sha`, `head_ref`
- Create build with `git_ref = "pr-<number>"` (e.g., "pr-42")
- Deployment will be accessible at `<service>-pr-42.<project>.scottylabs.org`

For teardown actions:

- Mark deployments with `git_ref = "pr-<number>"` for teardown

#### Build Record Creation

For valid push events:

1. Look up project by name: `SELECT name, repo_url, repo_type, webhook_secret FROM projects WHERE name = :project`
1. Check if build already exists: `SELECT id FROM builds WHERE project_name = :name AND git_ref = :ref AND commit_sha = :sha`
1. If exists, return 200 (idempotent)
1. If not, insert build:
   ```sql
   INSERT INTO builds (project_name, git_ref, commit_sha, status, triggered_by, created_at)
   VALUES (:project_name, :git_ref, :commit_sha, 'queued', :author, NOW())
   RETURNING id
   ```
1. Send build ID to Builder via channel
1. Return 200

#### Error Handling

- **Signature verification failure:** log project name, IP address, event type; return 401
- **Database errors:** log full error context; return 500; rely on webhook sender retry
- **Invalid JSON:** log error; return 400
- **Builder channel send failure:** return 503 Service Unavailable; webhook sender will retry

#### Pseudocode

```rust
async fn handle_webhook(
    Path(project_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
    store: Arc<Store>,
    build_tx: mpsc::Sender<i64>,
) -> Result<StatusCode, WebhookError> {
    // Verify signature
    let project = store.projects().find_by_name(&project_name).await?
        .ok_or(WebhookError::NotFound)?;
    verify_signature(&headers, &body, &project.webhook_secret)?;

    // Parse event
    let event = parse_webhook_event(&headers, &body)?;
    
    match event {
        WebhookEvent::Push { git_ref, commit_sha, author, deleted } => {
            if deleted {
                store.deployments()
                    .mark_for_teardown(&project.name, &git_ref)
                    .await?;
                return Ok(StatusCode::ACCEPTED);
            }

            // Check for duplicate
            if store.builds().exists(&project.name, &git_ref, &commit_sha).await? {
                return Ok(StatusCode::OK);
            }

            // Create build
            let build = store.builds().create(
                &project.name,
                git_ref,
                commit_sha,
                author,
            ).await?;

            // Enqueue
            build_tx.send(build.id).await
                .map_err(|_| WebhookError::BuilderUnavailable)?;

            Ok(StatusCode::OK)
        }
        WebhookEvent::PullRequest { action, pr_number, commit_sha, author } => {
            match action.as_str() {
                "opened" | "synchronize" | "synchronized" | "reopened" => {
                    let git_ref = format!("pr-{}", pr_number);
                    
                    // Check for duplicate
                    if store.builds().exists(&project.name, &git_ref, &commit_sha).await? {
                        return Ok(StatusCode::OK);
                    }
                    
                    // Create build
                    let build = store.builds().create(
                        &project.name,
                        git_ref,
                        commit_sha,
                        author,
                    ).await?;
                    
                    // Enqueue
                    build_tx.send(build.id).await
                        .map_err(|_| WebhookError::BuilderUnavailable)?;
                    
                    Ok(StatusCode::OK)
                }
                "closed" => {
                    let git_ref = format!("pr-{}", pr_number);
                    store.deployments()
                        .mark_for_teardown(&project.name, &git_ref)
                        .await?;
                    Ok(StatusCode::ACCEPTED)
                }
                _ => {
                    tracing::debug!("Ignoring PR action: {}", action);
                    Ok(StatusCode::ACCEPTED)
                }
            }
        }
    }
}
```

### Builder

#### Responsibilities

- Receive build jobs from webhook receiver
- Clone Git repository at specified commit
- Parse kennel.toml configuration
- Build Nix derivations for each service
- Compare store paths to detect unchanged builds
- Push build artifacts to Cachix (if configured)
- Record build results per service
- Update build status (queued -> building -> success/failed)
- Trigger Deployer on successful builds

#### Worker Pool Architecture

Use tokio spawn with bounded parallelism via semaphore:

```rust
struct BuildWorker {
    store: Arc<Store>,
    config: BuildConfig,
    deploy_tx: mpsc::Sender<DeploymentRequest>,
}

async fn run_worker_pool(
    mut build_rx: mpsc::Receiver<i64>,
    concurrency: usize,
    worker: BuildWorker,
) {
    let semaphore = Arc::new(Semaphore::new(concurrency));
    
    while let Some(build_id) = build_rx.recv().await {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let worker = worker.clone();
        
        tokio::spawn(async move {
            if let Err(e) = worker.process_build(build_id).await {
                tracing::error!("Build {} failed: {}", build_id, e);
            }
            drop(permit);
        });
    }
}
```

Configuration:

- `max_concurrent_builds` - default 2 (adjustable based on host resources)

#### Build Lifecycle

For each build ID received:

1. **Update status:**

   ```sql
   UPDATE builds SET status = 'building', started_at = NOW() WHERE id = :id
   ```

1. **Check for cancellation:**
   Before each major step (clone, parse, build each service), check if build status changed to 'cancelled':

   ```rust
   let build = store.builds().find_by_id(build_id).await?;
   if build.status == BuildStatus::Cancelled {
       tracing::info!("Build {} cancelled, aborting", build_id);
       return Ok(());
   }
   ```

1. **Fetch repository:**

   ```bash
   cd /var/lib/kennel/builds/<build_id>
   git clone --depth 1 <repo_url> repo
   cd repo
   git checkout <commit_sha>
   ```

1. **Parse kennel.toml:**

   - Read `kennel.toml` from repository root
   - Deserialize into Rust struct
   - Extract service definitions, static site configs
   - Validate service names (lowercase alphanumeric + hyphens)

1. **Build services:**
   Check for cancellation before each service build.
   For each service in kennel.toml:

   ```bash
   nix build .#packages.x86_64-linux.<service> \
     --out-link /var/lib/kennel/builds/<build_id>/<service> \
     --log-format bar-with-logs \
     2>&1 | tee /var/lib/kennel/logs/<build_id>/<service>.log
   ```

   On success:

   - Read store path: `readlink /var/lib/kennel/builds/<build_id>/<service>`
   - Insert build result:
     ```sql
     INSERT INTO build_results (build_id, service_name, status, store_path)
     VALUES (:build_id, :service_name, 'success', :store_path)
     ```

   On failure:

   - Error output already captured in log file
   - Insert build result:
     ```sql
     INSERT INTO build_results (build_id, service_name, status, error_log)
     VALUES (:build_id, :service_name, 'failed', :error_log)
     ```

1. **Cachix push (if enabled):**

   ```bash
   nix store sign --key-file /etc/nix/signing-key.pem <store_path>
   cachix push <cache_name> <store_path>
   ```

   If Cachix push fails, log warning but do not fail the build (deployment can still proceed with local store path).

1. **Build static sites:**
   Check for cancellation before each static site build.
   For each static site in kennel.toml:

   ```bash
   nix build .#packages.x86_64-linux.<site> \
     --out-link /var/lib/kennel/builds/<build_id>/<site>
   ```

   Store path handling same as services.

1. **Update build status:**

   - If cancelled: status already set to 'cancelled', do nothing
   - If all services and sites succeeded:
     ```sql
     UPDATE builds SET status = 'success', finished_at = NOW() WHERE id = :id
     ```
   - If any failed:
     ```sql
     UPDATE builds SET status = 'failed', finished_at = NOW() WHERE id = :id
     ```

1. **Trigger deployment:**
   If build succeeded, send deployment request to Deployer:

   ```rust
   deploy_tx.send(DeploymentRequest {
       build_id,
       project_name,
       git_ref,
   }).await
   ```

#### Unchanged Build Detection

Before building a service, query for recent successful builds of the same service:

```sql
SELECT br.store_path
FROM build_results br
JOIN builds b ON br.build_id = b.id
WHERE b.project_name = :project_name
  AND b.git_ref = :git_ref
  AND b.status = 'success'
  AND br.service_name = :service_name
  AND br.status = 'success'
ORDER BY b.created_at DESC
LIMIT 5
```

After building, if current store path matches any recent build, the service is unchanged. Still mark as success and proceed with deployment (existing deployment may need updates to secrets, environment, etc.).

#### Error Handling Strategies

- **Git clone failure:** mark build as failed, log error, do not retry
- **kennel.toml parse error:** mark build as failed, include parse error in logs
- **Nix build failure:** record per-service failure in build_results, continue with other services
- **Cachix push failure:** log warning but do not fail build (deployment proceeds with local store)
- **Database errors:** log and retry transaction once; if still failing, mark build as failed

#### Logging

Write structured logs to `/var/lib/kennel/logs/<build_id>/build.log`:

- Timestamp (from tracing subscriber)
- Build phase (clone, parse, build-service-X, cachix-push, etc.)
- Command invocations
- Stderr/stdout from Nix
- Errors and warnings

Use tracing spans to structure logs:

```rust
let span = tracing::info_span!("build", build_id = %build_id);
let _enter = span.enter();

tracing::info!("Starting build");
tracing::info!("Cloning repository");
// ...
```

#### Pseudocode

```rust
async fn process_build(
    build_id: i64,
    store: Arc<Store>,
    deploy_tx: mpsc::Sender<DeploymentRequest>,
) -> Result<()> {
    // Update status
    store.builds().update_status(build_id, BuildStatus::Building).await?;

    // Fetch build details
    let build = store.builds().find_by_id(build_id).await?.unwrap();
    let project = store.projects().find_by_name(&build.project_name).await?.unwrap();

    // Clone repo
    let work_dir = PathBuf::from(format!("/var/lib/kennel/builds/{}", build_id));
    git_clone(&project.repo_url, &build.commit_sha, &work_dir).await?;

    // Parse config
    let config_path = work_dir.join("repo/kennel.toml");
    let config = parse_kennel_toml(&config_path).await?;

    // Build services
    let mut all_success = true;
    for service in config.services {
        match nix_build(&work_dir, &service.name, build_id).await {
            Ok(store_path) => {
                store.build_results().create_success(
                    build_id,
                    &service.name,
                    &store_path,
                ).await?;
                
                // Cachix push (best effort)
                if let Some(ref cache) = config.cachix_cache {
                    let _ = cachix_push(cache, &store_path).await;
                }
            }
            Err(e) => {
                all_success = false;
                store.build_results().create_failure(
                    build_id,
                    &service.name,
                    &e.to_string(),
                ).await?;
            }
        }
    }

    // Build static sites
    for site in config.static_sites {
        match nix_build(&work_dir, &site.name, build_id).await {
            Ok(store_path) => {
                store.build_results().create_success(
                    build_id,
                    &site.name,
                    &store_path,
                ).await?;
            }
            Err(e) => {
                all_success = false;
                store.build_results().create_failure(
                    build_id,
                    &site.name,
                    &e.to_string(),
                ).await?;
            }
        }
    }

    // Update final status
    let final_status = if all_success {
        BuildStatus::Success
    } else {
        BuildStatus::Failed
    };
    store.builds().update_status(build_id, final_status).await?;

    // Trigger deployment if successful
    if all_success {
        deploy_tx.send(DeploymentRequest {
            build_id,
            project_name: build.project_name,
            git_ref: build.git_ref,
        }).await?;
    }

    Ok(())
}
```

### Deployer

#### Responsibilities

- Receive deployment requests from Builder
- Manage deployment lifecycle (pending -> building -> active)
- Deploy systemd services with port allocation and secrets
- Deploy static sites with symlink management
- Create preview databases for configured services
- Perform health checks before marking deployments active
- Tear down old deployments (stop services, release resources)
- Run periodic cleanup job for expired deployments

#### Deployment State Machine

Deployments transition through states defined in RFC 0003:

```
pending -> building -> active -> tearing_down -> torn_down
                   \-> failed
```

States:

- `pending` - deployment record created, not yet started
- `building` - build in progress (Builder sets this)
- `active` - deployed and healthy
- `failed` - deployment failed, resources released
- `tearing_down` - marked for deletion, cleanup in progress
- `torn_down` - cleanup complete, record can be deleted

#### Service Deployment Process

For each service in a successful build:

1. **Check for existing deployment:**

   ```sql
   SELECT id, port FROM deployments
   WHERE project_name = :project_name
     AND git_ref = :git_ref
     AND service_name = :service_name
     AND status IN ('active', 'pending', 'building')
   ```

1. **Create new deployment if needed:**
   If no active/pending/building deployment exists for this branch:

   ```sql
   INSERT INTO deployments (
       project_name, git_ref, service_name, build_id,
       status, deployed_at
   ) VALUES (
       :project_name, :git_ref, :service_name, :build_id,
       'pending', NOW()
   ) RETURNING id
   ```

1. **Allocate port:**
   Use kennel-store port allocation:

   ```rust
   let port = store.port_allocations()
       .allocate_port(deployment_id, &project.name, &git_ref, &service.name)
       .await?;
   ```

   Port range: 18000-19999 (RFC 0003)

1. **Create preview database if configured:**
   If service has `preview_database = true` in kennel.toml:

   ```rust
   let db_num = store.preview_databases()
       .allocate_database(deployment_id, &project.name, &git_ref)
       .await?;
   ```

   Valkey database numbers: 0-15 (RFC 0003)

1. **Create system user:**

   ```bash
   username="kennel-<project_name>-<branch>-<service_name>"
   if ! id "$username" 2>/dev/null; then
       useradd --system --no-create-home --shell /bin/false "$username"
   fi
   ```

   Users are created per-service (not per-project) for security isolation. Each service runs under its own user, preventing cross-service file access.

1. **Generate secrets file:**
   Write `/run/kennel/secrets/<project_name>-<git_ref>.env`:

   ```bash
   PORT=<allocated_port>
   VALKEY_URL=redis://127.0.0.1:6379/<db_num>
   DATABASE_URL=postgresql://127.0.0.1:5432/<project_name>_<git_ref>
   ```

   Set ownership to `kennel-<project_name>:kennel-<project_name>` with mode 0400.

1. **Generate systemd unit:**
   Write `/etc/systemd/system/kennel-<project_name>-<git_ref>-<service_name>.service`:

   ```ini
   [Unit]
   Description=Kennel deployment: <project_name>/<git_ref>/<service_name>
   After=network.target

   [Service]
   Type=simple
   User=kennel-<project_name>
   WorkingDirectory=/var/lib/kennel/deployments/<deployment_id>
   ExecStart=<store_path>/bin/<service_name>
   EnvironmentFile=/run/kennel/secrets/<project_name>-<git_ref>.env
   Restart=on-failure
   RestartSec=5s
   StandardOutput=journal
   StandardError=journal

   [Install]
   WantedBy=multi-user.target
   ```

1. **Start service:**

   ```bash
   systemctl daemon-reload
   systemctl enable kennel-<project_name>-<git_ref>-<service_name>
   systemctl start kennel-<project_name>-<git_ref>-<service_name>
   ```

1. **Health check:**
   Poll `http://localhost:<port>/health` for up to 30 seconds with exponential backoff (1s, 2s, 4s, 8s, 15s):

   ```rust
   let mut backoff = 1;
   let start = Instant::now();

   loop {
       match reqwest::get(format!("http://localhost:{}/health", port)).await {
           Ok(resp) if resp.status().is_success() => break,
           _ => {}
       }
       
       if start.elapsed() > Duration::from_secs(30) {
           return Err(DeployerError::HealthCheckTimeout);
       }
       
       tokio::time::sleep(Duration::from_secs(backoff)).await;
       backoff = (backoff * 2).min(15);
   }
   ```

   If health check times out, proceed to error handling.

1. **Update deployment status:**

   ```sql
   UPDATE deployments
   SET status = 'active',
       port = :port,
       store_path = :store_path,
       health_check_url = :health_url,
       last_health_check = NOW()
   WHERE id = :deployment_id
   ```

1. **Update router (blue-green cutover):**
   Notify router to start routing traffic to new deployment:

   ```rust
   router_tx.send(RouterUpdate::DeploymentActive {
       project_name: project.name.clone(),
       git_ref: build.git_ref.clone(),
       service_name: result.service_name.clone(),
       port: new_port,
   })?;
   ```

1. **Tear down previous deployment (blue-green completion):**
   If replacing an existing active deployment (e.g., new build for same branch):

   - Look up old deployment for same project/branch/service
   - Wait 30 seconds for existing connections to drain
   - Mark old deployment as `tearing_down`
   - Trigger teardown process (releases old port, stops old service)

#### Static Site Deployment Process

For each static site in a successful build:

1. **Create deployment record:**

   ```sql
   INSERT INTO deployments (
       project_name, git_ref, service_name, build_id,
       status, deployed_at
   ) VALUES (
       :project_name, :git_ref, :site_name, :build_id,
       'pending', NOW()
   ) RETURNING id
   ```

1. **Create symlink:**
   Target: `/var/lib/kennel/sites/<project_name>/<branch_sanitized>/<site_name>`
   Source: `<store_path>`

   Note: `<branch_sanitized>` is the git_ref with filesystem-unsafe characters replaced (e.g., `/` becomes `-`, converted to lowercase).

   ```bash
   mkdir -p /var/lib/kennel/sites/<project_name>/<branch_sanitized>
   ln -sf <store_path> /var/lib/kennel/sites/<project_name>/<branch_sanitized>/<site_name>
   ```

1. **Update deployment:**

   ```sql
   UPDATE deployments
   SET status = 'active',
       store_path = :store_path,
       static_path = :symlink_path
   WHERE id = :deployment_id
   ```

1. **Remove old deployment:**
   If replacing existing static deployment for this branch:

   - Look up old deployment
   - Remove old symlink
   - Mark old deployment as `torn_down`

#### Teardown Process

When deployment status is `tearing_down`:

1. **Stop systemd service (if service deployment):**

   ```bash
   systemctl stop kennel-<project_name>-<git_ref>-<service_name>
   systemctl disable kennel-<project_name>-<git_ref>-<service_name>
   rm /etc/systemd/system/kennel-<project_name>-<git_ref>-<service_name>.service
   systemctl daemon-reload
   ```

   If service does not exist, log warning and continue.

1. **Release port (if service deployment):**

   ```rust
   store.port_allocations().release_port(deployment_id).await?;
   ```

1. **Release preview database (if allocated):**

   ```rust
   store.preview_databases().release_database(deployment_id).await?;
   ```

1. **Remove secrets file:**

   ```bash
   rm -f /run/kennel/secrets/<project_name>-<git_ref>.env
   ```

1. **Remove static symlink (if static deployment):**

   ```bash
   rm -f /var/lib/kennel/sites/<project_name>/<branch_sanitized>/<site_name>
   ```

1. **Update deployment status:**

   ```sql
   UPDATE deployments SET status = 'torn_down' WHERE id = :deployment_id
   ```

1. **Delete deployment record:**
   After successful teardown:

   ```sql
   DELETE FROM deployments WHERE id = :deployment_id
   ```

#### Auto-Expiry Cleanup Job

Run periodic task every 10 minutes using tokio interval:

```rust
async fn run_cleanup_job(config: DeployerConfig, teardown_tx: mpsc::Sender<i32>) {
    let mut interval = tokio::time::interval(Duration::from_secs(600));
    
    loop {
        interval.tick().await;
        
        match config.store.find_expired_deployments(7).await {
            Ok(expired) if !expired.is_empty() => {
                let ids: Vec<i32> = expired.iter().map(|d| d.id).collect();
                let _ = config.store.deployments().mark_ids_tearing_down(&ids).await;
                
                for id in &ids {
                    let _ = teardown_tx.send(*id).await;
                }
            }
            _ => {}
        }
    }
}
```

A separate build log cleanup job runs daily, deleting build records and
their log directories older than 30 days:

```rust
async fn run_log_cleanup_job(config: DeployerConfig) {
    let mut interval = tokio::time::interval(Duration::from_secs(86400));
    
    loop {
        interval.tick().await;
        
        if let Ok(old_builds) = config.store.find_old_builds(30).await {
            for build in &old_builds {
                let _ = tokio::fs::remove_dir_all(format!("/var/lib/kennel/logs/{}", build.id)).await;
                let _ = config.store.builds().delete(build.id).await;
            }
        }
    }
}
```

Auto-expiry logic (from RFC 0003):

- `auto_expiry_days` from projects table
- Deployments where `last_activity < NOW() - INTERVAL 'N days'`
- Main branch deployments never expire

#### Error Handling Strategies

- **Port allocation failure:** mark deployment as failed, log error, do not proceed with this service
- **Preview database allocation failure:** mark deployment as failed (service cannot start without DB)
- **systemd start failure:** capture journalctl logs, mark deployment as failed, release allocated resources
- **Health check timeout:** stop service, mark deployment as failed, release resources
- **Teardown failures:** log error but continue best-effort cleanup (orphaned resources cleaned up by reconciliation)

#### Pseudocode

```rust
async fn deploy_service(
    deployment_req: DeploymentRequest,
    store: Arc<Store>,
    router_tx: broadcast::Sender<RouterUpdate>,
) -> Result<()> {
    let build = store.builds().find_by_id(deployment_req.build_id).await?.unwrap();
    let project = store.projects().find_by_name(&build.project_name).await?.unwrap();
    
    // Get successful build results
    let results = store.build_results()
        .find_successful(deployment_req.build_id)
        .await?;

    for result in results {
        let service = store.services()
            .find_by_name(&project.name, &result.service_name)
            .await?
            .unwrap();

        // Create or find deployment
        let deployment = match store.deployments()
            .find_active(&project.name, &build.git_ref, &result.service_name)
            .await?
        {
            Some(d) => {
                // Update existing deployment with new build
                store.deployments().update_build(d.id, build.id).await?;
                d
            }
            None => {
                store.deployments().create(
                    &project.name,
                    &build.git_ref,
                    &result.service_name,
                    build.id,
                ).await?
            }
        };

        if service.service_type == "static" {
            deploy_static_site(&project, &build, &result, deployment.id, &store).await?;
        } else {
            // Allocate resources
            let port = store.port_allocations()
                .allocate_port(deployment.id, &project.name, &build.git_ref, &result.service_name)
                .await?;

            let db_num = if service.preview_database {
                Some(store.preview_databases()
                    .allocate_database(deployment.id, &project.name, &build.git_ref)
                    .await?)
            } else {
                None
            };

            // Generate and start systemd unit
            create_system_user(&project.name).await?;
            generate_secrets_file(&project.name, &build.git_ref, port, db_num).await?;
            generate_systemd_unit(&project, &build.git_ref, &result, port).await?;
            systemctl_start(&project.name, &build.git_ref, &result.service_name).await?;

            // Health check
            health_check(port).await?;

            // Update deployment
            store.deployments().mark_active(
                deployment.id,
                port,
                &result.store_path,
            ).await?;
        }

        // Notify router
        router_tx.send(RouterUpdate::DeploymentActive {
            project_name: project.name.clone(),
            git_ref: build.git_ref.clone(),
            service_name: result.service_name.clone(),
        })?;
    }

    Ok(())
}
```

### Router

#### Responsibilities

- Accept all HTTP traffic on port 80 (and 443 with TLS)
- Parse incoming request Host header
- Look up deployment by custom domain or auto-generated subdomain
- Proxy requests to backend service (localhost:port) or serve static files
- Handle SPA fallback for static sites (return index.html for 404s)
- Return appropriate errors (404, 502, 503)
- Reload routing table when deployments change

#### HTTP Server Design

Axum server listening on 0.0.0.0:80:

```rust
#[derive(Clone)]
struct AppState {
    router: Arc<RoutingTable>,
}

async fn route_request(
    State(state): State<AppState>,
    Host(host): Host,
    request: Request<Body>,
) -> Response<Body> {
    match state.router.lookup(&host).await {
        Some(Route::Service { port }) => proxy_to_service(request, port).await,
        Some(Route::Static { path, spa }) => serve_static(request, path, spa).await,
        None => {
            Response::builder()
                .status(404)
                .body(Body::from("Deployment not found"))
                .unwrap()
        }
    }
}

async fn run_router(router: Arc<RoutingTable>, bind_addr: SocketAddr) {
    let app = Router::new()
        .fallback(route_request)
        .with_state(AppState { router });

    axum::Server::bind(&bind_addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
```

#### Host Header Parsing

Extract `Host` header from request. If missing, return 400 Bad Request.

Two routing patterns:

**1. Custom Domain:**

Example: `example.com`

Lookup:

```sql
SELECT d.id, d.port, d.static_path, s.service_type, s.spa
FROM services s
JOIN deployments d ON s.project_name = d.project_name AND s.name = d.service_name
WHERE s.custom_domain = :host
  AND d.status = 'active'
LIMIT 1
```

**2. Auto-Generated Subdomain:**

Example: `api-main.myproject.scottylabs.org`

Parse as:

- `service_name` = "api"
- `git_ref` = "main"
- `project_name` = "myproject"
- `base_domain` = "scottylabs.org"

Pattern: `<service>-<branch>.<project>.<base_domain>`

Lookup:

```sql
SELECT d.id, d.port, d.static_path, s.service_type, s.spa
FROM deployments d
JOIN services s ON d.project_name = s.project_name AND d.service_name = s.name
WHERE d.project_name = :project_name
  AND d.git_ref = :git_ref
  AND d.service_name = :service_name
  AND d.status = 'active'
LIMIT 1
```

#### Backend Proxy (Services)

For service deployments (service_type = 'service'), proxy request to `http://127.0.0.1:<port>`:

```rust
async fn proxy_to_service(request: Request<Body>, port: u16) -> Response<Body> {
    let path_and_query = request.uri().path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    
    let uri = format!("http://127.0.0.1:{}{}", port, path_and_query);
    
    let client = reqwest::Client::new();
    let mut req_builder = client.request(request.method().clone(), &uri);
    
    // Forward headers (except Host)
    for (name, value) in request.headers() {
        if name != "host" {
            req_builder = req_builder.header(name, value);
        }
    }
    
    // Add X-Forwarded-* headers
    if let Some(original_host) = request.headers().get("host") {
        req_builder = req_builder.header("X-Forwarded-Host", original_host);
    }
    req_builder = req_builder.header("X-Forwarded-Proto", "http"); // or "https"
    
    match req_builder.body(request.into_body()).send().await {
        Ok(response) => {
            let mut builder = Response::builder().status(response.status());
            for (name, value) in response.headers() {
                builder = builder.header(name, value);
            }
            builder.body(response.bytes().await.unwrap().into()).unwrap()
        }
        Err(e) => {
            tracing::error!("Proxy error: {}", e);
            Response::builder()
                .status(502)
                .body(Body::from("Bad Gateway"))
                .unwrap()
        }
    }
}
```

Forward headers:

- All headers except `Host`
- Add `X-Forwarded-For` with client IP
- Add `X-Forwarded-Proto` (http or https)
- Add `X-Forwarded-Host` with original host header

#### Static File Serving

For static deployments (service_type = 'static'), serve files from `static_path`:

```rust
async fn serve_static(
    request: Request<Body>,
    base_path: PathBuf,
    spa: bool,
) -> Response<Body> {
    let path = request.uri().path().trim_start_matches('/');
    let file_path = base_path.join(path);

    // Security: prevent directory traversal
    if !file_path.starts_with(&base_path) {
        return Response::builder()
            .status(403)
            .body(Body::from("Forbidden"))
            .unwrap();
    }

    // Try to serve requested file
    if file_path.exists() && file_path.is_file() {
        return serve_file(file_path).await;
    }

    // Try index.html if directory
    if file_path.is_dir() {
        let index = file_path.join("index.html");
        if index.exists() {
            return serve_file(index).await;
        }
    }

    // SPA fallback: return root index.html for 404s
    if spa {
        let index_path = base_path.join("index.html");
        if index_path.exists() {
            return serve_file(index_path).await;
        }
    }

    Response::builder()
        .status(404)
        .body(Body::from("Not Found"))
        .unwrap()
}

async fn serve_file(path: PathBuf) -> Response<Body> {
    match tokio::fs::read(&path).await {
        Ok(contents) => {
            let mime = mime_guess::from_path(&path)
                .first_or_octet_stream();
            
            Response::builder()
                .status(200)
                .header("Content-Type", mime.as_ref())
                .body(Body::from(contents))
                .unwrap()
        }
        Err(e) => {
            tracing::error!("Failed to read file {:?}: {}", path, e);
            Response::builder()
                .status(500)
                .body(Body::from("Internal Server Error"))
                .unwrap()
        }
    }
}
```

Content-Type detection:

- Use mime_guess crate based on file extension
- Default to `application/octet-stream`

#### Routing Table Management

Maintain in-memory routing table for fast lookups:

```rust
struct RoutingTable {
    cache: Arc<RwLock<HashMap<String, Route>>>,
    health_status: Arc<RwLock<HashMap<u16, HealthStatus>>>,
    store: Arc<Store>,
}

#[derive(Clone)]
enum Route {
    Service { port: u16 },
    Static { path: PathBuf, spa: bool },
}

#[derive(Clone)]
struct HealthStatus {
    healthy: bool,
    last_check: Instant,
    consecutive_failures: u32,
}

impl RoutingTable {
    async fn lookup(&self, host: &str) -> Option<Route> {
        self.cache.read().await.get(host).cloned()
    }
    
    async fn is_healthy(&self, port: u16) -> bool {
        self.health_status.read().await
            .get(&port)
            .map(|status| status.healthy)
            .unwrap_or(false)
    }
    
    async fn reload_from_database(&self) -> Result<()> {
        let deployments = self.store.deployments().find_all_active().await?;
        
        let mut new_cache = HashMap::new();
        
        for deployment in deployments {
            let service = self.store.services()
                .find_by_name(&deployment.project_name, &deployment.service_name)
                .await?
                .unwrap();
            
            let route = if service.service_type == "static" {
                Route::Static {
                    path: PathBuf::from(&deployment.static_path.unwrap()),
                    spa: service.spa,
                }
            } else {
                Route::Service {
                    port: deployment.port.unwrap(),
                }
            };
            
            // Add custom domain route
            if let Some(domain) = service.custom_domain {
                new_cache.insert(domain, route.clone());
            }
            
            // Add auto-generated subdomain route
            let auto_domain = format!(
                "{}-{}.{}.{}",
                deployment.service_name,
                deployment.git_ref,
                deployment.project_name,
                "scottylabs.org" // from config
            );
            new_cache.insert(auto_domain, route);
        }
        
        *self.cache.write().await = new_cache;
        Ok(())
    }
    
    async fn apply_update(&self, update: RouterUpdate) {
        match update {
            RouterUpdate::DeploymentActive { project_name, git_ref, service_name, port } => {
                let auto_domain = format!(
                    "{}-{}.{}.scottylabs.org",
                    service_name, git_ref, project_name
                );
                self.cache.write().await.insert(
                    auto_domain,
                    Route::Service { port },
                );
            }
            RouterUpdate::DeploymentRemoved { project_name, git_ref, service_name } => {
                let auto_domain = format!(
                    "{}-{}.{}.scottylabs.org",
                    service_name, git_ref, project_name
                );
                self.cache.write().await.remove(&auto_domain);
            }
        }
    }
}
```

Reload strategies:

**Event-Driven Updates:**

- Deployer sends `RouterUpdate` messages via broadcast channel when deployments change
- Router subscribes and updates cache incrementally
- Low latency (immediate routing table updates)

**Periodic Full Reload:**

- Poll database every 60 seconds as safety net
- Ensures eventual consistency if events are missed

The router uses event-driven updates for normal operation with periodic full reload for crash recovery and consistency guarantees.

```rust
async fn run_routing_table_updater(
    table: Arc<RoutingTable>,
    mut updates: broadcast::Receiver<RouterUpdate>,
) {
    // Initial load from database
    if let Err(e) = table.reload_from_database().await {
        tracing::error!("Failed to load routing table: {}", e);
    }

    let mut reload_interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            Ok(update) = updates.recv() => {
                table.apply_update(update).await;
            }
            _ = reload_interval.tick() => {
                tracing::debug!("Periodic routing table reload");
                if let Err(e) = table.reload_from_database().await {
                    tracing::error!("Routing table reload failed: {}", e);
                }
            }
        }
    }
}
```

**Continuous Health Monitoring:**

Run periodic health checks on all active service deployments:

```rust
async fn run_health_monitor(table: Arc<RoutingTable>, store: Arc<Store>) {
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    
    loop {
        interval.tick().await;
        
        // Get all active service deployments
        let deployments = match store.deployments().find_all_active_services().await {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("Failed to query deployments for health check: {}", e);
                continue;
            }
        };
        
        for deployment in deployments {
            let port = deployment.port.unwrap();
            let health_url = format!("http://localhost:{}/health", port);
            
            let is_healthy = match tokio::time::timeout(
                Duration::from_secs(5),
                reqwest::get(&health_url)
            ).await {
                Ok(Ok(resp)) if resp.status().is_success() => true,
                _ => false,
            };
            
            let mut health_status = table.health_status.write().await;
            
            if let Some(status) = health_status.get_mut(&port) {
                status.last_check = Instant::now();
                if is_healthy {
                    status.healthy = true;
                    status.consecutive_failures = 0;
                } else {
                    status.consecutive_failures += 1;
                    // Mark unhealthy after 3 consecutive failures
                    if status.consecutive_failures >= 3 {
                        status.healthy = false;
                        tracing::warn!("Deployment {} (port {}) marked unhealthy after {} failures",
                            deployment.id, port, status.consecutive_failures);
                    }
                }
            } else {
                // Initialize health status for new deployment
                health_status.insert(port, HealthStatus {
                    healthy: is_healthy,
                    last_check: Instant::now(),
                    consecutive_failures: if is_healthy { 0 } else { 1 },
                });
            }
        }
    }
}
```

#### Error Responses

- **400 Bad Request:** missing Host header
- **403 Forbidden:** directory traversal attempt in static file serving
- **404 Not Found:** no deployment matches host, or static file not found
- **502 Bad Gateway:** backend service unreachable or returned error during proxy
- **503 Service Unavailable:** deployment exists in routing table but marked unhealthy by health monitor

#### TLS Termination

Kennel router includes built-in ACME (Let's Encrypt) integration for automatic TLS certificate management using rustls-acme.

Certificate acquisition:

- On startup, load TLS config from database (domains requiring certificates)
- For each custom domain, request certificate via ACME HTTP-01 challenge
- Store certificates in `/var/lib/kennel/tls/<domain>.{cert,key}`
- Auto-renew certificates 30 days before expiry

ACME HTTP-01 challenge handling:

- Router serves `.well-known/acme-challenge/<token>` from ACME validation endpoint
- After validation, ACME provider issues certificate
- Router installs certificate and begins serving HTTPS

Dual-stack listener:

- Port 80 (HTTP): ACME challenges and redirect to HTTPS
- Port 443 (HTTPS): TLS termination with rustls, then route requests

Auto-generated subdomains (`*.scottylabs.org`) use wildcard certificate configured at deployment time.

#### Pseudocode

```rust
struct Router {
    table: Arc<RoutingTable>,
}

async fn handle_request(
    State(router): State<Arc<Router>>,
    Host(host): Host,
    request: Request<Body>,
) -> Response<Body> {
    // Lookup route
    let route = match router.table.lookup(&host).await {
        Some(r) => r,
        None => {
            tracing::warn!("No route found for host: {}", host);
            return Response::builder()
                .status(404)
                .body(Body::from("Deployment not found"))
                .unwrap();
        }
    };

    // Route to backend
    match route {
        Route::Service { port } => {
            // Check health status before proxying
            if !router.table.is_healthy(port).await {
                tracing::warn!("Service on port {} is unhealthy", port);
                return Response::builder()
                    .status(503)
                    .body(Body::from("Service Unavailable"))
                    .unwrap();
            }
            
            tracing::debug!("Proxying {} to port {}", host, port);
            proxy_request(request, port).await
        }
        Route::Static { path, spa } => {
            tracing::debug!("Serving static files from {:?}", path);
            serve_static_file(request, path, spa).await
        }
    }
}
```

### Full Pipeline Data Flow

End-to-end sequence for a Git push event:

```
1. Git push to branch "feature-x"
   |
   v
2. Forgejo/GitHub sends webhook POST
   POST /webhook/myproject
   X-Forgejo-Event: push
   {"ref": "refs/heads/feature-x", "after": "abc123", ...}
   |
   v
3. Webhook Receiver:
   - Verifies HMAC signature
   - Parses push event (git_ref = "feature-x", commit_sha = "abc123")
   - Checks for duplicate: SELECT ... WHERE project_name = 'myproject' AND git_ref = 'feature-x' AND commit_sha = 'abc123'
   - Inserts build: INSERT INTO builds ... RETURNING id (e.g., build_id = 42)
   - Sends to Builder: build_tx.send(42)
   - Returns 200 OK to Git platform
   |
   v
4. Builder worker receives build_id = 42:
   - Updates: UPDATE builds SET status = 'building', started_at = NOW() WHERE id = 42
   - Clones: git clone <repo_url> /var/lib/kennel/builds/42/repo && git checkout abc123
   - Parses: kennel.toml -> services = [{name: "api", ...}]
   - Builds: nix build .#packages.x86_64-linux.api --out-link /var/lib/kennel/builds/42/api
   - Reads store path: /nix/store/xyz123-api
   - Inserts: INSERT INTO build_results (build_id=42, service_name='api', status='success', store_path='/nix/store/xyz123-api')
   - Updates: UPDATE builds SET status = 'success', finished_at = NOW() WHERE id = 42
   - Sends to Deployer: deploy_tx.send(DeploymentRequest { build_id: 42, project_name: "myproject", git_ref: "feature-x" })
   |
   v
5. Deployer receives deployment request:
   - Checks for existing: SELECT ... WHERE project_name = 'myproject' AND git_ref = 'feature-x' AND service_name = 'api' AND status = 'active'
   - No existing, creates: INSERT INTO deployments ... RETURNING id (e.g., deployment_id = 7)
   - Allocates port: INSERT INTO port_allocations ... RETURNING port (e.g., 18042)
   - Creates user: useradd kennel-myproject-feature-x-api (per-service user for isolation)
   - Writes secrets: /run/kennel/secrets/myproject-feature-x-api.env (PORT=18042, ...)
   - Writes unit: /etc/systemd/system/kennel-myproject-feature-x-api.service
   - Starts: systemctl start kennel-myproject-feature-x-api
   - Health checks: GET http://localhost:18042/health -> 200 OK
   - Updates: UPDATE deployments SET status = 'active', port = 18042, store_path = '/nix/store/xyz123-api' WHERE id = 7
   - Notifies Router: router_tx.send(RouterUpdate::DeploymentActive { project_name: "myproject", git_ref: "feature-x", service_name: "api", port: 18042 })
   |
   v
6. Router receives update:
   - Adds to routing table: cache.insert("api-feature-x.myproject.scottylabs.org", Route::Service { port: 18042 })
   |
   v
7. User makes request:
   GET http://api-feature-x.myproject.scottylabs.org/users
   |
   v
8. Router:
   - Parses host: "api-feature-x.myproject.scottylabs.org"
   - Looks up: cache.get("api-feature-x.myproject.scottylabs.org") -> Route::Service { port: 18042 }
   - Proxies: GET http://127.0.0.1:18042/users
   - Returns response to user
```

Teardown sequence (branch deletion):

```
1. User deletes branch "feature-x"
   git push origin :feature-x
   |
   v
2. Forgejo sends webhook with "after": "0000000000000000000000000000000000000000"
   |
   v
3. Webhook Receiver:
   - Detects branch deletion
   - Updates: UPDATE deployments SET status = 'tearing_down' WHERE project_name = 'myproject' AND git_ref = 'feature-x'
   - Sends: teardown_tx.send(7)
   - Returns 202 Accepted
   |
   v
4. Deployer receives teardown request:
   - Stops: systemctl stop kennel-myproject-feature-x-api
   - Disables: systemctl disable kennel-myproject-feature-x-api
   - Removes unit: rm /etc/systemd/system/kennel-myproject-feature-x-api.service
   - Releases port: DELETE FROM port_allocations WHERE deployment_id = 7
   - Removes secrets: rm /run/kennel/secrets/myproject-feature-x.env
   - Updates: UPDATE deployments SET status = 'torn_down' WHERE id = 7
   - Deletes: DELETE FROM deployments WHERE id = 7
   - Notifies Router: router_tx.send(RouterUpdate::DeploymentRemoved { project_name: "myproject", git_ref: "feature-x", service_name: "api" })
   |
   v
5. Router:
   - Removes from cache: cache.remove("api-feature-x.myproject.scottylabs.org")
   |
   v
6. Subsequent requests to api-feature-x.myproject.scottylabs.org return 404
```

### Startup Reconciliation

On Kennel startup, reconciliation ensures the database matches reality:

1. Sync projects from NixOS configuration (add new, remove stale)
1. Clean up orphaned systemd units not backed by active deployments
1. Release stale port allocations for non-existent deployments
1. Remove orphaned static site symlinks and empty directories
1. Mark stuck builds (in `building` state) as failed

This recovers gracefully from crashes or unclean shutdowns without
requiring manual intervention.

Component-specific startup:

1. **Router:** Load all active deployments into routing table
1. **Deployer:** Spawn auto-expiry cleanup task
1. **Builder:** Start worker pool (no state to recover)
1. **Webhook Receiver:** Start HTTP server (no state to recover)

### Configuration

Environment variables:

```bash
# Database
DATABASE_URL=postgresql://127.0.0.1:5432/kennel

# Webhook receiver
WEBHOOK_BIND_ADDR=0.0.0.0:8080

# Router
ROUTER_BIND_ADDR=0.0.0.0:80
BASE_DOMAIN=scottylabs.org

# Builder
MAX_CONCURRENT_BUILDS=2

# Cachix (optional)
CACHIX_CACHE_NAME=kennel
CACHIX_AUTH_TOKEN=...

# Cleanup
AUTO_EXPIRY_CHECK_INTERVAL_SECS=600
```

CLI arguments:

```
kennel run [OPTIONS]

Options:
  --webhook-port <PORT>        Port for webhook receiver [default: 8080]
  --router-port <PORT>         Port for router [default: 80]
  --max-builds <N>             Max concurrent builds [default: 2]
  --cleanup-interval <SECS>    Auto-expiry check interval [default: 600]
  --base-domain <DOMAIN>       Base domain for auto-generated subdomains [default: scottylabs.org]
```

Graceful shutdown:

Use kennel-config shutdown signal handling:

```rust
async fn run(config: Config) -> Result<()> {
    let shutdown = kennel_config::shutdown_signal();
    
    // Start all components
    let webhook_handle = tokio::spawn(run_webhook_receiver(...));
    let builder_handle = tokio::spawn(run_builder_pool(...));
    let deployer_handle = tokio::spawn(run_deployer(...));
    let router_handle = tokio::spawn(run_router(...));
    
    // Wait for shutdown signal
    shutdown.await;
    
    tracing::info!("Shutdown signal received, draining...");
    
    // Stop accepting new work
    drop(build_tx);
    drop(deploy_tx);
    
    // Wait for in-progress work (with timeout)
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(300)) => {
            tracing::warn!("Shutdown timeout, forcing exit");
        }
        _ = async {
            let _ = webhook_handle.await;
            let _ = builder_handle.await;
            let _ = deployer_handle.await;
            let _ = router_handle.await;
        } => {
            tracing::info!("All components shut down cleanly");
        }
    }
    
    Ok(())
}
```

On SIGTERM or SIGINT:

1. Webhook Receiver stops accepting new requests (server shutdown)
1. Builder finishes in-progress builds or aborts after 5 min timeout
1. Deployer finishes in-progress deployments
1. Router drains existing connections
1. Close database pool

## Alternatives Considered

### Builder: Sequential vs Parallel Service Builds

**Chosen:** Parallel builds (all services in a project build concurrently)

**Alternative:** Sequential builds (build services one at a time)

**Reasoning:** Parallel builds reduce total build time. Most projects have 1-2 services, so complexity is low. Nix already handles parallelism internally.

### Deployer: Database Polling vs Event-Driven

**Chosen:** Event-driven via channels

**Alternative:** Deployer polls database for new successful builds every N seconds

**Reasoning:** Event-driven has lower latency (immediate deployment after build) and lower database load. Polling adds unnecessary delay and query overhead.

### Router: In-Memory Cache vs Direct Database Lookup

**Chosen:** In-memory routing table with event-driven updates and periodic reload

**Alternative:** Query database on every HTTP request

**Reasoning:** Database lookup per request adds latency (5-10ms) and database load. In-memory cache provides sub-millisecond lookups. Periodic reload ensures eventual consistency.

### Communication: Channels vs Database as Queue

**Chosen:** Channels (mpsc, broadcast) for component communication

**Alternative:** Database as work queue (e.g., builds table as queue, polling for new rows)

**Reasoning:** Channels are simpler and faster for same-process communication. Database queue adds latency and complexity. Database still serves as source of truth for state, but channels handle real-time events.

### Deployment Strategy: Blue-Green vs In-Place

**Chosen:** Blue-green deployment (start new, health check, then stop old)

**Alternative:** In-place deployment (stop old, start new)

**Reasoning:** Blue-green provides zero-downtime deployments, which is essential for production services. While it requires double the resources temporarily (2x ports, 2x processes), the port pool (18000-19999, 2000 ports total) is large enough to support this. The brief resource overlap during deployment is acceptable for production reliability. In-place deployment would cause downtime during every deployment, which is unacceptable for production services.

## Open Questions

1. **Build cancellation UI:** How should users cancel in-progress builds? Dashboard button? CLI command? Both?

1. **Build artifact retention:** How long to keep `/var/lib/kennel/builds/<build_id>` directories? Keep for N days? Only keep last N per project?

1. **Deployment rollback:** How to rollback to previous deployment? Re-trigger old build? Keep previous deployment warm in database?

1. **Multi-service routing:** When project has multiple services, how to route traffic? Separate subdomains (`api-main.myproject`, `web-main.myproject`)? Path-based (`main.myproject/api`, `main.myproject/web`)?

1. **Resource limits:** Should systemd units include CPU/memory limits? If so, how are limits configured (kennel.toml, database, global defaults)?

1. **Logging aggregation:** Where do service logs go? Just journalctl? Centralized log aggregation?

1. **Connection draining duration:** Is 30 seconds sufficient for blue-green deployment connection draining? Should this be configurable per-service?

## Implementation Phases

### 1. Webhook Receiver

Implement HTTP endpoint with signature verification and build record creation.

**Files to create:**

- `crates/kennel-webhook/Cargo.toml`
- `crates/kennel-webhook/src/lib.rs` - public API and Axum router
- `crates/kennel-webhook/src/verify.rs` - HMAC signature verification
- `crates/kennel-webhook/src/parse.rs` - JSON event parsing
- `crates/kennel-webhook/src/error.rs` - error types

**Files to modify:**

- `Cargo.toml` - add kennel-webhook to workspace

**Tests:**

- Unit tests for signature verification (valid/invalid HMAC)
- Unit tests for event parsing (Forgejo/GitHub JSON)
- Integration test: mock webhook POST, verify build record created

**Done when:**

- Webhook endpoint accepts POST requests
- Signature verification works for Forgejo and GitHub
- Build records created in database with status = 'queued'
- Build IDs sent to channel

### 2. Builder Foundation

Implement worker pool, Nix invocation, and build results recording.

**Files to create:**

- `crates/kennel-builder/Cargo.toml`
- `crates/kennel-builder/src/lib.rs` - public API and worker pool
- `crates/kennel-builder/src/worker.rs` - build processing logic
- `crates/kennel-builder/src/nix.rs` - Nix command invocation
- `crates/kennel-builder/src/git.rs` - Git clone operations
- `crates/kennel-builder/src/error.rs` - error types

**Files to modify:**

- `Cargo.toml` - add kennel-builder to workspace

**Tests:**

- Unit test: Nix build invocation (mock with fake flake)
- Integration test: full build from git clone to build_results record

**Done when:**

- Worker pool receives build IDs and processes them
- Git clone works for public repositories
- Single service Nix builds execute and record store paths
- Build status updates to 'building' then 'success' or 'failed'

### 3. Builder Completion

Add kennel.toml parsing, multi-service builds, Cachix, and static sites.

**Files to create:**

- `crates/kennel-builder/src/config.rs` - kennel.toml parsing
- `crates/kennel-builder/src/cachix.rs` - Cachix push integration

**Files to modify:**

- `crates/kennel-builder/src/worker.rs` - multi-service loop, static site builds

**Tests:**

- Unit test: kennel.toml parsing (valid/invalid TOML)
- Integration test: multi-service build
- Integration test: static site build

**Done when:**

- kennel.toml parsed and validated
- All services and static sites in config get built
- Cachix push works (conditional on CACHIX_AUTH_TOKEN)
- Deployment requests sent to Deployer

### 4. Deployer for Services

Implement systemd unit generation, port allocation, and service lifecycle.

**Files to create:**

- `crates/kennel-deployer/Cargo.toml`
- `crates/kennel-deployer/src/lib.rs` - public API
- `crates/kennel-deployer/src/systemd.rs` - unit file generation and systemctl
- `crates/kennel-deployer/src/health.rs` - health check logic
- `crates/kennel-deployer/src/secrets.rs` - secrets file generation
- `crates/kennel-deployer/src/user.rs` - system user creation
- `crates/kennel-deployer/src/error.rs` - error types

**Files to modify:**

- `Cargo.toml` - add kennel-deployer to workspace

**Tests:**

- Unit test: systemd unit template rendering
- Unit test: health check retry logic
- Integration test: full service deployment (requires sudo for systemctl)

**Done when:**

- Systemd units generated and started
- Ports allocated from pool
- Health checks pass before marking active
- Deployments update to 'active' status
- Old deployments torn down

### 5. Deployer for Static Sites

Add symlink management and SPA handling.

**Files to create:**

- `crates/kennel-deployer/src/static_site.rs` - static deployment logic

**Files to modify:**

- `crates/kennel-deployer/src/lib.rs` - route static vs service deployments

**Tests:**

- Integration test: static site deployment creates symlink
- Integration test: SPA flag persists to database

**Done when:**

- Static sites deployed to `/var/lib/kennel/sites/...`
- Symlinks created and removed correctly
- SPA flag stored for router

### 6. Router Implementation

Implement HTTP server, routing table, proxy, and static file serving.

**Files to create:**

- `crates/kennel-router/Cargo.toml`
- `crates/kennel-router/src/lib.rs` - public API and Axum server
- `crates/kennel-router/src/table.rs` - routing table logic
- `crates/kennel-router/src/proxy.rs` - service proxy
- `crates/kennel-router/src/static_serve.rs` - static file serving
- `crates/kennel-router/src/error.rs` - error types

**Files to modify:**

- `Cargo.toml` - add kennel-router to workspace

**Tests:**

- Unit test: host header parsing
- Unit test: routing table lookup
- Integration test: proxy to mock backend service
- Integration test: serve static files with SPA fallback

**Done when:**

- Router accepts HTTP requests
- Host header parsed (custom domain + auto-generated subdomain)
- Requests proxied to backend services
- Static files served from filesystem
- SPA fallback returns index.html for 404s

### 7. Integration

Wire all components together in main binary.

**Files to modify:**

- `crates/kennel/src/main.rs` - spawn all component tasks
- `crates/kennel/Cargo.toml` - depend on webhook, builder, deployer, router

**Tests:**

- End-to-end test: webhook -> build -> deploy -> route

**Done when:**

- Single `kennel run` command starts all components
- Channels connect components
- Graceful shutdown works
- Configuration loaded from env vars and CLI args

### 8. Additional Features

Add preview databases, auto-expiry, reconciliation, and polish.

**Files to modify:**

- `crates/kennel-deployer/src/lib.rs` - preview database allocation
- `crates/kennel-deployer/src/cleanup.rs` - auto-expiry task
- `crates/kennel/src/main.rs` - startup reconciliation

**Tests:**

- Integration test: preview database allocation and release
- Integration test: auto-expiry cleanup job

**Done when:**

- Preview databases allocated for configured services
- Auto-expiry cleanup runs periodically
- Startup reconciliation automatically cleans up orphaned resources
- Continuous health monitoring tracks service health
- All core pipeline features complete
