# Database Migrations

This crate contains SeaORM migrations for the Kennel database schema.

## Running Migrations

Use `sea-orm-cli` for migration commands. From the project root:

- **Generate a new migration file**

  ```sh
  sea-orm-cli migrate generate -d crates/migration <MIGRATION_NAME>
  ```

- **Apply all pending migrations**

  ```sh
  sea-orm-cli migrate up -d crates/migration
  ```

- **Apply first 10 pending migrations**

  ```sh
  sea-orm-cli migrate up -d crates/migration -n 10
  ```

- **Rollback last applied migration**

  ```sh
  sea-orm-cli migrate down -d crates/migration
  ```

- **Rollback last 10 applied migrations**

  ```sh
  sea-orm-cli migrate down -d crates/migration -n 10
  ```

- **Drop all tables and reapply all migrations**

  ```sh
  sea-orm-cli migrate fresh -d crates/migration
  ```

- **Rollback all migrations and reapply**

  ```sh
  sea-orm-cli migrate refresh -d crates/migration
  ```

- **Rollback all applied migrations**

  ```sh
  sea-orm-cli migrate reset -d crates/migration
  ```

- **Check migration status**

  ```sh
  sea-orm-cli migrate status -d crates/migration
  ```

## Alternative: Using cargo run

You can also use `cargo run` from within the `crates/migration` directory:

```sh
cd crates/migration
cargo run -- generate <MIGRATION_NAME>
cargo run -- up
cargo run -- status
```

## Database Connection

Migrations require a `DATABASE_URL` environment variable. In the devenv shell, this is automatically set to `postgresql://127.0.0.1:5432/kennel`.

## Generating Entities

After running migrations or modifying the schema, regenerate SeaORM entities:

```sh
just generate-entities
```

This generates Rust structs from the database schema in the `crates/entity` crate. The `--lib` flag ensures entities are generated as `lib.rs` instead of `mod.rs`, and `--model-extra-derives` and `--enum-extra-derives` add OpenAPI schema support for models and enums for the dashboard API.
