---
title: Architecture Overview
description: How Kennel transforms Git pushes into running deployments
---

Kennel runs as a single binary with four subsystems: webhook receiver, builder, deployer, and router.

## Request Flow

```
Git Push -> Webhook -> Builder -> Deployer -> Router -> Live Site
```

When you push to a Git repository:

1. Forgejo/GitHub sends a webhook to Kennel
2. Kennel verifies the signature and creates a build record
3. The builder clones your repo and runs `nix build` for each service
4. The deployer creates systemd units (for services) or symlinks (for static sites)
5. The router starts sending traffic to your new deployment

## Component Responsibilities

### Webhook Receiver

Accepts POST requests at `/webhook/:project`. Verifies HMAC-SHA256 signatures against the project's webhook secret. Creates build records and enqueues them for building.

Supports push events (new commits) and pull request events (opened, synchronized, closed). Branch deletions trigger teardown of existing deployments.

### Builder

Runs a worker pool that processes builds concurrently (default: 2 at a time). For each build:

- Clones the repository at the specified commit
- Parses `kennel.toml` to discover services and static sites
- Runs `nix build .#packages.x86_64-linux.<name>` for each
- Compares store paths to detect unchanged builds
- Records success/failure per service
- Triggers deployment on success

Checks for cancellation before each major step (clone, parse, each build).

### Deployer

Manages the deployment lifecycle. For services:

- Allocates a port from the 18000-19999 range
- Creates a system user `kennel-<project>-<branch>-<service>`
- Generates environment file with PORT, DATABASE_URL, etc.
- Writes systemd unit file
- Starts the service and polls `/health` endpoint
- Updates database to mark deployment as active
- Notifies router to start sending traffic

For static sites:

- Creates symlink at `/var/lib/kennel/sites/<project>/<branch>/<site>`
- Points symlink to Nix store path
- Records deployment with SPA flag for router

Implements blue-green deployment: when deploying a new version of an existing service, the new version starts first, then after 30 seconds the old version stops.

Runs cleanup job every 10 minutes to tear down expired deployments.

### Router

Reverse proxy listening on port 80 (and 443 with TLS). Routes based on Host header:

- `<service>-<branch>.<project>.scottylabs.org` - auto-generated subdomain
- Custom domains configured per service

For services: proxies to `http://127.0.0.1:<port>` with X-Forwarded-* headers

For static sites: serves files from symlink path with SPA fallback (returns index.html for 404s)

Monitors health continuously and removes unhealthy deployments after 3 consecutive failures.

Obtains TLS certificates automatically via ACME HTTP-01 and TLS-ALPN-01 challenges.

## Database State

All state lives in PostgreSQL:

- `projects` - Git repositories with webhook secrets
- `builds` - Build records (queued, building, success, failed, cancelled)
- `build_results` - Per-service results with store paths
- `services` - Service definitions with health check paths and custom domains
- `deployments` - Running deployments with ports, domains, status
- `port_allocations` - Which ports are in use
- `preview_databases` - Valkey database numbers allocated per branch

## Configuration

Environment variables:

- `DATABASE_URL` - PostgreSQL connection string
- `BASE_DOMAIN` - Base domain for auto-generated subdomains (default: scottylabs.org)
- `MAX_CONCURRENT_BUILDS` - Build worker pool size (default: 2)
- `WORK_DIR` - Build workspace directory (default: /var/lib/kennel/builds)
- `ROUTER_ADDR` - Router bind address (default: 0.0.0.0:80)
- `API_HOST` / `API_PORT` - API server bind (default: 0.0.0.0:3000)
- `TLS_ENABLED` - Enable HTTPS (default: false)
- `ACME_EMAIL` - Email for Let's Encrypt
- `ACME_PRODUCTION` - Use Let's Encrypt production (default: false, uses staging)
- `ACME_CACHE_DIR` - Certificate cache (default: /var/lib/kennel/acme)

All defaults are defined in `kennel-config::constants`.

## Communication Patterns

Components communicate via typed channels:

- Webhook -> Builder: `mpsc::channel<i64>` for build IDs
- Builder -> Deployer: `mpsc::channel<DeploymentRequest>` 
- Deployer -> Router: `broadcast::channel<RouterUpdate>` for routing table changes
- All -> Database: shared `Arc<Store>` with SeaORM repository pattern

The router also reloads its full routing table every 60 seconds as a safety net.

## Graceful Shutdown

On SIGTERM or Ctrl-C, Kennel waits up to 300 seconds for all components to finish their current work before forcing exit.
