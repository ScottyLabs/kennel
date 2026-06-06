# devenv Options

These options are provided by the [shared ScottyLabs devenv module](https://codeberg.org/ScottyLabs/devenv). Import it in your `devenv.nix`:

```nix
imports = [ inputs.scottylabs.devenvModules.default ];
```

## `scottylabs`

### `scottylabs.enable`

Enable the shared ScottyLabs development configuration. Required for all other `scottylabs.*` options to take effect.

Type: `bool`, default: `false`

### `scottylabs.project.name`

Project name. Used for database naming, log filtering, and secrets path resolution.

Type: `str`, required when `scottylabs.enable = true`

### `scottylabs.conventionalCommits.enable`

Enforce [Conventional Commits](https://www.conventionalcommits.org/) on `git commit` via the commitizen git hook. Commit messages that do not match the conventional format are rejected at commit time.

Type: `bool`, default: `true`

## `scottylabs.cachix`

### `scottylabs.cachix.push`

Push builds to the `scottylabs` cachix cache. Each developer must run this once, from inside any ScottyLabs devenv shell (after `bao login -method=oidc`):

```
cachix authtoken $(bao kv get -field=CACHIX_AUTH_TOKEN secret/shared/cachix)
```

The cache is always pulled when `scottylabs.enable = true`, regardless of this option.

Type: `bool`, default: `true`

## `scottylabs.rust`

### `scottylabs.rust.enable`

Enable the Rust development toolchain. Configures nightly Rust with [cranelift](https://github.com/rust-lang/rustc_codegen_cranelift) (fast debug-mode codegen), clippy, rustfmt, and the [wild](https://github.com/davidlattimore/wild)/lld linker.

Type: `bool`, default: `false`

### `scottylabs.rust.cranelift.excludePackages`

Crate names forced to the LLVM backend instead of cranelift. Some crates use features that cranelift does not support (FFI symbol emission, linker sections).

Type: `listOf str`, default: `[ "aws-lc-sys" "aws-lc-rs" "rustls" ]`

## `scottylabs.deno`

### `scottylabs.deno.enable`

Enable the Deno/JavaScript development toolchain. Adds [Deno](https://deno.com), [oxlint](https://oxc.rs/docs/guide/usage/linter.html) (with `--deny all`), [oxfmt](https://oxc.rs/docs/guide/usage/formatter), and [tsgolint](https://github.com/typescript-eslint/tsgolint) on `PATH` for `oxlint --type-aware`.

Type: `bool`, default: `false`

### `scottylabs.deno.react.enable`

Add the [`react`](https://oxc.rs/docs/guide/usage/linter/plugins#react-plugin) and [`jsx-a11y`](https://oxc.rs/docs/guide/usage/linter/plugins#jsx-a11y-plugin) plugins to oxlint.

Type: `bool`, default: `false`

### `scottylabs.deno.svelte.enable`

Add the [`svelte-check`](https://github.com/sveltejs/language-tools/tree/master/packages/svelte-check) pre-commit hook.

Type: `bool`, default: `false`

## `scottylabs.postgres`

### `scottylabs.postgres.enable`

Enable a local PostgreSQL 18 instance with Unix socket access. Creates an initial database named after `scottylabs.project.name` and exports `DATABASE_URL` into the shell environment.

Type: `bool`, default: `false`

### `scottylabs.postgres.extensions`

PostgreSQL extensions as a function of the extensions set.

Type: `function`, default: `e: [ e.pg_uuidv7 ]`

## `scottylabs.sqlite`

### `scottylabs.sqlite.enable`

Enable SQLite for local development. Adds the `sqlite` package and exports `DATABASE_PATH` pointing to a database file in the devenv state directory.

Type: `bool`, default: `false`

## `scottylabs.valkey`

### `scottylabs.valkey.enable`

Enable a local [Valkey](https://valkey.io/) instance for development. Layers `services.redis.package = pkgs.valkey` under the hood, so the upstream `services.redis` devenv module drives the process while the binary is the wire-compatible Valkey fork. Adds `pkgs.valkey` to the shell so `valkey-cli` is on the path.

Type: `bool`, default: `false`

## `scottylabs.secrets`

### `scottylabs.secrets.enable`

Enable secretspec integration for local secret resolution. Configures devenv's native secretspec support with the ScottyLabs OpenBao server as the provider. Adds the `bao` CLI for authentication.

Type: `bool`, default: `false`

### `scottylabs.secrets.host`

OpenBao server hostname. Used to derive both `BAO_ADDR` (for the bao CLI) and `SECRETSPEC_PROVIDER` (for secretspec).

Type: `str`, default: `"secrets2.scottylabs.org"`

### `scottylabs.secrets.profile`

secretspec profile for local development.

Type: `str`, default: `"dev"`

## `scottylabs.keycloak`

### `scottylabs.keycloak.enable`

Enable a local Keycloak instance for development. Bootstraps the `scottylabs` realm with a confidential OIDC client matching `scottylabs.project.name`. The client secret is read from `[profiles.dev].OIDC_CLIENT_SECRET.default` in the project's `secretspec.toml` so the dev realm and the secretspec contract stay in sync.

Type: `bool`, default: `false`

### `scottylabs.keycloak.port`

HTTP port the local Keycloak listens on (bound to `127.0.0.1`).

Type: `port`, default: `8088`

### `scottylabs.keycloak.devClient.redirectUris`

Permitted redirect URIs for the dev OIDC client.

Type: `listOf str`, default: `[ "http://localhost:*/*" "http://127.0.0.1:*/*" ]`

## `scottylabs.kennel`

### `scottylabs.kennel.services`

Backend services deployed by kennel. Each key must match a devenv process name. Kennel builds the corresponding flake package and deploys it as a systemd transient unit.

Type: `attrsOf submodule`

Each service accepts:

- `customDomain` (`nullOr str`, default: `null`) -- custom domain for this service
- `oidc` (`nullOr submodule`, default: `null`) -- when set, kennel reconciles a Keycloak prod and staging client for the project on every deploy. Accepts:
  - `redirectPaths` (`listOf str`) -- redirect URI paths (e.g. `"/oauth2/callback"`). Hosts are derived from kennel's URL pattern: `https://{slug}-main.scottylabs.net{path}` for prod (plus `customDomain` if set) and `https://{slug}-staging.scottylabs.net{path}` for staging. PR-preview URLs (`{slug}-pr-{N}.scottylabs.net`) are added to the staging client on PR open and removed on PR close

### `scottylabs.kennel.sites`

Static sites deployed by kennel. Each key names a site. Kennel builds the corresponding flake package and serves it via Caddy's file server.

Type: `attrsOf submodule`

Each site accepts:

- `spa` (`bool`, default: `false`) -- serve index.html for all routes
- `customDomain` (`nullOr str`, default: `null`) -- custom domain for this site

### `scottylabs.kennel.config`

Read-only. The generated `kennel.json` derivation that the kennel builder evaluates at build time. You do not set this directly.

Type: `package`

## `scottylabs.claude`

### `scottylabs.claude.enable`

Enable Claude Code integration. Generates the `.mcp.json` configuration with the devenv MCP server.

Type: `bool`, default: `true`
