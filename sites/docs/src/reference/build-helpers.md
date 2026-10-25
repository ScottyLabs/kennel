# Build Helpers

The [shared flake module](./flake-options.md) passes these builders to every `perSystem` module as `helpers`:

```nix
perSystem =
  { helpers, ... }:
  {
    services.api.package = helpers.buildRustService { ... };
  };
```

Helpers build with your flake's `pkgs`, which comes from kennel's nixpkgs.

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

Options forwarded to crane's `buildPackage`. `doCheck` defaults to `false` and `meta.mainProgram` to `pname`. `cargoExtraArgs` defaults to `"--locked"` and applies to the dependency build, clippy, and tests as well as the package. In a workspace, set it to `"--locked -p kennel"` to build one member.

The package carries `passthru.checks.clippy` and `passthru.checks.test`, which the flake module adds to `checks` for every service and site, so nixbot runs clippy (denying warnings) and `cargo test` on every push. The other languages' linters and tests run in the `hooks` check (see [Continuous Integration](../guides/ci.md)).

Type: `attrs`, default: `{ }`

```nix
helpers.buildRustService {
  src = ./.;
  pname = "kennel";
  version = "0.1.0";
  buildArgs.cargoExtraArgs = "--locked -p kennel";
}
```

## `buildHaskellService`

Builds a Haskell project with GHC 9.14 and strips it to its static executables. Dependencies come from the nixpkgs Haskell package set rather than Hackage, so version bounds must accept the versions nixpkgs has. Use `overrides` for exceptions.

### `pname`

Package name to build. Must be a key of `localPackages`; its executables become the service binaries.

Type: `str`, required

### `localPackages`

The project's cabal packages, package name to source directory. Every package builds inside the same extended set, so intra-project dependencies resolve to each other. List the same directories as `scottylabs.haskell.localPackages` so that the shell and the build resolve identical dependencies.

Type: `attrsOf path`, required

### `overrides`

A final overlay over the package set, to jailbreak or pin a dependency the set's version cannot satisfy (for example `final: prev: { some-dep = pkgs.haskell.lib.doJailbreak prev.some-dep; }`).

Type: `function`, default: `_final: _prev: { }`

```nix
helpers.buildHaskellService {
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
helpers.buildDenoTask {
  src = ./sites/web;
  pname = "link-shortener-web";
  version = "0.1.0";
}
```

A workspace member builds from the workspace root that holds the shared `deno.lock`, with `cwd` selecting the member:

```nix
helpers.buildDenoTask {
  src = ./.;
  cwd = "apps/backend";
  pname = "housing-backend";
  entrypoint = "src/index.ts";
  compile = true;
}
```

## `buildPythonService`

Builds a [uv](https://docs.astral.sh/uv/) project into a virtual environment with [uv2nix](https://github.com/pyproject-nix/uv2nix), resolving every dependency from `uv.lock` as a Nix derivation. The result is a relocatable venv exposing each `[project.scripts]` entry under `bin/`, consumed as `${pkg}/bin/<script>`.

### `src`

Project root holding `pyproject.toml` and `uv.lock`. The package name is read from `pyproject.toml`.

Type: `path`, required

### `python`

The Python interpreter the environment is built against. Set `scottylabs.python.package` to the same interpreter so that the shell and the `hooks` check match the service.

Type: `package`, default: `pkgs.python3`

### `sourcePreference`

Whether dependencies come from prebuilt wheels (`"wheel"`) or are built from source (`"sdist"`). The build-system overlay is matched to this choice automatically.

Type: `str`, default: `"wheel"`

### `selectDeps`

Selects the locked dependency closure to install from the loaded workspace. Override to install an optional group, for example `ws: ws.deps.optionals.prod`.

Type: `function`, default: `ws: ws.deps.default`

### `overrides`

An overlay applied last over the Python package set, to supply system libraries or build tools uv2nix cannot infer for a package built from source (for example adding `pkgs.openssl` to `buildInputs`). Set `scottylabs.python.overrides` to the same overlay so that the `hooks` check can build the project's dev dependencies.

Type: `function`, default: `_final: _prev: { }`

```nix
helpers.buildPythonService {
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
helpers.buildMdbook {
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
helpers.buildZensical {
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
helpers.buildOptionsDoc {
  module = ./nixos;
  subtree = options: options.services.kennel;
  root = ./.;
  repoUrl = "https://git.cmu.dev/ScottyLabs/kennel/src/branch/main";
}
```
