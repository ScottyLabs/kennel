# Deploying a Project

This guide walks through setting up a ScottyLabs project for deployment with kennel.

## Prerequisites

- A project registed in governance with an associated Codeberg repository
- [devenv](https://devenv.sh) and [direnv](https://direnv.net/) installed locally

## 1. Import the shared module

Add the ScottyLabs devenv input to your `devenv.yaml`:

```yaml
secretspec:
  enable: true
  profile: dev

inputs:
  nixpkgs:
    url: github:nixos/nixpkgs/nixos-unstable
  scottylabs:
    url: git+https://codeberg.org/ScottyLabs/devenv
    inputs:
      nixpkgs:
        follows: nixpkgs
  rust-overlay:
    url: github:oxalica/rust-overlay
    inputs:
      nixpkgs:
        follows: nixpkgs
  treefmt-nix:
    url: github:numtide/treefmt-nix
  git-hooks:
    url: github:cachix/git-hooks.nix
    inputs:
      nixpkgs:
        follows: nixpkgs
```

This `nixpkgs` pin must match the one in `flake.nix` below, so the shell and the build resolve identical packages.

Import it in your `devenv.nix`:

```nix
{ pkgs, config, inputs, ... }:
{
  imports = [ inputs.scottylabs.devenvModules.default ];

  scottylabs = {
    enable = true;
    project.name = "my-project";
  };
}
```

The `secretspec` block resolves your project's secrets from OpenBao into the shell. See the [Secrets](./secrets.md) guide to declare and manage them.

## 2. Set up direnv and .gitignore

Create an `.envrc` to automatically activate the devenv environment when you enter the project directory:

```bash
eval "$(devenv direnvrc)"
use devenv
```

Then allow it:

```bash
direnv allow
```

If this command fails with a secret-related error, make sure you have run the one-time [OpenBao setup](./secrets.md).

Add a `.gitignore` for generated and local-only files:

```gitignore
# Nix / devenv
.devenv/
.devenv.flake.nix
.pre-commit-config.yaml
result
result-*

# AI
.mcp.json
.claude

# direnv
.direnv/

# Rust
target/
.cargo/

# Secrets
.env

# OS
.DS_Store
rustc-ice-*.txt
```

Add any project-specific entries as needed (e.g., `sites/docs/book/` for mdbook output, `node_modules/` for JS projects).

## 3. Declare what to deploy

Add kennel options to your `devenv.nix` to tell kennel what your project produces.

For a backend service:

```nix
scottylabs.kennel.services.api = {
  customDomain = "api.my-project.scottylabs.org";
};
```

In production kennel runs the flake package, so the `scottylabs.kennel.services` key and the flake package must share the same name (here, `api`).

For a static site:

```nix
scottylabs.kennel.sites.app = {
  spa = true;
};
```

Set `spa = true` for single-page apps so Caddy serves `index.html` for unmatched routes. It defaults to `false`, which serves files directly, suitable for pre-rendered sites like mdbook docs.

Note that custom domains that are not already in use must first have their Cloudflare Zone IDs registered with kennel in the [infrastructure repository](https://codeberg.org/scottylabs/infrastructure). Kennel creates the DNS record itself for registered zones, and certificate issuance begins only once the domain resolves to kennel, so a domain whose zone is missing serves no TLS until the zone is registered.

### Health checks

Every backend service must expose a `GET /api/health` endpoint that returns HTTP `200` once it is ready to accept traffic. After starting a service, kennel polls this endpoint every 2 seconds for up to 60 seconds, holding back traffic until it returns `200`. If it has not passed by then, the deployment is marked failed and the public domain is never routed to it. Static sites have no health endpoint and are exempt.

### Flake packages

Your `flake.nix` must expose these packages, and their names must match the keys in `scottylabs.kennel.services` and `scottylabs.kennel.sites`. Build them with the shared [build helpers](../reference/build-helpers.md):

```nix
inputs = {
  nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  scottylabs = {
    url = "git+https://codeberg.org/ScottyLabs/devenv";
    inputs.nixpkgs.follows = "nixpkgs";
  };
};

outputs = { nixpkgs, scottylabs, ... }:
  let
    forAllSystems = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ];
  in {
    packages = forAllSystems (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        helpers = scottylabs.mkLib pkgs;
      in {
        api = helpers.buildRustService {
          src = ./.;
          pname = "api";
          buildArgs.cargoExtraArgs = "-p api";
        };
        docs = helpers.buildMdbook { src = ./sites/docs; };
      }
    );
  };
```

This must be the same `nixpkgs` pinned in `devenv.yaml`, so `nix build` and `devenv shell` resolve identical packages.

A Deno or JavaScript front-end uses `buildDenoTask` the same way.

A browser front-end (Vite, Svelte) also needs a root `deno.json` that excludes its directory, so the `deno check` hook skips it instead of failing on browser code. Replace `sites/web` with your front-end's path:

```json
{
  "exclude": ["sites/web"]
}
```

For a SvelteKit app, `svelte-check` type-checks it (enable `scottylabs.deno.svelte.enable`).

Kennel builds each package with `nix build .#packages.{system}.{name}`.

### Runtime environment

Kennel injects these variables into every backend service it deploys:

- `PORT`: the port your service must bind to. Kennel allocates it and routes the public domain to it through Caddy, so read it at startup instead of hardcoding a port.
- `COMMIT_HASH`: the full Git commit SHA of the running build.
- `APP_URL`: the public URL of this deployment, its custom domain when configured or the generated domain otherwise.

Resolved secrets from your `secretspec.toml` are injected alongside these.

Kennel runs each service from a working directory that contains your `secretspec.toml`, so the [secretspec SDK's runtime `load()`](./secrets.md#runtime-loading) finds it automatically. The filesystem is read-only apart from a private `/tmp`, so keep persistent state in a provisioned database or object store.

## 4. Enable infrastructure

If your project needs a database:

```nix
scottylabs.postgres.enable = true;
```

This gives you a local PostgreSQL instance in development and a provisioned per-deployment database in production. Your app reads `DATABASE_URL` from the environment in both cases.

`sqlite`, `garage` (S3-compatible object storage), and `valkey` (Redis-compatible key-value store) are also available and documented in the [devenv options](../reference/devenv-options.md). Aside from `garage`, these databases are configured with unix socket auth rather than TCP/password auth.

Kennel provisions a resource only when the project enables it, so a project that does not declare `valkey` (or `postgres`, or `garage`) is never allocated one. Provisioning also depends on keeping the shared devenv module current. Kennel stamps each `kennel.json` with a schema version and refuses any deployment whose version does not match, so run `devenv update` if a build is rejected for a version mismatch.

When developing the OAuth flow locally, enable the relay so the IdP redirects back through it the way it does in production:

```nix
scottylabs.ricochet.enable = true;
scottylabs.ricochet.appUrl = "http://localhost:3000";
```

Governance already seeds the correct `OAUTH_RELAY_URL` for `profiles.dev`, so `enable` only runs Ricochet locally. `appUrl` is your service's local URL, the port your dev server listens on, exported as `APP_URL` so the relay can return to it. Set it only for development: deployed environments receive `APP_URL` from kennel automatically (see [Runtime environment](#runtime-environment)), and it is never declared in `secretspec.toml`.

## 5. Development scripts

`scripts.<name>.exec` defines a command available in your shell whenever the devenv environment is active. Use it for project-specific development tasks such as database migrations or code generation:

```nix
scripts = {
  migrate.exec = "sea-orm-cli migrate up -d crates/migration";
  generate-api.exec = "cd app && deno task generate-api";
};
```

Running `migrate` in the shell then executes that command. Scripts are a development convenience only. Kennel never runs them, in builds or deployments.

## 6. Push

Kennel deploys three long-lived branches (`main`, `staging`, or `dev`). To get a preview, open a pull request and kennel builds and deploys it. Pushing any other branch is ignored.

Your deployment will be available at `{project}-{service}-{branch}.scottylabs.net`, where `service` is the key from `scottylabs.kennel.services`:

- `my-project-api-main.scottylabs.net` for the `api` service on the main branch
- `my-project-api-pr-42.scottylabs.net` for PR #42

As it works, kennel reports progress back to Forgejo as commit statuses on the pushed commit. The `kennel/build` status moves from pending to success or failure as the build runs, and `kennel/deploy` reports success once the deployment passes its health check. A failed status means the commit never reached a healthy deployment; check the build log to see why.

### Disabling preview deployments

Some services should never run more than one instance at a time. A Discord bot, for example, would respond twice if a preview ran alongside production. Disable preview deployments for the whole project:

```nix
scottylabs.kennel.previewDeployments = false;
```

It defaults to `true`. When disabled, opening or updating a pull request still builds and reports `kennel/build`, but kennel does not deploy the preview.
