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

Builds a Rust crate with [crane](https://crane.dev), caching dependencies separately from the build. Dependencies are fetched by the checksums in `Cargo.lock`, so there is no hash to maintain.

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

Builds a Deno project that has npm dependencies, running a `deno task` and copying its output directory. Each package is fetched by the integrity already in `deno.lock`, so there is no hash to maintain. On Linux it patches the prebuilt native addons and drops the musl variants, which cannot be patched against a glibc host.

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

```nix
(scottylabs.mkLib pkgs).buildDenoTask {
  src = ./sites/web;
  pname = "link-shortener-web";
  version = "0.1.0";
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
