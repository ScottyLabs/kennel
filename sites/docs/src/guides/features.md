# Enabling Features

In the ScottyLabs governance repository, add your project to its team's TOML file and list the features you want in its `features` table. Give a feature an empty table to use its defaults, or set its options as keys.

```toml
[[team.repos]]
name = "collie"

[team.repos.features]
kennel = {}
sentry = {}

[team.repos.features.ai_gateway]
prod_monthly_budget = 20.0
```

- `kennel` provisions the webhook that connects your repository to kennel for builds and deployments
- `sentry` creates a Sentry project and writes its DSN to Vault as `SENTRY_DSN` on the `prod` profile; set `platform` (for example `rust`, `deno`, or `python`) to label the project's SDK
- `posthog` creates a PostHog project and writes its key and host to Vault as `PUBLIC_POSTHOG_KEY` and `PUBLIC_POSTHOG_HOST` on the `prod` profile
- `posthog` with `server_side = true` also parses browser, OS, and device from each event's user agent and keeps client IPs for location, for services that capture events from a server on behalf of visitors
- `cdn` provisions a per-project public-read [Garage](https://garagehq.deuxfleurs.fr/) bucket and writes `CDN_S3_ENDPOINT`, `CDN_S3_BUCKET`, `CDN_ACCESS_KEY_ID`, `CDN_SECRET_ACCESS_KEY`, and `CDN_PUBLIC_URL` to Vault on the `prod` profile
- `oidc_client` provisions prod, staging, and dev Keycloak OIDC clients and writes `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, `KEYCLOAK_URL`, `KEYCLOAK_REALM`, `OAUTH_RELAY_URL`, `PROJECT_GROUP`, and `PROJECT_ADMIN_GROUP` to Vault for each profile
- `oidc_client` with `admin = true` also provisions a service-account client with the `view-users`, `manage-users`, and `view-identity-providers` roles, written to Vault as `KEYCLOAK_ADMIN_CLIENT_ID` and `KEYCLOAK_ADMIN_CLIENT_SECRET`
- `oidc_client` with `groups` also creates extra Keycloak groups under the project group, left empty for you to manage
- `oidc_client` with `public = true` provisions public clients that have no secret, so `OIDC_CLIENT_SECRET` is not written
- `ai_gateway` mints budgeted [LiteLLM](https://docs.litellm.ai) API keys under your team and writes `LITELLM_API_KEY` and `LITELLM_BASE_URL` to Vault on every profile
- `docs` publishes the repository's `docs/` directory to the [documentation hub](https://docs.scottylabs.org)
- `mobile_app` grants your team access to its Play Console app and, if `app_store` is set, to App Store Connect
- `email` sends mail from a project domain, forwards replies to the project's `sl-<project>` group, and writes `EMAIL_FROM` and SMTP credentials to Vault on the `prod` profile

### OIDC

For OIDC authentication, note the distinction between the OAuth relay (`scottylabs.ricochet.enable` in your flake) and the Keycloak OIDC client (`oidc_client` in governance). The OAuth relay is used to relay between the standardized authorized redirect URI used by the OIDC client.

- prod uses the standard `<name>` OIDC client, preview and staging share `<name>-staging`, and dev uses `<name>-dev`
- prod, staging, and preview all share the `https://oauth.scottylabs.org/oauth2/callback` (Ricochet) relay, while dev uses the `http://localhost:8090/oauth2/callback` (local) relay that requires `scottylabs.ricochet.enable` in your flake.

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

Your service builds its own callback URL from `APP_URL` and the relay forwards the authorization code there after login. Set it with [`scottylabs.ricochet.appUrl`](../reference/flake-options.md#scottylabsricochetappurl) in local development; in deployments kennel injects `APP_URL` from the domain (see [runtime environment](./deploying.md#runtime-environment)).

The OIDC client also adds the user's group memberships to the token as a `groups` claim, in full-path form like `/projects/<slug>`. Match it against `PROJECT_GROUP` and `PROJECT_ADMIN_GROUP` to grant enhanced permissions.

### PKCE

Every OIDC client requires [PKCE](https://datatracker.ietf.org/doc/html/rfc7636) with the `S256` method, so Keycloak rejects any authorization request without a `code_challenge`. Most OIDC libraries handle this for you, including `openid-client` and `axum-oidc`, but a hand-built authorization URL has to add `code_challenge` and `code_challenge_method=S256` itself.

### Public Clients

Apps that can't keep a secret, like CLIs, SPAs, and mobile apps, should use a public client:

```toml
[team.repos.features.oidc_client]
public = true
```

Public clients authenticate with PKCE alone, so governance doesn't write `OIDC_CLIENT_SECRET` and you should drop it from your `secretspec.toml`.

### Keycloak Groups

If your app needs roles beyond members and admins, list them as `groups` on `oidc_client`. Governance creates each one under the project group and leaves its membership to you:

```toml
[team.repos.features.oidc_client]
groups = ["staff"]
```

For a project named `quest`, that creates `/projects/quest/staff` as an empty group. Add members in the [Keycloak admin console](https://idp.scottylabs.org/admin/master/console/#/scottylabs/groups) afterwards.

These groups appear in the OIDC `groups` claim like any other, so the app can authorize against `<PROJECT_GROUP>/staff`. Unlike the project's own groups, they don't give members access to any secrets in OpenBao.

### Sentry

If your service reports errors to Sentry, add `sentry` to its `features` in governance. It creates a Sentry project and writes the project DSN to Vault as `SENTRY_DSN` on the `prod` profile, so declare it there:

```toml
[profiles.prod]
SENTRY_DSN = { description = "Sentry project DSN" }
```

Read `SENTRY_DSN` at startup and pass it to the Sentry SDK to turn on error reporting, following [Sentry's setup guide](https://docs.sentry.io/platforms/). Without a DSN, in local development and previews, the SDK stays inert.

Set `platform` in the feature to label the project's SDK in Sentry (for example `rust`, `deno`, `python`, or `other` when Sentry has no matching SDK).

### PostHog

If your service sends product analytics to PostHog, add `posthog` to its `features` in governance. It creates a PostHog project and writes the project key and host to Vault as `PUBLIC_POSTHOG_KEY` and `PUBLIC_POSTHOG_HOST` on the `prod` profile, so declare them there:

```toml
[profiles.prod]
PUBLIC_POSTHOG_KEY = { description = "PostHog project API key" }
PUBLIC_POSTHOG_HOST = { description = "PostHog ingestion host" }
```

Read them at startup and pass them to the PostHog SDK to turn on analytics. Without a key, in local development and previews, the SDK stays inert.

`PUBLIC_POSTHOG_HOST` is `https://v.scottylabs.org`, PostHog's managed reverse proxy on a ScottyLabs domain, so ad blockers that filter PostHog's own domains don't drop events. Pass it as the SDK's host (`api_host` in `posthog-js`). A browser frontend reads both at runtime, so also list them in the [public scope](./deploying.md#public-values):

```toml
[scopes.public]
secrets = ["PUBLIC_POSTHOG_KEY", "PUBLIC_POSTHOG_HOST"]
```

In `posthog-js`, also set `ui_host` to `https://us.posthog.com`. Without it, the PostHog toolbar won't work on your site.

If your service captures events from a server on behalf of visitors (for example a redirect service sending one event per click), set `server_side`:

```toml
[team.repos.features.posthog]
server_side = true
```

This makes PostHog derive browser, OS, and device from the visitor's user agent instead of the server SDK's own values, and keeps client IPs for location and bot detection. On each event, set `$raw_user_agent` to the visitor's `User-Agent` header and `$ip` to their `CF-Connecting-IP` header.

It defaults to `false`, and only affects events from server SDKs, so `posthog-js` events in the same project are unchanged.

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

### Email

If your service sends mail, add `email` with the address it sends from and the [Google Group](#google-groups) that receives replies. The address must be on a domain the project owns in Cloudflare, and the group must be one of the project's `contact` groups:

```toml
[team.projects.repos.features.email]
from = "info@<domain>"
inbox = "sl-<project>@andrew.cmu.edu"
```

Declare the address and SMTP credentials on the `prod` profile:

```toml
[profiles.prod]
EMAIL_FROM = { description = "Sending address" }
SMTP_HOST = { description = "SMTP server" }
SMTP_PORT = { description = "SMTP port, using implicit TLS" }
SMTP_USERNAME = { description = "SMTP username" }
SMTP_PASSWORD = { description = "SMTP password" }
```

Someone in the group must confirm Cloudflare's verification email before replies are forwarded.

### AI Gateway

If your service calls language models, add `ai_gateway` to its `features` in governance. It mints two LiteLLM virtual keys under your team: one for prod, and a lower-budget key shared by staging, preview, and dev. Budgets are monthly, in USD, and configurable as feature settings:

```toml
[team.repos.features.ai_gateway]
prod_monthly_budget = 20.0 # default
dev_monthly_budget = 5.0   # default
```

The key and the gateway URL are written to Vault on every profile, so declare them in `default`:

```toml
[profiles.default]
LITELLM_API_KEY = { description = "LiteLLM virtual key" }
LITELLM_BASE_URL = { description = "LiteLLM gateway base URL" }
```

Point any OpenAI-compatible client at `LITELLM_BASE_URL` with `LITELLM_API_KEY` as the API key; the gateway routes to the models it has configured and meters spend against your key. Keys are grouped under your team in LiteLLM for spend attribution.

### Mobile Apps

Store apps cannot be created through any API, so create the app in the [Play Console](https://play.google.com/console) first, then add `mobile_app` with its package name. If the iOS app is published under the ScottyLabs Apple developer account, create it in [App Store Connect](https://appstoreconnect.apple.com) and add its bundle ID as well:

```toml
[team.repos.features.mobile_app]
google_play = "org.scottylabs.example"
app_store = "org.scottylabs.example"
```

Leave out `app_store` when the iOS app lives under another Apple developer account, as Quest's does under CMU's.

On Google Play, governance grants your team access to the app:

- members release to testing tracks, edit tester lists, and read crash and vitals data
- leads additionally release to production, manage the store listing and policy pages, and reply to reviews
- since the provider rejects users with no account-level permissions, everyone holds account-wide read on non-financial data

On the App Store, governance invites everyone on the team at the email of the Apple Account they linked in Keycloak, so each member must link Apple with Share My Email before the change can merge:

- members build, upload, and run internal TestFlight testing
- leads additionally submit releases, manage the store listing and pricing, run external TestFlight, and reply to reviews
- everyone can view analytics and sign builds, and since Apple shows every app to anyone who can sign, access isn't limited to your team's app
- service keys such as MapKit and push notification keys are created by the tech director

## Google Groups

A project can list CMU Google Groups for governance to keep in sync with its leads and members. Each group has a `kind` that decides who can post to it and who manages its members:

```toml
[[team.projects.google_groups]]
email = "sl-<project>@andrew.cmu.edu"
kind = "contact"
```

| Kind | Managers | Who can post | People outside the project |
| --- | --- | --- | --- |
| `contact` | tech director | Anyone on the web | never |
| `discussion` | tech director, leads, and members | Anyone on the web | added by managers |
| `announce` | tech director, leads, and members | Group managers | added by managers |

Every kind has the project's leads and members as members, including the parent team's. A project's `sl-<project>` group is its inbox and should be a `contact` group.

When you add a group, set its "Who can post" in [Google Groups](https://groups.google.com) to match its kind. The group must already exist with `scottylabs-svc@andrew.cmu.edu` as one of its managers.

## Documentation

Add `docs` to publish the repository's `docs/` directory to the [documentation hub](https://docs.scottylabs.org).
