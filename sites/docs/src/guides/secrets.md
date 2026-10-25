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
openbao = "openbao://secrets.scottylabs.org/secret"

[profiles.default]
AWS_ACCESS_KEY_ID = { description = "sccache S3 access key ID", ref = { item = "shared/sccache", field = "AWS_ACCESS_KEY_ID" } }
AWS_SECRET_ACCESS_KEY = { description = "sccache S3 secret access key", ref = { item = "shared/sccache", field = "AWS_SECRET_ACCESS_KEY" } }
JWT_SECRET = { description = "JWT signing key" }
STRIPE_KEY = { description = "Stripe API key" }

[profiles.prod]

[profiles.staging]

[profiles.preview]
STRIPE_KEY = { description = "Stripe test key", required = false }

[profiles.dev.defaults]
providers = ["openbao"]

[profiles.dev]
```

`[project] name` must be the slug governance stores your secrets under, which is the project's slug, or the team's slug for a team without projects. A deploy fails at the `secrets` stage when the name doesn't match.

`[profiles.default]` holds the secrets shared across profiles; named profiles inherit from it and may override an entry, for example `preview` making `STRIPE_KEY` optional. Declare a `[profiles.<name>]` header for every environment.

The `[providers]` table defines `openbao`, which logs in with your local `bao` token, and `local`, a gitignored `.env` for [per-developer secrets](#per-developer-secrets). `[profiles.dev.defaults]` makes local development read from `openbao`. Deployed profiles need no `providers`, since kennel sets its own.

Forgejo Actions jobs that need secrets use an `openbao-ci` provider, set up in [Custom workflows](./ci.md#custom-workflows).

## Enabling resolution

The flake module resolves secrets whenever the project has a `secretspec.toml` (`scottylabs.secrets.enable`, on by default). It adds the `bao` and `secretspec` CLIs, sets `BAO_ADDR`, and exports every resolved secret into your shell with `secretspec export`, using the profile in `SECRETSPEC_PROFILE` (`dev` when unset). Kennel chooses the profile per branch when it deploys.

## Per-developer secrets

Some secrets differ per developer and cannot be shared, for example each person's own `DISCORD_TOKEN` while developing a bot. Declare those in the profiles that use them rather than in `[profiles.default]`, and pin the `dev` entry to the `local` alias so it reads from your `.env`:

```toml
[profiles.prod]
DISCORD_TOKEN = { description = "Discord bot token" }

[profiles.dev]
DISCORD_TOKEN = { description = "Discord bot token", providers = ["local"] }
```

`prod` resolves `DISCORD_TOKEN` from OpenBao while `dev` reads it from your `.env`. Chains are tried in order, so `providers = ["openbao", "local"]` would try OpenBao first and fall back to `.env`; every alias you name must be defined in the committed `[providers]` table.

Commit a `.env.example` listing each personal secret as an empty `KEY=` line and point developers at it from your README (`cp .env.example .env`). A key that is present but empty counts as set, so a fresh copy enters the shell fine and real values get filled in when the code actually needs them. Without the file, entry fails on any required pinned secret. Keep `.env` itself gitignored.

## Local development

Log in to OpenBao once per machine:

```bash
nix run git+https://git.cmu.dev/ScottyLabs/kennel#login
```

A project shell resolves secrets as it loads and only warns when it has no token, leaving the secrets unset. The standalone command above logs in without a project shell.

The token is periodic and renews on each shell entry, so it stays valid as long as you open a project shell at least once every 90 days. You only log in again on a new machine, or after 90 days without using it.

Secrets then load into your shell on entry.

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

Values a browser frontend reads go in a `[scopes.public]` table, which kennel serves to the site for the branch's profile. Only values safe to publish belong there (see [Public values](./deploying.md#public-values)).

## Runtime loading

Use the typed secretspec SDKs to load secrets in your application code. Environment variables work but lose type safety and expose every secret to every process. The SDKs generate typed accessors from your `secretspec.toml` at build time.

Runtime `load()` reads `secretspec.toml` from disk, not only at build time. Kennel makes it available to deployed services automatically (see [Runtime environment](./deploying.md#runtime-environment)).

- [Rust](https://secretspec.dev/sdk/rust/)
- [TypeScript](https://secretspec.dev/sdk/nodejs/)
- [Python](https://secretspec.dev/sdk/python/)
- [Haskell](https://secretspec.dev/sdk/haskell/)

See the [SDK overview](https://secretspec.dev/sdk/overview/) for the full list of supported languages.
