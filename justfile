#!/usr/bin/env -S just --justfile

set dotenv-load

# Show this help message
help:
    @just --list

# Start infrastructure services (postgres)
services:
    devenv up -d

# Start backend server
server: services
    cargo run -p kennel

# Create a new database migration
migration NAME:
    sea-orm-cli migrate generate {{NAME}} -d crates/migration

# Run database migrations
migrate:
    sea-orm-cli migrate up -d crates/migration

# Generate SeaORM entities from database schema
generate-entities:
    sea-orm-cli generate entity -o crates/entity/src --with-serde both --lib --model-extra-derives 'utoipa::ToSchema' --enum-extra-derives 'utoipa::ToSchema'

# Generate OpenAPI specs for web dashboard
generate-api:
    cd sites/web && bun run generate-api

# Start web dashboard dev server
web:
    cd sites/web && bun dev

# Start docs dev server
docs:
    cd sites/docs && mdbook serve

# Stop infrastructure services
down:
    devenv processes down

# Clean devenv state (removes all service data)
clean:
    rm -rf .devenv/state
