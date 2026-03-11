# RFC 0012: Secrets Management

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-03-11
- **Updated:** 2026-03-11

## Overview

Integrate [secretspec](https://github.com/cachix/secretspec) for declarative secrets management. Projects define their secrets in `secretspec.toml`, which devenv resolves locally and Kennel resolves from a platform-managed Vault/OpenBao instance at deploy time. This eliminates the manual secrets file generation in the deployer and the `secrets` field in `kennel.toml`.

## Motivation

Secrets management in Kennel is currently a stub. The deployer generates environment files at `/run/kennel/secrets/` with hardcoded variables (PORT, DATABASE_URL, VALKEY_URL), and `kennel.toml` has a `secrets` field that lists secret names but has no backend to resolve them. There is no actual secret fetching from any vault.

secretspec solves this cleanly:

- Projects declare what secrets they need in `secretspec.toml` (checked into version control)
- In local dev, devenv resolves secrets from the developer's configured provider (keyring, .env, 1Password)
- In production, Kennel resolves the same secrets from the platform's Vault/OpenBao instance
- The Vault storage backend (contributed in secretspec v0.8.0) stores secrets at `secretspec/{project}/{profile}/{key}`

This follows the same "define once, run everywhere" pattern as devenv process configuration and infrastructure provisioning.

## Goals

- Use secretspec's Rust library to parse `secretspec.toml` from cloned repositories
- Resolve secrets from a platform-managed Vault/OpenBao instance at deploy time
- Inject resolved secrets as environment variables into the process config before supervisor start
- Profile selection based on deployment environment (prod, staging, dev, preview)
- Remove the `secrets` field from `kennel.toml` (secretspec.toml replaces it)
- Remove the manual env file generation in the deployer

## Non-Goals

- Managing the Vault/OpenBao instance itself (that's NixOS infrastructure)
- Interactive secret setting (Kennel only reads, developers set via `secretspec set`)
- Secret rotation or renewal at runtime (secrets are resolved at deploy time)

## Detailed Design

### Secret Declaration

Projects declare secrets in `secretspec.toml` at the repository root:

```toml
[project]
name = "myapp"

[profiles.default]
DATABASE_PASSWORD = { description = "PostgreSQL password", required = true }
JWT_SECRET = { description = "JWT signing key", required = true, generate = { length = 64 } }
SMTP_PASSWORD = { description = "SMTP credentials", required = false }

[profiles.production]
DATABASE_PASSWORD = { description = "PostgreSQL password", required = true }
JWT_SECRET = { description = "JWT signing key", required = true }
SMTP_PASSWORD = { description = "SMTP credentials", required = true }
STRIPE_SECRET_KEY = { description = "Stripe API key", required = true }
```

In local dev, `devenv.nix` references these:

```nix
{ config, ... }: {
  env.DATABASE_PASSWORD = config.secretspec.secrets.DATABASE_PASSWORD;
  env.JWT_SECRET = config.secretspec.secrets.JWT_SECRET;
}
```

### Resolution at Deploy Time

During deployment, after the builder clones the repository and before the supervisor starts the process, the deployer resolves secrets:

```rust
use secretspec::Secrets;

fn resolve_secrets(
    repo_path: &Path,
    environment: &str,
    vault_endpoint: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let secretspec_path = repo_path.join("secretspec.toml");
    if !secretspec_path.exists() {
        return Ok(HashMap::new());
    }

    let mut spec = Secrets::load_from(&secretspec_path)?;
    spec.set_provider(vault_endpoint);
    spec.set_profile(environment);

    let validated = spec.ensure_secrets(None, None, false)?;

    Ok(validated
        .resolved
        .secrets
        .into_iter()
        .map(|(k, v)| (k, v.expose_secret().to_string()))
        .collect())
}
```

The Vault endpoint is configured in the NixOS module. The profile maps directly to the deployment environment: `prod`, `staging`, `dev`, or `preview`.

### Integration with Deployer

Secret resolution slots into the deployment flow between infrastructure provisioning and process start, in `deploy_service()`:

- Resource providers inject `DATABASE_URL`, `VALKEY_URL`, etc.
- Secret resolution injects application secrets (`JWT_SECRET`, `STRIPE_KEY`, etc.)
- Both are merged into `ProcessConfig.env` before the supervisor starts the process

The existing `secrets::generate_env_file()` function is removed. Secrets are environment variables in the process config, not files on disk.

### Vault Path Convention

secretspec stores secrets at `secretspec/{project}/{profile}/{key}` in the Vault KV engine. For a project named "myapp" in the "prod" profile:

```
GET /v1/secret/data/secretspec/myapp/prod/DATABASE_PASSWORD
GET /v1/secret/data/secretspec/myapp/prod/JWT_SECRET
```

Authentication uses `VAULT_TOKEN` from the environment or `~/.vault-token` file. The NixOS module provides the token via an environment file.

### kennel.toml Changes

The `secrets` field is removed from `ServiceConfig`:

```toml
# Before
[services.api]
custom_domain = "api.myapp.com"
secrets = ["DATABASE_PASSWORD", "JWT_SECRET"]

# After
[services.api]
custom_domain = "api.myapp.com"
```

Secret declarations live in `secretspec.toml`, not `kennel.toml`.

### NixOS Module

```nix
services.kennel.secrets = {
  enable = mkEnableOption "secretspec secret resolution";
  vaultEndpoint = mkOption {
    type = types.str;
    default = "vault://127.0.0.1:8200/secret";
    description = "Vault/OpenBao endpoint URI for secret resolution";
  };
  tokenFile = mkOption {
    type = types.path;
    example = "/run/secrets/kennel-vault-token";
    description = "Path to file containing the Vault token";
  };
};
```

### Deployer Secrets Module

Secret resolution lives in the deployer as a `secrets` module:

```rust
pub fn resolve_secrets(
    repo_path: &Path,
    environment: &str,
    vault_endpoint: &str,
) -> anyhow::Result<HashMap<String, String>>;
```

The deployer depends on `secretspec` with the `vault` feature flag.

## Alternatives Considered

**Direct Vault API integration.** Read secrets from Vault using reqwest, bypassing secretspec. This works but loses the declarative `secretspec.toml` manifest and the local dev parity with devenv's secretspec integration.

**Secrets as files.** Write secrets to files on disk (the current stub approach) rather than environment variables. This adds filesystem management complexity and requires the process to read files instead of env vars.

**Parse secretspec.toml only, use own Vault client.** Read the manifest to discover what secrets are needed, but resolve them via a custom Vault client. This duplicates the provider logic that secretspec already implements and tests.

## Open Questions

- **Vault authentication method.** Token auth is simplest but requires token renewal. AppRole or Kubernetes auth may be better for production. secretspec currently only supports token auth for Vault.

## Implementation Phases

### Secrets Module

Add `secretspec` with `vault` feature as a dependency of the deployer. Implement `resolve_secrets()` in a `secrets` module that loads `secretspec.toml`, sets provider and profile, and returns resolved secrets as a HashMap. Call it in `deploy_service()` after infrastructure provisioning and merge into `ProcessConfig.env`.

### Update kennel.toml

Remove the `secrets` field from `ServiceConfig`. Update config parsing and documentation.

### NixOS Module

Add `services.kennel.secrets.*` options. Pass Vault endpoint and token file to the Kennel process environment.

### Documentation

Update the docs site with secretspec integration guide. Add a section on setting up secrets for a project.
