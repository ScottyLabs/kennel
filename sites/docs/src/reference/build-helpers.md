# Build Helpers

These builders come from the [shared ScottyLabs devenv flake](https://codeberg.org/ScottyLabs/devenv) as `mkLib`, a function applied to a `pkgs` set (the same shape as `crane.mkLib`). Add the flake as an input, then call a helper per system:

```nix
# flake.nix inputs
scottylabs = {
  url = "git+https://codeberg.org/ScottyLabs/devenv";
  inputs.nixpkgs.follows = "nixpkgs";
};

# in outputs
pkgs = nixpkgs.legacyPackages.${system};
kennel = (scottylabs.mkLib pkgs).buildRustService { ... };
```

Each helper takes the consumer's `pkgs`, so packages build against your nixpkgs pin rather than the shared flake's.

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

## `buildDenoTask`

Builds a Deno project that has npm dependencies, running a `deno task` and copying its output directory. On Linux it patches the prebuilt native addons and drops the musl variants, which cannot be patched against a glibc host.

### `src`

Directory holding `deno.lock` and `deno.json`.

Type: `path`, required

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
  repoUrl = "https://codeberg.org/ScottyLabs/kennel/src/branch/main";
}
```
