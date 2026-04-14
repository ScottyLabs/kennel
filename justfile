#!/usr/bin/env -S just --justfile

set dotenv-load

# Show this help message
help:
    @just --list

# Start kennel server
server:
    cargo run -p kennel

# Create a new database migration
migration NAME:
    sea-orm-cli migrate generate {{NAME}} -d crates/migration

# Run database migrations
migrate:
    DATABASE_URL="sqlite://.devenv/state/kennel.db?mode=rwc" sea-orm-cli migrate up -d crates/migration

# Generate SeaORM entities from database schema
generate-entities:
    DATABASE_URL="sqlite://.devenv/state/kennel.db" sea-orm-cli generate entity -o crates/entity/src --with-serde both --lib

# Start docs dev server
docs:
    cd sites/docs && mdbook serve

# Clean devenv state
clean:
    rm -rf .devenv/state
