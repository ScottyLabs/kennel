# RFC 0011: Infrastructure Provisioning

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-03-10
- **Updated:** 2026-03-10

## Overview

Automatically provision per-deployment infrastructure resources (databases, key-value stores, object storage) using a trait-based provider architecture. Providers manage isolated resources within shared host-level services and inject connection information as environment variables, transparent to application code.

## Motivation

Deployed services need infrastructure beyond compute. A typical project requires a PostgreSQL database, a Valkey instance for caching, and S3-compatible object storage for uploads. In local development, devenv manages these as per-project instances (`services.postgres.enable = true`). In production, running a dedicated instance per branch deployment is wasteful -- shared instances with per-deployment isolation are more appropriate.

The current codebase handles this minimally via the `preview_database` flag in `kennel.toml`, which provisions a PostgreSQL database and a Valkey DB number. This approach has several limitations:

- Hardcoded to exactly two services (PostgreSQL, Valkey)
- Valkey is limited to 16 DB numbers
- No object storage support
- Discovery is manual (`preview_database = true` in `kennel.toml` instead of derived from the project's devenv config)
- No general model for adding new infrastructure types

With devenv integration, Kennel discovers infrastructure requirements automatically from the project's process configuration. The `devenv:processes:postgres` and `devenv:processes:redis` entries in the task config signal what a project needs.

## Goals

- Trait-based provider architecture for extensibility
- PostgreSQL provider: per-deployment database within a shared instance
- Valkey provider: per-deployment DB number within a shared instance (up to 32)
- Garage provider: per-deployment S3 bucket and API key within a shared cluster
- Automatic discovery from devenv process configuration
- Unix socket authentication where possible (no passwords in environment variables)
- Environment variable injection transparent to application code
- Resource lifecycle: provision on deploy, teardown on branch delete
- Startup reconciliation: detect and clean up orphaned resources

## Non-Goals

- Running per-deployment infrastructure instances (one postgres per branch)
- Managing the shared infrastructure services themselves (those are NixOS-managed)
- Application-level schema migrations (Kennel creates empty databases, applications migrate them)

## Detailed Design

### Provider Trait

```rust
#[async_trait]
pub trait ResourceProvider: Send + Sync {
    /// Provider name, used for logging and resource tracking
    /// (e.g., "postgres", "valkey", "garage").
    fn name(&self) -> &str;

    /// Provision isolated resources for a deployment. Returns environment
    /// variables to inject into the process.
    async fn provision(&self, request: &ResourceRequest) -> Result<HashMap<String, String>>;

    /// Destroy resources when a deployment is torn down.
    async fn teardown(&self, request: &ResourceRequest) -> Result<()>;

    /// Find and clean up resources not associated with any active deployment.
    async fn reconcile(&self, active_deployments: &[DeploymentInfo]) -> Result<ReconciliationSummary>;
}

pub struct ResourceRequest {
    pub deployment_id: i32,
    pub project_name: String,
    pub service_name: String,
    pub branch: String,
    pub branch_slug: String,
    pub environment: String,
    pub system_user: String,
}
```

### Discovery

When the builder evaluates the devenv task configuration, it encounters infrastructure process entries like `devenv:processes:postgres` and `devenv:processes:redis`. These are not deployed as application processes -- they are filtered out. Instead, their presence signals that the project requires the corresponding infrastructure.

The deployer maps devenv process names to resource providers:

| devenv process name | Provider |
|---|---|
| `devenv:processes:postgres` | PostgreSQL |
| `devenv:processes:redis` or `devenv:processes:valkey` | Valkey |
| `devenv:processes:garage` | Garage |

The `DeploymentRequest` carries the list of required resource provider names alongside the process configs.

### PostgreSQL Provider

The shared PostgreSQL instance runs on the host, managed by NixOS. Kennel connects via Unix socket.

**Provisioning:**

- Create a database named `kennel_{project}_{branch_slug}` (e.g., `kennel_terrier_feature_x`)
- Grant the deployment's system user (`kennel-{project}`) ownership of the database
- Return `DATABASE_URL=postgresql:///{database_name}?host=/run/postgresql`

PostgreSQL authenticates via peer authentication -- the OS user matches the database role, no password needed.

**Teardown:**

- Terminate active connections to the database
- Drop the database

**Reconciliation:**

- List all databases matching the `kennel_*` naming pattern
- Compare against active deployments
- Drop orphaned databases

### Valkey Provider

The shared Valkey instance runs on the host with `databases 32` in its configuration, managed by NixOS. Kennel connects via Unix socket.

**Provisioning:**

- Allocate the lowest unused DB number (0-31)
- Return `VALKEY_URL=redis+unix:///run/valkey/valkey.sock?db={db_number}`

No password is needed -- Valkey accepts connections from the Unix socket, and file permissions restrict access.

**Teardown:**

- Flush the allocated DB (`SELECT {db}; FLUSHDB`)
- Release the DB number

**Reconciliation:**

- Query all allocated DB numbers from the tracking table
- Compare against active deployments
- Flush and release orphaned DB numbers

The 32-DB limit is sufficient for branch preview deployments. Production and staging deployments on `main` and `staging` branches are long-lived and represent a small fraction of the pool.

### Garage Provider

The shared [Garage](https://garagehq.deuxfleurs.fr/) cluster runs on the host or on dedicated nodes, managed by NixOS. Kennel interacts with Garage's admin API (HTTP on port 3903) using a scoped admin token.

**Provisioning:**

- Create a bucket with global alias `kennel-{project}-{branch_slug}-{service}` via `POST /v2/CreateBucket`
- Create an API key via `POST /v2/CreateKey`
- Grant the key read + write access to the bucket via `POST /v2/AllowBucketKey`
- Return:
  - `S3_ENDPOINT` -- the configured Garage S3 endpoint
  - `S3_BUCKET` -- the bucket name
  - `AWS_ACCESS_KEY_ID` -- the created key's access key
  - `AWS_SECRET_ACCESS_KEY` -- the created key's secret key

Bucket names follow S3 naming rules (3-63 characters, lowercase alphanumeric and hyphens). The `kennel-` prefix and slug sanitization keep names within bounds.

**Teardown:**

- Delete all objects in the bucket
- Delete the bucket via `POST /v2/DeleteBucket`
- Delete the API key via `POST /v2/DeleteKey`

**Reconciliation:**

- List all buckets matching the `kennel-*` naming pattern via `GET /v2/ListBuckets`
- Compare against active deployments
- Delete orphaned buckets and their associated API keys

Kennel authenticates to Garage's admin API using a bearer token configured in the NixOS module. The token is scoped to bucket and key management operations.

### Resource Tracking

The current `preview_databases` table is replaced with a general `provisioned_resources` table tracking all provider allocations. Columns:

- `id` -- primary key
- `provider` -- provider name (`postgres`, `valkey`, `garage`)
- `deployment_id` -- foreign key to deployments
- `project_name`, `service_name`, `branch` -- for reconciliation queries
- `resource_identifier` -- provider-specific identifier (database name, DB number, bucket name)
- `metadata` -- JSONB for provider-specific data (Garage API key ID, bucket UUID, etc.)
- `created_at` -- timestamp

The migration drops the `preview_databases` table and creates `provisioned_resources`.

### Environment Variable Parity

The environment variables injected by Kennel match what applications expect from devenv. devenv's built-in service modules set standard connection variables; Kennel uses the same variable names with connection strings pointing at the shared production infrastructure instead of the local dev instance:

| Service | Variable | Local (devenv) | Production (Kennel) |
|---|---|---|---|
| PostgreSQL | `DATABASE_URL` | `postgresql:///mydb?host=/tmp/...` | `postgresql:///kennel_proj_branch?host=/run/postgresql` |
| Valkey | `VALKEY_URL` | `redis://localhost:6379` | `redis+unix:///run/valkey/valkey.sock?db=5` |
| Garage | `S3_ENDPOINT`, `S3_BUCKET`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | local instance | shared cluster with scoped key |

Applications read these variables at startup. The connection details differ between dev and production but the variable names and semantics are identical. No application code changes are required.

### Integration with Deployer

The deployer calls providers after creating the deployment record and before starting the process through the supervisor:

```rust
async fn deploy_service(
    request: &DeploymentRequest,
    config: &DeployerConfig,
    supervisor: &mut Supervisor,
) -> Result<()> {
    let mut process_config = request.process_config.clone();

    for provider in &config.resource_providers {
        if request.requires_resource(provider.name()) {
            let env_vars = provider.provision(&resource_request).await?;
            process_config.env.extend(env_vars);
        }
    }

    supervisor.start(process_config).await?;

    Ok(())
}
```

Teardown calls providers after stopping the process:

```rust
async fn teardown_deployment(
    deployment: &Deployment,
    config: &DeployerConfig,
    supervisor: &mut Supervisor,
) -> Result<()> {
    supervisor.stop(&deployment.process_name, grace).await?;

    for provider in &config.resource_providers {
        if let Ok(resources) = store.provisioned_resources()
            .find_by_deployment(deployment.id, provider.name())
            .await
        {
            if !resources.is_empty() {
                provider.teardown(&resource_request).await?;
            }
        }
    }

    Ok(())
}
```

### Configuration

NixOS module options for each provider:

```nix
services.kennel.resources = {
  postgres = {
    enable = mkEnableOption "PostgreSQL resource provisioning";
    socketDir = mkOption {
      type = types.path;
      default = "/run/postgresql";
    };
  };

  valkey = {
    enable = mkEnableOption "Valkey resource provisioning";
    socketPath = mkOption {
      type = types.path;
      default = "/run/valkey/valkey.sock";
    };
    maxDatabases = mkOption {
      type = types.int;
      default = 32;
    };
  };

  garage = {
    enable = mkEnableOption "Garage resource provisioning";
    adminEndpoint = mkOption {
      type = types.str;
      default = "http://localhost:3903";
    };
    s3Endpoint = mkOption {
      type = types.str;
      default = "http://localhost:3900";
    };
    adminTokenFile = mkOption {
      type = types.path;
      example = "/run/secrets/garage-admin-token";
    };
  };
};
```

## Alternatives Considered

**Per-deployment infrastructure instances.** Run a separate PostgreSQL, Valkey, and Garage per branch deployment. This provides perfect isolation but is wasteful -- each instance consumes memory and CPU even when idle. Shared instances with per-deployment isolation strike a better balance for preview deployments.

**Application-level isolation (key prefixing for Valkey).** Require applications to prefix all Valkey keys with the deployment name. This leaks deployment concerns into application code and requires every library and framework to cooperate. DB number isolation is transparent.

**Hardcoded provider implementations.** Implement each provider directly in the deployer without a trait. Simpler initially but makes adding new providers require modifying core deployer code instead of implementing a trait.

## Open Questions

- **Garage bucket cleanup.** Deleting all objects in a bucket before deletion could be slow for large buckets. Should Kennel set a lifecycle policy on creation instead, or is synchronous deletion acceptable for preview deployments?

- **Cross-deployment resource sharing.** Deployments of the same project on the same branch currently share a database (`kennel_terrier_feature_x` persists across redeployments of `feature-x`), preserving data between redeploys. Should this be the default for all providers?

## Implementation Phases

### Provider Trait and Resource Tracking

Define `ResourceProvider` trait, `ResourceRequest`, and `ReconciliationSummary` types. Create the `provisioned_resources` migration (replacing `preview_databases`). Generate entities. Implement store methods for resource CRUD.

### PostgreSQL Provider

Implement `PostgresProvider` with provision (create database, grant user), teardown (terminate connections, drop database), and reconciliation (list `kennel_*` databases, compare against active deployments). Write integration tests against a real PostgreSQL instance.

### Valkey Provider

Implement `ValkeyProvider` with provision (allocate DB number), teardown (flush, release), and reconciliation. Configure Valkey with `databases 32`. Write integration tests.

### Garage Provider

Implement `GarageProvider` using `reqwest` against the Garage admin API. Provision creates bucket and scoped API key. Teardown deletes objects, bucket, and key. Reconciliation lists `kennel-*` buckets. Write integration tests against a Garage instance.

### Discovery Integration

Implement devenv process name -> provider mapping in the builder. Update `DeploymentRequest` to carry required resource types.

### Deployer Integration

Wire providers into the deploy and teardown paths. Inject environment variables from provider responses into `ProcessConfig`. Update startup reconciliation to call `provider.reconcile()` for each enabled provider.

### NixOS Module

Add `services.kennel.resources.*` options. Configure Valkey `databases 32`. Configure Garage admin token. Add assertions for required provider configuration.
