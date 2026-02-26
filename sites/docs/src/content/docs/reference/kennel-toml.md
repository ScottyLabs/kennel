---
title: kennel.toml Reference
description: Configuration file format for Kennel projects
---

Every repository deployed with Kennel must have a `kennel.toml` file at the root. This file defines your services and static sites.

## Basic Structure

```toml
[cachix]
cache_name = "my-cache"
auth_token_file = "/path/to/token"

[services.api]
preview_database = true
health_check = "/health"
custom_domain = "api.example.com"

[static_sites.docs]
spa = true
```

## Services

Services are backend applications that run as systemd services. Each service must have a corresponding Nix package.

```toml
[services.<name>]
```

### Service Options

`flake_output` (string, optional)

Override the Nix flake output path. By default, Kennel looks for `.#packages.x86_64-linux.<service-name>`. Use this to specify a different output path.

Example:
```toml
[services.api]
flake_output = "my-custom-api"
```

This will build `.#packages.x86_64-linux.my-custom-api` instead of `.#packages.x86_64-linux.api`.

`preview_database` (boolean, optional, default: false)

Allocate a Valkey database for this service. The database number (0-15) is provided via `VALKEY_URL` environment variable:

```
VALKEY_URL=redis://127.0.0.1:6379/3
```

Databases are allocated per branch and released when the last deployment for that branch is torn down.

`health_check` (string, optional, default: "/health")

HTTP path to poll for health checks. Kennel sends GET requests to `http://localhost:<port><path>` and expects 200 OK during deployment. Uses exponential backoff: 1s, 2s, 4s, 8s, 15s.

`health_check_timeout_secs` (integer, optional, default: 30)

Maximum time in seconds to wait for the health check to succeed during deployment.

After deployment, the router continuously monitors this endpoint every 30 seconds. After 3 consecutive failures, the deployment is removed from routing.

`custom_domain` (string, optional)

Custom domain name for this service. Kennel automatically obtains TLS certificates via ACME. The domain must point to your Kennel server.

Both the custom domain and auto-generated subdomain work simultaneously:
- `https://example.com` (custom)
- `https://<service>-<branch>.<project>.scottylabs.org` (auto-generated)

`env` (object, optional)

Additional environment variables to set for this service. These are written to the secrets file and available to the service process.

```toml
[services.api]
preview_database = true

[services.api.env]
LOG_LEVEL = "debug"
TIMEOUT = "30"
```

`secrets` (array of strings, optional)

List of secret environment variable names that should be read from NixOS configuration. These are merged with the `env` variables.

```toml
[services.api]
secrets = ["DATABASE_PASSWORD", "JWT_SECRET"]
```

The NixOS module provides these secrets, and they're written to `/run/kennel/secrets/<project>-<branch>-<service>.env`.

### Environment Variables

All services receive:

- `PORT` - Allocated port number (18000-19999)
- `VALKEY_URL` - Redis connection string (if `preview_database = true`)
- `DATABASE_URL` - PostgreSQL connection string (if preview database allocated)

Additional environment variables can be configured via secrets files at `/run/kennel/secrets/<project>-<branch>-<service>.env`.

### System User

Each service runs as user `kennel-<project>-<branch>-<service>` with working directory `/var/lib/kennel/services/<project>/<branch>/<service>`.

### Example Service

```toml
[services.api]
preview_database = true
health_check = "/api/health"
custom_domain = "api.myapp.com"
```

Nix package must be defined at `.#packages.x86_64-linux.api`.

## Static Sites

Static sites are served directly from the Nix store via symlinks. No process runs - the router serves files.

```toml
[static_sites.<name>]
```

### Static Site Options

`flake_output` (string, optional)

Override the Nix flake output path. By default, Kennel looks for `.#packages.x86_64-linux.<site-name>`. Use this to specify a different output path.

Example:
```toml
[static_sites.web]
flake_output = "frontend-dist"
```

This will build `.#packages.x86_64-linux.frontend-dist` instead of `.#packages.x86_64-linux.web`.

`spa` (boolean, optional, default: false)

Enable single-page application mode. When enabled, 404 errors return `index.html` instead, allowing client-side routing to work.

Without SPA mode, missing files return 404.

`custom_domain` (string, optional)

Custom domain for this static site. Works the same as service custom domains.

### Example Static Site

```toml
[static_sites.web]
spa = true
custom_domain = "myapp.com"
```

Nix package must be defined at `.#packages.x86_64-linux.web` and output a directory of static files.

## Cachix

Optional Cachix configuration for sharing build artifacts.

```toml
[cachix]
cache_name = "<cache-name>"
auth_token_file = "/path/to/auth-token"
```

`cache_name` (string, required if section present)

Cachix cache name to push to.

`auth_token_file` (string, optional)

Path to file containing Cachix authentication token. The file should contain only the token (whitespace is trimmed). Must be readable by the Kennel process.

If not provided, Kennel will use the `CACHIX_AUTH_TOKEN` environment variable or rely on existing cachix CLI authentication.

If Cachix push fails, a warning is logged but the build continues - deployments work with local store paths.

## Complete Example

```toml
[cachix]
cache_name = "myproject"
auth_token_file = "/var/lib/kennel/cachix-token"

[services.api]
preview_database = true
health_check = "/health"
custom_domain = "api.myapp.com"
secrets = ["DATABASE_PASSWORD", "JWT_SECRET"]

[services.api.env]
LOG_LEVEL = "info"
TIMEOUT = "30"

[services.worker]
preview_database = true
health_check = "/healthz"

[services.worker.env]
QUEUE_SIZE = "100"

[static_sites.web]
spa = true
custom_domain = "myapp.com"

[static_sites.docs]
spa = false
custom_domain = "docs.myapp.com"
```

This configuration defines:
- 2 services (api, worker) with preview databases, custom domains, and environment variables
- 2 static sites (web as SPA, docs as plain HTML) with custom domains
- Cachix caching enabled

All four must have corresponding Nix packages.
