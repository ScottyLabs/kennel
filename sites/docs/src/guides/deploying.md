# Deploying a Project

This guide walks through setting up a ScottyLabs project for deployment with kennel.

## Prerequisites

- A project registered in governance with an associated Forgejo repository
- [Nix](https://nixos.org) with flakes enabled

## 1. Write the flake

Kennel's flake exports a [flake-parts](https://flake.parts) module that sets up the dev shell, local services, git hooks, and the outputs kennel deploys. Your `flake.nix` imports it and configures it under `scottylabs`:

```nix
{
  nixConfig = {
    extra-substituters = [ "https://scottylabs.cachix.org" ];
    extra-trusted-public-keys = [
      "scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
    ];
  };

  inputs = {
    scottylabs.url = "git+https://git.cmu.dev/ScottyLabs/kennel";
    nixpkgs.follows = "scottylabs/nixpkgs";
    flake-parts.follows = "scottylabs/flake-parts";
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ inputs.scottylabs.flakeModules.default ];
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];

      perSystem =
        { pkgs, helpers, ... }:
        {
          scottylabs.project.name = "my-project";
        };
    };
}
```

Set `project.name` to your governance slug, the same value as `[project] name` in `secretspec.toml`.

Follow kennel's nixpkgs so that your builds can reuse the outputs other projects have already pushed to the binary cache. Renovate updates every flake input by opening a `nix flake update` pull request each week and after every push to kennel. To update sooner, run `nix flake update scottylabs` and commit the new `flake.lock`.

Load the shell with `nix develop`, which resolves your secrets from OpenBao on entry. If that fails, run the one-time [OpenBao setup](./secrets.md).

The shell keeps its state (build caches, local databases) in `.scottylabs/` and links generated files such as `.pre-commit-config.yaml` into the checkout, adding each one to `.git/info/exclude`. Add a `.gitignore` for anything else local:

```gitignore
# Rust
target/
rustc-ice-*.txt

# Secrets
.env

# OS
.DS_Store
```

## 2. Declare what to deploy

Kennel deploys two flake outputs:

- `services`: backend services, run as systemd units
- `sites`: static sites, served by Caddy

Build each target's package with the shared [build helpers](../reference/build-helpers.md), which every `perSystem` module receives as `helpers`:

```nix
perSystem =
  { pkgs, helpers, ... }:
  {
    scottylabs.project.name = "my-project";

    services.api = {
      package = helpers.buildRustService {
        src = ./.;
        pname = "api";
        buildArgs.cargoExtraArgs = "--locked -p api";
      };
      customDomain = "api.my-project.scottylabs.org";
    };

    sites.app = {
      package = helpers.buildDenoTask {
        src = ./sites/app;
        pname = "my-project-app";
        output = "build";
      };
      spa = true;
    };
  };
```

The attribute name is the target's name, so this deploys an `api` service and an `app` site. You can use `nix build .#services.x86_64-linux.api.package` to build one locally.

A service unit runs the package's `meta.mainProgram`, which `buildRustService` and `buildHaskellService` set to `pname`. Without one, kennel runs the first executable in `bin/`.

Besides `package`, a target takes:

- `customDomain`: a domain the target is also served on in production
- `spa`: for sites, serve `index.html` for unmatched routes (single-page apps). It defaults to `false`, which serves files directly, suitable for pre-rendered sites like mdbook docs.
- `env`: for services, extra variables in the service's environment (see [Runtime environment](#runtime-environment))

Custom domains that are not already in use must first have their Cloudflare Zone IDs registered with kennel in the [infrastructure repository](https://git.cmu.dev/ScottyLabs/infrastructure). Kennel creates the DNS record itself for registered zones, so a domain whose zone is missing does not resolve until the zone is registered.

If `customDomain` is an apex domain like `example.org`, kennel also redirects `www.example.org` to it.

### Health checks

Every backend service must expose a `GET /api/health` endpoint that returns HTTP `200` once it is ready to accept traffic. After starting a service, kennel polls this endpoint every 2 seconds for up to 60 seconds, holding back traffic until it returns `200`. If it has not passed by then, the deployment is marked failed and the public domain is never routed to it. Static sites have no health endpoint and are exempt.

### Frontends

Enable Deno and list each browser frontend's directory (Vite, Svelte) so that the shell installs its npm packages:

```nix
scottylabs.deno = {
  enable = true;
  dirs = [ "sites/app" ];
};
```

It also needs a root `deno.json` that excludes that directory, so the `deno check` hook skips it instead of failing on browser code, and `.scottylabs`, where those packages are cached:

```json
{
  "exclude": ["sites/app", ".scottylabs"]
}
```

For a SvelteKit app, `svelte-check` type-checks it (enable `scottylabs.deno.svelte.enable`).

If a service serves a frontend, give the service the frontend's store path through `env`. That way, changing the frontend doesn't rebuild the service:

```nix
services.api =
  let
    web = helpers.buildDenoTask {
      src = ./web;
      pname = "api-web";
    };
  in
  {
    package = helpers.buildRustService { src = ./.; pname = "api"; };
    env.STATIC_DIR = "${web}";
  };
```

### Runtime environment

Kennel injects these variables into every backend service it deploys:

- `PORT`: the port your service must bind to. Kennel allocates it and routes the public domain to it through Caddy, so read it at startup instead of hardcoding a port.
- `COMMIT_HASH`: the full Git commit SHA of the running deployment.
- `APP_URL`: the public URL of this deployment, its custom domain when configured or the generated domain otherwise.

Resolved secrets from your `secretspec.toml` are injected alongside these.

A service's `env` adds variables of your own, and secrets and resource variables take precedence over it when both set the same name.

Kennel runs each service from a working directory that contains your `secretspec.toml`, so the [secretspec SDK's runtime `load()`](./secrets.md#runtime-loading) finds it automatically. The filesystem is read-only apart from a private `/tmp`, so keep persistent state in a provisioned database or object store.

### Public values

A browser frontend often needs values that change per environment, such as an API URL or an analytics key. Declare them in `secretspec.toml` like any other secret, with names starting with `PUBLIC_`, and list them in a `public` scope:

```toml
[profiles.default]
PUBLIC_API_URL = { description = "API base URL", default = "http://localhost:3000" }

[profiles.prod]
PUBLIC_API_URL = { description = "API base URL", default = "https://api.my-project.scottylabs.org" }
PUBLIC_POSTHOG_KEY = { description = "PostHog project key" }

[scopes.public]
secrets = ["PUBLIC_API_URL", "PUBLIC_POSTHOG_KEY"]
```

When kennel deploys a site, it resolves these values for the branch's profile and serves them next to the site's files:

- `/_app/env.js`, which SvelteKit loads for `$env/dynamic/public`
- `/_kennel/env.json`, a JSON object other frameworks can fetch on load

Locally, SvelteKit's dev server reads them from the dev shell's environment. For another framework, commit a development copy at `public/_kennel/env.json`, which kennel's file replaces once deployed.

Anyone can read these files, so only list values that are safe to publish. If a frontend needs something secret, such as a signing key, keep the key in a backend as a runtime secret and have the frontend ask the backend to sign.

A site builds once for every environment, so promoting a commit from `dev` to `staging` or `main` reuses its build from the binary cache. Builds depend on the source tree and not the commit, which means a merge or squash commit with an already built tree reuses them as well. Keep commit metadata such as `self.rev` out of builds, since it would give every commit its own build. Services read the commit from `COMMIT_HASH` at runtime instead.

## 3. Enable infrastructure

If your project needs a database:

```nix
scottylabs.postgres.enable = true;
```

This gives you a local PostgreSQL instance in development and a provisioned per-deployment database in production. Your app reads `DATABASE_URL` from the environment in both cases.

List the PostgreSQL extensions your app uses:

```nix
scottylabs.postgres.extensions = e: [
  e.pg_uuidv7
  e.pgvector
];
```

Kennel creates the same extensions in your deployed database, defaulting to `e: [ e.pg_uuidv7 ]`. It can only create extensions that the host's NixOS configuration installs as trusted, so a new one has to be added there first (see [PostgreSQL extensions](../reference/nixos-module.md#postgresql-extensions)).

`sqlite`, `garage` (S3-compatible object storage), and `valkey` (Redis-compatible key-value store) are also available and documented in the [flake options](../reference/flake-options.md). Aside from `garage`, these databases are configured with unix socket auth rather than TCP/password auth. With `sqlite`, each deployed service reads `DATABASE_PATH`, a file in a private directory that persists across restarts and redeploys. Services do not share it, so two services on one branch get separate files.

Kennel provisions a resource only when the project enables it, so a project that does not declare `valkey` (or `postgres`, `sqlite`, or `garage`) is never allocated one.

To run these services locally, see [Local Development](./local-development.md).

## 4. Push

Kennel deploys three long-lived branches (`main`, `staging`, and `dev`). To get a preview, open a pull request. Pushing any other branch is ignored.

On every push, [nixbot](https://nixbot.scottylabs.org) evaluates the flake once, builds its `checks` (which include every service and site) on the build farm, and pushes the results to the binary cache. Kennel then fetches the manifest from the cache and deploys it with the branch's profile.

Your deployment will be available at `{project}-{target}-{branch}.scottylabs.net`:

- `my-project-api-main.scottylabs.net` for the `api` service on the main branch
- `my-project-api-pr-42.scottylabs.net` for PR #42

Progress shows up as commit statuses on the pushed commit. nixbot reports one status per built attribute, with a link to the build log, and `kennel/deploy` reports success once the deployment passes its health check.

### Disabling preview deployments

If a second running copy would cause trouble, as with a Discord bot that would answer every message twice while a preview runs next to production, turn off preview deployments, which are on by default:

```nix
scottylabs.previewDeployments = false;
```

With previews off, nixbot still builds pull requests, but kennel doesn't deploy them.
