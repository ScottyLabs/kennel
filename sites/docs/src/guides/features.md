# Enabling Features

In the ScottyLabs governance repository, add your project to its team's TOML file and list its `features`. Governance provisions everything those features imply:

- `kennel` provisions the webhook that connects your repository to kennel for builds and deployments
- `sentry` creates a Sentry project and writes its DSN to Vault as `SENTRY_DSN` on the `prod` profile
- `oidc_client` provisions prod, staging, and dev Keycloak OIDC clients and writes `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, `KEYCLOAK_URL`, `KEYCLOAK_REALM`, and `OAUTH_RELAY_URL` to Vault for each profile
- `admin_client` provisions a Keycloak service-account client with the `view-users`, `manage-users`, and `view-identity-providers` roles, written to Vault as `KEYCLOAK_ADMIN_CLIENT_ID` and `KEYCLOAK_ADMIN_CLIENT_SECRET`

### OIDC

For OIDC authentication, note the distinction between the OAuth relay (`scottylabs.ricochet.enable` in devenv) and the Keycloak OIDC client (`oidc_client` in governance). The OAuth relay is used to relay between the standardized authorized redirect URI used by the OIDC client.

- prod uses the standard `<name>` OIDC client, preview and staging share `<name>-staging`, and dev uses `<name>-dev`
- prod, staging, and preview all share the `https://oauth.scottylabs.org/oauth2/callback` (Ricochet) relay, while dev uses the `http://localhost:8090/oauth2/callback` (local) relay that requires `scottylabs.ricochet.enable` to be enabled in devenv.

Your service receives the client credentials and Keycloak connection settings as env vars, declared in your `secretspec.toml`:

```toml
[profiles.default]
OIDC_CLIENT_ID = { description = "Keycloak OIDC client ID" }
OIDC_CLIENT_SECRET = { description = "Keycloak OIDC client secret" }
KEYCLOAK_URL = { description = "Keycloak base URL" }
KEYCLOAK_REALM = { description = "Keycloak realm" }
OAUTH_RELAY_URL = { description = "OAuth relay callback URL" }
```

### Sentry

If your service reports errors to Sentry, add `sentry` to its `features` in governance. It creates a Sentry project and writes the project DSN to Vault as `SENTRY_DSN` on the `prod` profile, so declare it there:

```toml
[profiles.prod]
SENTRY_DSN = { description = "Sentry project DSN" }
```

Read `SENTRY_DSN` at startup and pass it to the Sentry SDK to turn on error reporting, following [Sentry's setup guide](https://docs.sentry.io/platforms/). Without a DSN, in local development and previews, the SDK stays inert.

Documentation is controlled separately by `docs` (boolean, default `true`), which aggregates the repository's `./docs` directory into the documentation hub.
