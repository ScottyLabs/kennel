# RFC 0015: Configuration Schema

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-03-12
- **Updated:** 2026-03-12

## Overview

Auto-generate a JSON schema from Kennel's Rust configuration structs and publish it as part of the documentation site. This gives editors autocompletion and validation for `kennel.toml`, and provides a single source of truth for the configuration reference.

## Motivation

`kennel.toml` is the primary configuration file that project maintainers interact with. Currently, the only documentation for its format is a hand-written reference page at `sites/docs/src/reference/kennel-toml.md`. This has two problems:

- **No editor support.** Editors cannot autocomplete field names, validate types, or flag unknown keys without a schema. Typos like `cusotm_domain` go unnoticed until deployment fails.
- **Documentation drift.** The hand-written reference and the Rust structs that actually parse the file can diverge. Adding a field to the Rust struct does not automatically update the docs.

Generating the schema from the Rust source eliminates both problems: the schema is always correct, and editors consume it directly.

## Goals

- JSON schema generated from the Rust config structs via `schemars`
- Schema published as a static file in the documentation site
- Editor integration via `$schema` or [Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml) configuration
- Reference documentation in `sites/docs/` generated from or validated against the schema
- Validation of `kennel.toml` at parse time using the schema

## Non-Goals

- Schema for `projects.json` (server-side config, not user-facing)
- Schema for devenv process configuration (managed by devenv, not Kennel)
- GUI configuration editor

## Detailed Design

### Schema Generation

Add `schemars` as a dependency of `kennel-config`. Derive `JsonSchema` on the configuration structs:

```rust
use schemars::JsonSchema;

#[derive(Deserialize, JsonSchema)]
pub struct KennelConfig {
    pub cachix: Option<CachixConfig>,
    pub services: HashMap<String, ServiceConfig>,
    pub static_sites: HashMap<String, StaticSiteConfig>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ServiceConfig {
    pub custom_domain: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct StaticSiteConfig {
    pub spa: Option<bool>,
    pub custom_domain: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CachixConfig {
    pub cache: String,
}
```

The `auth_token_file` field is intentionally absent from `CachixConfig` -- Cachix authentication is server-side configuration, not user-facing (see C14 in the code review).

### Schema Output

A build script or CLI subcommand outputs the schema as JSON:

```rust
let schema = schemars::schema_for!(KennelConfig);
println!("{}", serde_json::to_string_pretty(&schema).unwrap());
```

The generated schema is committed to `sites/docs/src/reference/kennel-toml.schema.json` and served as a static file from the documentation site.

### Editor Integration

Users add a `$schema` comment to their `kennel.toml` for editor support:

```toml
#:schema https://kennel.scottylabs.org/reference/kennel-toml.schema.json

[services.api]
custom_domain = "api.example.com"

[static_sites.docs]
spa = true
```

Editors that support TOML schemas ([Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml) for VS Code, Taplo for others) provide autocompletion, validation, and hover documentation from this schema.

### Documentation Site

The existing reference page at `sites/docs/src/reference/kennel-toml.md` is updated to reflect the current config struct fields. Going forward, field additions to the Rust structs automatically appear in the schema. The reference page can either be generated from the schema or maintained manually with CI validation that it matches the schema.

### Parse-Time Validation

`parse_kennel_toml` in `kennel-config` validates the parsed TOML against the schema before returning, catching unknown fields and type mismatches early with clear error messages. TOML's `serde` deserialization already handles type validation, but `schemars` validation adds:

- Unknown field detection (with `#[serde(deny_unknown_fields)]`)
- Custom validation rules (e.g., domain format validation)
- Descriptive error messages referencing the schema

## Alternatives Considered

**Hand-maintained JSON schema.** Write the schema manually. Drifts from the code immediately.

**TypeScript/Zod schema.** Generate from a different language's type system. Adds a build dependency and another source of truth.

**No schema, just documentation.** The current approach. No editor support, documentation drifts.

## Implementation Phases

### Schema Generation

Add `schemars` to `kennel-config`. Derive `JsonSchema` on config structs. Add a way to output the schema (CLI subcommand or build script).

### Documentation

Commit the generated schema to `sites/docs/`. Update the reference page. Add CI check that the committed schema matches what the code generates.

### Editor Integration

Document the `#:schema` comment pattern in the docs site and in the example `kennel.toml`.
