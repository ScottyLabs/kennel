# Enabling Features

In the ScottyLabs governance repository, add your project to its team's TOML file and list its `features`. Governance provisions everything those features imply:

- `kennel` provisions the webhook that connects your repository to kennel for builds and deployments
- `sentry` creates a Sentry project and writes its DSN to Vault as `SENTRY_DSN` on the `prod` profile
- `posthog` creates a PostHog project and writes its key and host to Vault as `POSTHOG_KEY` and `POSTHOG_HOST` on the `prod` profile
- `cdn` provisions a per-project public-read [Garage](https://garagehq.deuxfleurs.fr/) bucket and writes `CDN_S3_ENDPOINT`, `CDN_S3_BUCKET`, `CDN_ACCESS_KEY_ID`, `CDN_SECRET_ACCESS_KEY`, and `CDN_PUBLIC_URL` to Vault on the `prod` profile
- `oidc_client` provisions prod, staging, and dev Keycloak OIDC clients and writes `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, `KEYCLOAK_URL`, `KEYCLOAK_REALM`, `OAUTH_RELAY_URL`, `PROJECT_GROUP`, and `PROJECT_ADMIN_GROUP` to Vault for each profile
- `admin_client` provisions a Keycloak service-account client with the `view-users`, `manage-users`, and `view-identity-providers` roles, written to Vault as `KEYCLOAK_ADMIN_CLIENT_ID` and `KEYCLOAK_ADMIN_CLIENT_SECRET`

### OIDC

For OIDC authentication, note the distinction between the OAuth relay (`scottylabs.ricochet.enable` in devenv) and the Keycloak OIDC client (`oidc_client` in governance). The OAuth relay is used to relay between the standardized authorized redirect URI used by the OIDC client.

- prod uses the standard `<name>` OIDC client, preview and staging share `<name>-staging`, and dev uses `<name>-dev`
- prod, staging, and preview all share the `https://oauth.scottylabs.org/oauth2/callback` (Ricochet) relay, while dev uses the `http://localhost:8090/oauth2/callback` (local) relay that requires `scottylabs.ricochet.enable` to be enabled in devenv.

Your service receives the client credentials, Keycloak connection settings, and project group paths as env vars, declared in your `secretspec.toml`:

```toml
[profiles.default]
OIDC_CLIENT_ID = { description = "Keycloak OIDC client ID" }
OIDC_CLIENT_SECRET = { description = "Keycloak OIDC client secret" }
KEYCLOAK_URL = { description = "Keycloak base URL" }
KEYCLOAK_REALM = { description = "Keycloak realm" }
OAUTH_RELAY_URL = { description = "OAuth relay callback URL" }
PROJECT_GROUP = { description = "Keycloak project group path" }
PROJECT_ADMIN_GROUP = { description = "Keycloak project admin group path" }

# Declare all profiles, even if you only use default
[profiles.prod]

[profiles.staging]

[profiles.preview]

[profiles.dev]
```

Your service builds its own callback URL from `APP_URL` and the relay forwards the authorization code there after login. Set it with [`scottylabs.ricochet.appUrl`](../reference/devenv-options.md#scottylabsricochetappurl) in local development; in deployments kennel injects `APP_URL` from the domain (see [runtime environment](./deploying.md#runtime-environment)).

The OIDC client also adds the user's group memberships to the token as a `groups` claim, in full-path form like `/projects/<slug>`. Match it against `PROJECT_GROUP` and `PROJECT_ADMIN_GROUP` to grant enhanced permissions.

### Sentry

If your service reports errors to Sentry, add `sentry` to its `features` in governance. It creates a Sentry project and writes the project DSN to Vault as `SENTRY_DSN` on the `prod` profile, so declare it there:

```toml
[profiles.prod]
SENTRY_DSN = { description = "Sentry project DSN" }
```

Read `SENTRY_DSN` at startup and pass it to the Sentry SDK to turn on error reporting, following [Sentry's setup guide](https://docs.sentry.io/platforms/). Without a DSN, in local development and previews, the SDK stays inert.

### PostHog

If your service sends product analytics to PostHog, add `posthog` to its `features` in governance. It creates a PostHog project and writes the project key and host to Vault as `POSTHOG_KEY` and `POSTHOG_HOST` on the `prod` profile, so declare them there:

```toml
[profiles.prod]
POSTHOG_KEY = { description = "PostHog project API key" }
POSTHOG_HOST = { description = "PostHog instance host" }
```

Read them at startup and pass them to the PostHog SDK to turn on analytics. Without a key, in local development and previews, the SDK stays inert.

### CDN

If your service serves static assets from object storage, add `cdn` to its `features` in governance. It provisions a per-project [Garage](https://garagehq.deuxfleurs.fr/) bucket with website mode enabled and a scoped access key, then writes the S3 credentials and public URL to Vault on the `prod` profile, so declare them there:

```toml
[profiles.prod]
CDN_S3_ENDPOINT = { description = "Garage S3 endpoint" }
CDN_S3_BUCKET = { description = "CDN bucket name" }
CDN_ACCESS_KEY_ID = { description = "CDN bucket access key ID" }
CDN_SECRET_ACCESS_KEY = { description = "CDN bucket secret access key" }
CDN_PUBLIC_URL = { description = "Public base URL for the bucket" }
```

Upload assets with the S3 credentials; they are publicly readable under `CDN_PUBLIC_URL` (`https://cdn.scottylabs.org/<repo>/`).

## Documentation

Documentation is controlled separately by `docs` (boolean, default `true`), which aggregates the repository's `./docs` directory into the documentation hub.
