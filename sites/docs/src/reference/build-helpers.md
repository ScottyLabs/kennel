# Build Helpers

These builders come from the [shared ScottyLabs flake](https://git.cmu.dev/ScottyLabs/kennel) as `mkLib`, a function applied to a `pkgs` set (the same shape as `crane.mkLib`). Add the flake as an input, then call a helper per system:

```nix
# flake.nix inputs
scottylabs = {
  url = "git+https://git.cmu.dev/ScottyLabs/kennel";
  inputs.nixpkgs.follows = "nixpkgs";
};

# in outputs
pkgs = nixpkgs.legacyPackages.${system};
kennel = (scottylabs.mkLib pkgs).buildRustService { ... };
```

Each helper takes the consumer's `pkgs`, so packages build against your nixpkgs pin rather than the shared flake's.

Under kennel, every helper exports the project's [build environment](../guides/deploying.md#build-environment) into the build of the project's own package, leaving dependency builds and their cache untouched. The values are also available as `(scottylabs.mkLib pkgs).buildEnv` for a derivation built without the helpers, and are empty outside kennel. `buildRustService` merges `buildArgs.env` over them.

## `buildRustService`

Builds a Rust crate with [crane](https://crane.dev), caching dependencies separately from the build.

### `src`

Crate or workspace root. Passed through `craneLib.cleanCargoSource`, so only Cargo-relevant files enter the build.

Type: `path`, required

### `pname`

Package name. When unset, it is read from the `pname` field of `Cargo.toml`. A workspace has a virtual manifest with no package name, so a workspace must set this explicitly.

Type: `nullOr str`, default: `null`

### `version`

Package version. When unset, it is read from the `version` field of `Cargo.toml`.

Type: `nullOr str`, default: `null`

### `paths`

Scopes `src` to just these workspace-member paths plus `Cargo.toml`/`Cargo.lock`, so changes elsewhere in the workspace don't invalidate the build.

Type: `nullOr (listOf str)`, default: `null`

### `nativeBuildInputs`

Build-time tools, applied to both the dependency build and the final build. For example `pkg-config` or `makeWrapper`.

Type: `listOf package`, default: `[ ]`

### `buildInputs`

Libraries to link against, applied to both the dependency build and the final build. For example `openssl`.

Type: `listOf package`, default: `[ ]`

### `buildArgs`

Build-only options forwarded to crane's `buildPackage`, kept separate from the arguments above so changing them never rebuilds the cached dependencies. `doCheck` defaults to `false`. Common keys are `cargoExtraArgs` to select a workspace member such as `"-p kennel"`, and `postInstall` to wrap the binary.

Type: `attrs`, default: `{ }`

```nix
(scottylabs.mkLib pkgs).buildRustService {
  src = ./.;
  pname = "kennel";
  version = "0.1.0";
  buildArgs.cargoExtraArgs = "-p kennel";
}
```

## `buildHaskellService`

Builds a Haskell project with GHC 9.14 and strips it to its static executables. Dependencies come from the nixpkgs Haskell package set rather than Hackage, so version bounds must accept the versions nixpkgs has. Use `overrides` for exceptions.

### `pname`

Package name to build. Must be a key of `localPackages`; its executables become the service binaries.

Type: `str`, required

### `localPackages`

The project's cabal packages, package name to source directory. Every package builds inside the same extended set, so intra-project dependencies resolve to each other. List the same directories as `scottylabs.haskell.localPackages` in `devenv.nix`, so the shell and the build resolve identical dependencies.

Type: `attrsOf path`, required

### `overrides`

A final overlay over the package set, to jailbreak or pin a dependency the set's version cannot satisfy (for example `final: prev: { some-dep = pkgs.haskell.lib.doJailbreak prev.some-dep; }`).

Type: `function`, default: `_final: _prev: { }`

```nix
(scottylabs.mkLib pkgs).buildHaskellService {
  pname = "reviewd";
  localPackages = {
    kernel-effects = ./kernel-effects;
    reviewd = ./reviewd;
  };
}
```

## `buildDenoTask`

Builds a Deno project, or a single member of a Deno workspace, that has npm dependencies, running a `deno task` and copying its output directory. On Linux it patches the prebuilt native addons and drops the musl variants, which cannot be patched against a glibc host.

### `src`

Directory holding `deno.lock` and `deno.json`. In a Deno workspace this is the workspace root that holds the single `deno.lock`, and `cwd` selects the member to build.

Type: `path`, required

### `cwd`

Subdirectory of `src` to build in, for a Deno workspace member. `"."` builds `src` itself. When set, `src` is the workspace root holding the shared `deno.lock`, and the task runs from `src/<cwd>`, so `output` and `entrypoint` resolve relative to the member.

Type: `str`, default: `"."`

### `pname`

Package name.

Type: `str`, required

### `version`

Package version.

Type: `str`, default: `"0.1.0"`

### `task`

The `deno task` to run.

Type: `str`, default: `"build"`

### `output`

Directory the task emits, copied verbatim to the result.

Type: `str`, default: `"dist"`

### `entrypoint`

Passed to `deno install --entrypoint` so dependency resolution ignores dependencies not reachable from it, such as test-only dependencies.

Type: `nullOr str`, default: `null`

### `compile`

Provision [denort](https://docs.deno.com/runtime/reference/cli/compile/) so `deno compile` runs inside the offline build sandbox. Enable when the task compiles a binary instead of emitting a `dist` directory.

Type: `bool`, default: `false`

```nix
(scottylabs.mkLib pkgs).buildDenoTask {
  src = ./sites/web;
  pname = "link-shortener-web";
  version = "0.1.0";
}
```

A workspace member builds from the workspace root that holds the shared `deno.lock`, with `cwd` selecting the member:

```nix
(scottylabs.mkLib pkgs).buildDenoTask {
  src = ./.;
  cwd = "apps/backend";
  pname = "housing-backend";
  entrypoint = "src/index.ts";
  compile = true;
}
```

## `buildPnpmTask`

Builds a [pnpm](https://pnpm.io) project, or a single member of a pnpm workspace, running a `package.json` script with Node.js. Dependencies come from `pnpm-lock.yaml` through nixpkgs' `fetchPnpmDeps`, so the build needs the hash of the fetched store. Install scripts do not run, so code generation such as `prisma generate` belongs in `script`. On Linux it patches the prebuilt native addons and drops the musl variants, which cannot be patched against a glibc host.

By default the result is the script's output directory, for a static site. Set `main` to build a backend service instead. See [Node.js with pnpm](../guides/deploying.md#nodejs-with-pnpm) for the shell side and how the build differs from it.

### `src`

Directory holding `pnpm-lock.yaml`. In a pnpm workspace this is the workspace root, and `cwd` selects the member to build.

Type: `path`, required

### `cwd`

Subdirectory of `src` to build in, for a workspace member. `"."` builds `src` itself. The script runs from `src/<cwd>`, so `output` and `main` resolve relative to the member.

Type: `str`, default: `"."`

### `pname`

Package name. With `main` set, it is also the name of the executable under `bin/`.

Type: `str`, required

### `version`

Package version.

Type: `str`, default: `"0.1.0"`

### `hash`

Hash of the dependencies fetched from `pnpm-lock.yaml`. Set it to `""`, build once, and copy the `got:` hash from the mismatch error. Update it the same way whenever the lockfile changes; until then the build fails with that mismatch. Packages built from the same `src` share one hash.

Type: `str`, required

### `script`

The `package.json` script to run with `pnpm run`.

Type: `str`, default: `"build"`

### `output`

Directory the script emits, copied verbatim to the result. Ignored when `main` is set.

Type: `str`, default: `"dist"`

### `main`

Script, relative to `cwd`, that the service runs with Node.js. When set, the result holds the built workspace with its `node_modules` under `lib/` and a `bin/<pname>` wrapper that runs it, which is what kennel starts for a service. The wrapper keeps the caller's working directory, so the service still finds `secretspec.toml`. The whole workspace ships, development dependencies included.

A TypeScript `main` runs directly only if it uses erasable syntax alone ([type stripping](https://nodejs.org/api/typescript.html#type-stripping)); otherwise compile it in `script` and point `main` at the output.

Type: `nullOr str`, default: `null`

### `nodejs`

Node.js package the script and the service run with. Match `scottylabs.node.package`.

Type: `package`, default: `pkgs.nodejs_24`

### `pnpm`

pnpm package that fetches and installs the dependencies. Match `scottylabs.node.pnpm`, which must agree with any `packageManager` field in `package.json`.

Type: `package`, default: `pkgs.pnpm`

```nix
(scottylabs.mkLib pkgs).buildPnpmTask {
  src = ./.;
  cwd = "apps/web";
  pname = "web";
  hash = "sha256-...";
}
```

A backend service in the same workspace sets `main` to its built entrypoint:

```nix
(scottylabs.mkLib pkgs).buildPnpmTask {
  src = ./.;
  cwd = "apps/server";
  pname = "server";
  hash = "sha256-...";
  main = "dist/index.js";
}
```

## `buildPythonService`

Builds a [uv](https://docs.astral.sh/uv/) project into a virtual environment with [uv2nix](https://github.com/pyproject-nix/uv2nix), resolving every dependency from `uv.lock` as a Nix derivation. The result is a relocatable venv exposing each `[project.scripts]` entry under `bin/`, consumed as `${pkg}/bin/<script>`.

### `src`

Project root holding `pyproject.toml` and `uv.lock`. The package name is read from `pyproject.toml`.

Type: `path`, required

### `python`

The Python interpreter the environment is built against.

Type: `package`, default: `pkgs.python3`

### `sourcePreference`

Whether dependencies come from prebuilt wheels (`"wheel"`) or are built from source (`"sdist"`). The build-system overlay is matched to this choice automatically.

Type: `str`, default: `"wheel"`

### `selectDeps`

Selects the locked dependency closure to install from the loaded workspace. Override to install an optional group, for example `ws: ws.deps.optionals.prod`.

Type: `function`, default: `ws: ws.deps.default`

### `overrides`

An overlay applied last over the Python package set, to supply system libraries or build tools uv2nix cannot infer for a package built from source (for example adding `pkgs.openssl` to `buildInputs`).

Type: `function`, default: `_final: _prev: { }`

```nix
(scottylabs.mkLib pkgs).buildPythonService {
  src = ./.;
}
```

## `buildMdbook`

Builds an [mdBook](https://rust-lang.github.io/mdBook/) site to the result.

### `src`

Directory holding `book.toml`.

Type: `path`, required

### `name`

Derivation name.

Type: `str`, default: `"docs"`

```nix
(scottylabs.mkLib pkgs).buildMdbook {
  src = ./sites/docs;
  name = "kennel-docs";
}
```

## `buildZensical`

Builds a [Zensical](https://zensical.org) site to the result. The site is built to Zensical's default `site_dir`, so the config must not override it.

### `src`

Directory holding `zensical.toml` and the docs it points to.

Type: `path`, required

### `name`

Derivation name.

Type: `str`, default: `"docs"`

```nix
(scottylabs.mkLib pkgs).buildZensical {
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./zensical.toml
      ./docs
    ];
  };
  name = "my-project-docs";
}
```

## `buildOptionsDoc`

Renders a module set's option declarations to a CommonMark reference with nixpkgs' `nixosOptionsDoc`, producing a single markdown file listing every option with its description, type, default, and a link to the declaring module. Both options reference pages in these docs are generated with it. Only declarations are evaluated, never the merged config, so an option whose `default` reads other config must set `defaultText`. Every documented option must have a `description`, or the build fails.

### `module`

Module (or import path) declaring the options.

Type: `module`, required

### `subtree`

Selects the options to document from the evaluated options tree.

Type: `function`, required

### `root`

Repo root. Declaration paths under it become source links relative to it.

Type: `path`, required

### `repoUrl`

Source browser base URL the declaration links point to.

Type: `str`, required

```nix
(scottylabs.mkLib pkgs).buildOptionsDoc {
  module = ./nixos;
  subtree = options: options.services.kennel;
  root = ./.;
  repoUrl = "https://git.cmu.dev/ScottyLabs/kennel/src/branch/main";
}
```
