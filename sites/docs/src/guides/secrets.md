# Secrets

Kennel resolves secrets from OpenBao via [secretspec](https://secretspec.dev). Secrets are injected as environment variables and never written to disk or stored in the database.

## Declaring secrets

Create a `secretspec.toml` in your project root. The `[project]` table requires a `name` and `revision = "1.0"`, and secrets are declared per profile:

```toml
[project]
name = "my-project"
revision = "1.0"

[providers]
local = "dotenv://.env"
vault = "vault://secrets.scottylabs.org/secret"

[profiles.ci.defaults]
required = false

[profiles.default]
JWT_SECRET = { description = "JWT signing key", required = true }
STRIPE_KEY = { description = "Stripe API key", required = true }

# Declare all profiles, even if you only use default
# default secrets can only be substituted into existing profiles
[profiles.prod]

[profiles.staging]

[profiles.preview]
STRIPE_KEY = { description = "Stripe test key", required = false }

[profiles.dev.defaults]
providers = ["vault"]

[profiles.dev]
```

`[profiles.default]` holds the secrets shared across every profile; named profiles inherit from it and may override individual entries, for example making `STRIPE_KEY` optional in preview. Declare a `[profiles.<name>]` header for `dev` (used locally) and for every environment you deploy to (see the [branch-to-profile mapping](#production)); a section may be left empty to inherit `default` unchanged.

The `[providers]` table names the backends once. `vault` is OpenBao, where shared secrets live, and `local` is a gitignored `.env` for [per-developer secrets](#per-developer-secrets). `[profiles.dev.defaults]` routes every dev secret without an explicit chain to OpenBao; without it, secrets inherited from `default` have no route when you enter the shell and resolution fails.

The `[profiles.ci.defaults]` block makes every inherited secret optional for CI. The shared CI workflow sets `SECRETSPEC_PROFILE=ci` and `SECRETSPEC_PROVIDER=env` so that `devenv shell` does not require an OpenBao token. CI checks run without project secret values and must not depend on them. The workflow's only secrets are the org-level Cachix and sccache credentials it forwards for binary caching.

## Enabling resolution

Turn secretspec on in your `devenv.yaml`:

```yaml
secretspec:
  enable: true
  profile: dev
```

`enable` turns on resolution and `profile` selects which profile to load locally (kennel chooses the profile per branch when it deploys). The shared ScottyLabs config (`scottylabs.enable = true`) supplies the `bao` and `secretspec` CLIs, sets `BAO_ADDR`, and exports every resolved secret into your shell.

## Per-developer secrets

Some secrets differ per developer and cannot be shared, for example each person's own `DISCORD_TOKEN` while developing a bot. Declare those in the profiles that use them rather than in `[profiles.default]`, and pin the `dev` entry to the `local` alias so it reads from your `.env`:

```toml
[profiles.prod]
DISCORD_TOKEN = { description = "Discord bot token" }

[profiles.dev]
DISCORD_TOKEN = { description = "Discord bot token", providers = ["local"] }
```

`prod` resolves `DISCORD_TOKEN` from OpenBao while `dev` reads it from your `.env`. Chains are tried in order, so `providers = ["vault", "local"]` would try OpenBao first and fall back to `.env`; every alias you name must be defined in the committed `[providers]` table.

Commit a `.env.example` listing each personal secret as an empty `KEY=` line and point developers at it from your README (`cp .env.example .env`). A key that is present but empty counts as set, so a fresh copy enters the shell fine and real values get filled in when the code actually needs them. Without the file, entry fails on any required pinned secret. Keep `.env` itself gitignored.

## Local development

Log in to OpenBao and configure Cachix once per machine:

```bash
nix run git+https://codeberg.org/ScottyLabs/devenv#login
```

A project shell resolves secrets as it loads, so it won't build without a token. On a fresh checkout you don't have one and can't reach `bao` from inside the shell yet, hence the standalone command above. It also stores your Cachix auth token so devenv pulls from and pushes to the shared binary cache.

The token is periodic and renews on each shell entry, so it stays valid as long as you open a project shell at least once every 90 days. You only log in again on a new machine, or after 90 days without using it.

Secrets then load into your shell automatically each time direnv reloads.

## Managing secrets

Set a secret for the default (dev) profile:

```bash
secretspec set JWT_SECRET
```

Set a secret for a specific profile:

```bash
secretspec set -P prod STRIPE_KEY
secretspec set -P preview STRIPE_KEY
```

Verify all required secrets are present:

```bash
secretspec check
secretspec check -P prod
```

## Production

Kennel authenticates to OpenBao with a service token provided via `VAULT_TOKEN` in its environment file. It resolves secrets for each deployment using the profile matching the branch:

| Branch | Profile |
|--------|---------|
| `main` | `prod` |
| `staging` | `staging` |
| `dev` | `dev` |
| `pr-*` | `preview` |

If a required secret cannot be resolved, the deployment fails. Deployed services also receive `PORT`, `COMMIT_HASH`, and `APP_URL` (see [Deploying a Project](./deploying.md#runtime-environment)).

## Runtime loading

Use the typed secretspec SDKs to load secrets in your application code. Environment variables work but lose type safety and expose every secret to every process. The SDKs generate typed accessors from your `secretspec.toml` at build time.

Runtime `load()` reads `secretspec.toml` from disk, not only at build time. Kennel makes it available to deployed services automatically (see [Runtime environment](./deploying.md#runtime-environment)).

- [Rust](https://secretspec.dev/sdk/rust/)
- [TypeScript](https://secretspec.dev/sdk/nodejs/)
- [Python](https://secretspec.dev/sdk/python/)

See the [SDK overview](https://secretspec.dev/sdk/overview/) for the full list of supported languages.
