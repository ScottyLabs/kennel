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

## `scottylabs.garage`

### `scottylabs.garage.enable`

Enable a local [Garage](https://garagehq.deuxfleurs.fr/) S3 instance for development. Creates a bucket named after `scottylabs.project.name` and exports `S3_ENDPOINT`, `S3_REGION`, `S3_ACCESS_KEY`, `S3_SECRET_KEY`, and `S3_BUCKET` into the shell environment.

Type: `bool`, default: `false`

### `scottylabs.garage.accessKey`

S3 access key for the project bucket.

Type: `str`, default: `scottylabs.project.name`

### `scottylabs.garage.secretKey`

S3 secret key for the project bucket.

Type: `str`, default: `"${scottylabs.project.name}admin"`

## `scottylabs.secrets`

When `scottylabs.enable = true`, the `openbao` (`bao`) and `secretspec` CLIs are added to the shell, `BAO_ADDR` is set for OpenBao authentication, and every secret secretspec resolves is exported into the shell environment. Resolution is enabled per project through the `secretspec` block in `devenv.yaml` (see [Secrets](../guides/secrets.md)).

## `scottylabs.ricochet`

### `scottylabs.ricochet.enable`

Run a local OAuth relay on `127.0.0.1:8090` for development, the loopback callback the `dev` Keycloak client is registered against. Enable it alongside `oidc_client` to complete an OAuth flow locally the way production does: the IdP redirects to the relay, which forwards the authorization code on to your service's own callback.

Type: `bool`, default: `false`

## `scottylabs.kennel`

### `scottylabs.kennel.services`

Backend services deployed by kennel. Each key must match a devenv process name. Kennel builds the corresponding flake package and deploys it as a systemd transient unit.

Type: `attrsOf submodule`

Each service accepts:

- `customDomain` (`nullOr str`, default: `null`) -- custom domain for this service

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
