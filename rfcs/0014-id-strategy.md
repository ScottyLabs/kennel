# RFC 0014: ID Strategy

- **Status:** Accepted
- **Author(s):** @ap-1
- **Created:** 2026-03-12
- **Updated:** 2026-03-12

## Overview

Replace integer serial primary keys with UUIDv7 for database storage and TypeIDs for user-facing representation. UUIDv7 provides time-ordered, globally unique, index-friendly primary keys. TypeIDs add a human-readable type prefix for logs, API responses, and debugging.

## Motivation

Kennel currently uses `i32` serial primary keys for builds, deployments, and other entities. This has several problems:

- **No type distinction.** Build IDs and deployment IDs are both `i32`. Nothing prevents accidentally passing a build ID where a deployment ID is expected -- in function arguments, channel messages, or log output.
- **Sequential and guessable.** Serial IDs leak information about system activity (build #1042 tells you how many builds have run) and are trivially enumerable.
- **Not globally unique.** IDs are scoped to a single table. The value `42` could be a build, a deployment, or a DNS record. In logs, `"processing 42"` is ambiguous without additional context.
- **Poor index locality for time-range queries.** Serials correlate with insertion order but the correlation is implicit, not embedded in the value.

The [split-ID strategy](https://blog.alcazarsec.com/tech/posts/better-database-ids) addresses all of these: UUIDv7 internally for database performance, TypeIDs externally for human readability and type safety.

## Goals

- UUIDv7 primary keys on all entity tables
- TypeID representation for all user-facing contexts (API responses, logs, error messages)
- Type-safe ID newtypes in Rust (cannot mix build IDs and deployment IDs)

## Non-Goals

- Backward compatibility with existing serial IDs
- Sharding or distributed ID generation
- Custom ID generation beyond UUIDv7

## Detailed Design

### UUIDv7 in the Database

All primary keys use `UUID` with a default of `uuid_generate_v7()`. UUIDv7 embeds a Unix timestamp in the first 48 bits, so new IDs are monotonically increasing and append to the end of B-tree indexes, preserving insert performance equivalent to serials.

The `pg_uuidv7` extension (v1.7.0, available in nixpkgs for PostgreSQL 14-18) provides `uuid_generate_v7()`. The devenv and NixOS module configurations add the extension to PostgreSQL.

Foreign keys referencing these tables also use `UUID`.

### TypeIDs in the Application

A TypeID is a prefixed, base32-encoded UUID following the [TypeID specification](https://github.com/jetify-com/typeid). Examples:

- `bld_01h455vb4pex5vsknk084sn02q` -- a build
- `depn_01h455vb4pex5vsknk084sn02q` -- a deployment
- `drec_01h455vb4pex5vsknk084sn02q` -- a DNS record
- `proj_01h455vb4pex5vsknk084sn02q` -- a project
- `srvc_01h455vb4pex5vsknk084sn02q` -- a service
- `bres_01h455vb4pex5vsknk084sn02q` -- a build result

TypeIDs are used in:

- API responses
- Log output (structured span fields)
- Error messages
- Webhook callback payloads

TypeIDs are never stored in the database. The database stores raw UUIDs. Conversion happens at the application boundary.

### Rust Implementation

Each entity gets a newtype wrapper around `uuid::Uuid`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BuildId(Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeploymentId(Uuid);
```

Each newtype implements:

- `Display` -- renders as TypeID (`bld_01h455vb4p...`)
- `FromStr` -- parses a TypeID, validating the prefix
- `From<Uuid>` / `Into<Uuid>` -- for database conversion
- SeaORM value conversion -- for query parameter binding

A shared macro generates the boilerplate:

```rust
type_id!(BuildId, "bld");
type_id!(DeploymentId, "depn");
type_id!(ProjectId, "proj");
type_id!(ServiceId, "srvc");
type_id!(DnsRecordId, "drec");
type_id!(BuildResultId, "bres");
```

### Prefix Table

| Entity | Prefix | Newtype |
|--------|--------|---------|
| Build | `bld` | `BuildId` |
| Build Result | `bres` | `BuildResultId` |
| Deployment | `depn` | `DeploymentId` |
| DNS Record | `drec` | `DnsRecordId` |
| Project | `proj` | `ProjectId` |
| Service | `srvc` | `ServiceId` |

### Integration with DB-as-Queue

With the DB-as-queue architecture, workers claim from the database rather than receiving IDs through channels. The `Notify` signals carry no payload. Database queries return entities with typed UUID fields. The type confusion that exists with bare `i32` channels disappears at the architectural level.

## Alternatives Considered

**UUIDv4 everywhere.** Fully random, no timestamp component. Worse B-tree performance due to random insertion order. No time-ordering benefit.

**ULID.** Similar to UUIDv7 (timestamp + random), but uses a non-standard encoding. UUIDv7 is an IETF standard ([RFC 9562](https://www.rfc-editor.org/rfc/rfc9562)) and is natively supported by PostgreSQL's `uuid` type.

**Keep serials, add TypeID prefix only in the application.** Serials still lack global uniqueness and type safety at the database level.

**Snowflake IDs.** Require a worker ID component for distributed generation. Unwarranted for a single-instance system.

## Implementation Phases

### ID Types

Create the newtype wrappers, `type_id!` macro, `Display`/`FromStr` implementations, and SeaORM integration. This can be a module in `entity` or a new shared crate.

### Schema

Update all table definitions to use UUID primary keys and foreign keys. Add `pg_uuidv7` extension to devenv and NixOS module PostgreSQL configurations. Regenerate entities.

### Codebase

Update all function signatures, store methods, and API responses to use the typed IDs. The compiler enforces correctness -- every `i32` that should be a `BuildId` or `DeploymentId` becomes a type error.
