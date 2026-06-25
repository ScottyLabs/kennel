# Secrets

Kennel resolves secrets from OpenBao via [secretspec](https://secretspec.dev). Secrets are injected as environment variables and never written to disk or stored in the database.

## Declaring secrets

Create a `secretspec.toml` in your project root. The `[project]` table requires a `name` and `revision = "1.0"`, and secrets are declared per profile:

```toml
[project]
name = "my-project"
revision = "1.0"

[profiles.default]
JWT_SECRET = { description = "JWT signing key", required = true }
STRIPE_KEY = { description = "Stripe API key", required = true }

# Declare all profiles, even if you only use default
# default secrets can only be substituted into existing profiles
[profiles.prod]

[profiles.staging]

[profiles.preview]
STRIPE_KEY = { description = "Stripe test key", required = false }

[profiles.dev]
```

`[profiles.default]` holds the secrets shared across every profile; named profiles inherit from it and may override individual entries, for example making `STRIPE_KEY` optional in preview. Declare a `[profiles.<name>]` header for `dev` (used locally) and for every environment you deploy to (see the [branch-to-profile mapping](#production)); a section may be left empty to inherit `default` unchanged.

## Configuring the provider

Point secretspec at OpenBao in your `devenv.yaml`. Copy this block once per project:

```yaml
secretspec:
  enable: true
  provider: vault://secrets2.scottylabs.org/secret
  profile: dev
```

`enable` turns on resolution, `provider` is the default backend for every secret, and `profile` selects which profile to load locally (kennel chooses the profile per branch when it deploys). The shared ScottyLabs config (`scottylabs.enable = true`) supplies the `bao` and `secretspec` CLIs, sets `BAO_ADDR`, and exports every resolved secret into your shell.

## Per-developer secrets

Some secrets differ per developer and cannot be shared, for example each person's own `DISCORD_TOKEN` while developing a bot. Source those from a gitignored `.env` instead of OpenBao: define a `local` alias in a `[providers]` table and give the secret a provider chain in the `dev` profile.

```toml
[providers]
local = "dotenv://.env"

[profiles.prod]
DISCORD_TOKEN = { description = "Discord bot token" }

[profiles.dev]
DISCORD_TOKEN = { providers = ["local"] }
```

`prod` resolves `DISCORD_TOKEN` from OpenBao (the default provider) while `dev` reads it from your `.env`. Chains are tried in order, so `providers = ["vault", "local"]` would try OpenBao first and fall back to `.env`; every alias you name must be defined in this committed `[providers]` table. Keep `.env` gitignored.

## Local development

Log in to OpenBao once per machine:

```bash
nix run git+https://codeberg.org/ScottyLabs/devenv#login
```

A project shell resolves secrets as it loads, so it won't build without a token. On a fresh checkout you don't have one and can't reach `bao` from inside the shell yet, hence the standalone command above.

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
