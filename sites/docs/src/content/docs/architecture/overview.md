---
title: Architecture Overview
description: How Kennel transforms Git pushes into running deployments
---

Kennel runs as a single binary with four subsystems: webhook receiver, builder, deployer, and router.

## Request Flow

```
Git Push -> Webhook -> Builder -> Deployer -> Router -> Live Site
```

See the [usage guide](../guides/usage#push-to-deploy) for a detailed explanation of the deployment process.

## Component Responsibilities

### Webhook Receiver

Accepts webhook events from Git servers, verifies signatures, and creates build records. See the [webhooks guide](../guides/webhooks) for configuration details.

### Builder

Runs Nix builds in a worker pool with configurable concurrency. Clones repositories, parses kennel.toml, and builds all services and static sites. See the [usage guide](../guides/usage#build-process) for details.

### Deployer

Creates systemd units for services and symlinks for static sites. Implements blue-green deployment for zero downtime. See the [usage guide](../guides/usage#deployment-process) for the full deployment flow.

### Router

Reverse proxy that routes traffic based on Host header. Handles both auto-generated subdomains and custom domains. Automatically obtains TLS certificates via ACME. See the [usage guide](../guides/usage#routing) for routing details.

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
