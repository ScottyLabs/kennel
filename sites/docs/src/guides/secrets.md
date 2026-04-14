# Secrets

Kennel resolves secrets from OpenBao via [secretspec](https://secretspec.dev). Secrets are injected as environment variables at deploy time and never written to disk or stored in the database.

## Declaring secrets

Create a `secretspec.toml` in your project root:

```toml
[project]
name = "my-project"

[secrets]
JWT_SECRET = { description = "JWT signing key", required = true }
STRIPE_KEY = { description = "Stripe API key", required = true }

[profiles.preview]
STRIPE_KEY = { description = "Stripe test key", required = false }
```

The `[secrets]` section defines secrets shared across all profiles. Profile sections can override requirements, for example making secrets optional in preview deployments.

## Local development

Enable secrets in your `devenv.nix`:

```nix
scottylabs.secrets.enable = true;
```

This configures devenv's native secretspec integration. Secrets are resolved and exported into the shell environment when you enter `devenv shell`. If you haven't authenticated to OpenBao yet:

```bash
bao login -method=oidc
```

After authenticating, re-enter the shell and your secrets will be available as environment variables.

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

If a required secret cannot be resolved, the deployment fails.
