# kennel.toml Reference

Every repository deployed with Kennel must have a `kennel.toml` file at the root. This file defines Kennel-specific deployment metadata. Process configuration (health checks, restart policies, ports, dependencies) comes from your project's [devenv.nix](https://devenv.sh/) process definitions.

## Basic Structure

```toml
[cachix]
cache = "my-cache"
signing_key_file = "/path/to/key"

[services.api]
custom_domain = "api.example.com"
secrets = ["DATABASE_PASSWORD"]

[static_sites.docs]
spa = true
```

## Services

Services are backend applications managed by the process supervisor. Process configuration -- how to run the service, its readiness probes, restart policy, dependencies, and ports -- is defined in `devenv.nix`, not `kennel.toml`.

`kennel.toml` only contains Kennel-specific deployment metadata for each service:

```toml
[services.<name>]
```

### Service Options

`custom_domain` (string, optional)

Custom domain name for this service. Kennel automatically obtains TLS certificates via ACME. The domain must point to your Kennel server.

Both the custom domain and auto-generated subdomain work simultaneously:

- `https://example.com` (custom)
- `https://<service>-<branch>.<project>.scottylabs.org` (auto-generated)

`secrets` (list of strings, optional)

Secret names to inject from OpenBao. These are fetched at deploy time and written to the service's environment file.

### Environment Variables

Infrastructure resource URLs are injected automatically by Kennel's resource providers based on the project's devenv configuration:

- `DATABASE_URL` -- PostgreSQL connection string (if `services.postgres.enable = true` in devenv.nix)
- `VALKEY_URL` -- Valkey connection string (if `services.redis.enable = true` in devenv.nix)
- `S3_ENDPOINT`, `S3_BUCKET`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` -- Garage credentials (if `services.garage.enable = true` in devenv.nix)
- `ENVIRONMENT` -- deployment environment (prod, staging, dev, preview)

### System User

Each service runs as user `kennel-<project>-<branch>-<service>` with working directory `/var/lib/kennel/services/<project>/<branch>/<service>`.

### Example Service

```toml
[services.api]
custom_domain = "api.myapp.com"
secrets = ["JWT_SECRET"]
```

The corresponding devenv.nix defines how the service runs:

```nix
processes.api = {
  exec = "${pkgs.myapp}/bin/api";
  ready.http.get = { port = config.processes.api.ports.http.value; path = "/health"; };
  restart.on = "on_failure";
  after = [ "devenv:processes:postgres" ];
};
```

## Static Sites

Static sites are served directly from the Nix store via symlinks. No process runs -- the router serves files.

```toml
[static_sites.<name>]
```

### Static Site Options

`flake_output` (string, optional)

Override the Nix flake output path. Defaults to `packages.x86_64-linux.<name>`.

`spa` (boolean, optional, default: false)

Enable single-page application mode. When enabled, 404 errors return `index.html` instead, allowing client-side routing to work.

`custom_domain` (string, optional)

Custom domain for this static site. Works the same as service custom domains.

### Example Static Site

```toml
[static_sites.web]
spa = true
custom_domain = "myapp.com"
```

## Cachix

Optional Cachix configuration for sharing build artifacts.

```toml
[cachix]
cache = "<cache-name>"
signing_key_file = "/path/to/signing-key"
```

`cache` (string, required if section present)

Cachix cache name to push to.

`signing_key_file` (string, required if section present)

Path to Cachix signing key file. Must be readable by the Kennel process.

If Cachix push fails, a warning is logged but the build continues -- deployments work with local store paths.

## Complete Example

```toml
[cachix]
cache = "myproject"
signing_key_file = "/var/lib/kennel/cachix-key"

[services.api]
custom_domain = "api.myapp.com"
secrets = ["DATABASE_PASSWORD", "JWT_SECRET"]

[services.worker]

[static_sites.web]
spa = true
custom_domain = "myapp.com"

[static_sites.docs]
spa = false
custom_domain = "docs.myapp.com"
```

This configuration defines:

- 2 services (api with custom domain and secrets, worker with defaults)
- 2 static sites (web as SPA, docs as plain HTML)
- Cachix caching enabled

Process definitions (health checks, ports, restart policies, dependencies) for api and worker come from the project's `devenv.nix`.
