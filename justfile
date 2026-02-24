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

# Generate OpenAPI specs for web and docs
generate-api:
    cd sites/web && bun run generate-api
    cd sites/docs && bun run generate-api

# Start web dashboard dev server
web:
    cd sites/web && bun dev

# Start docs dev server
docs:
    cd sites/docs && bun dev

# Attach to process-compose interface
attach:
    process-compose attach

# Stop infrastructure services
down:
    process-compose down

# Clean devenv state (removes all service data)
clean: down
    rm -rf .devenv/state
