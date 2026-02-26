# Kennel RFCs

This directory contains Request for Comments (RFC) documents that guide the technical design and architecture of Kennel.

## What are RFCs?

RFCs are design documents that propose and document significant technical decisions. They provide a consistent and controlled path for new features and architectural changes to enter the project.

## When to write an RFC

Write an RFC when you want to propose:

- Major architectural decisions
- New features that affect multiple parts of the system
- Breaking changes to existing APIs or data models
- Changes to development processes or tooling

Small bug fixes, documentation improvements, and minor features don't need RFCs.

## RFC Process

1. **Draft** - Author creates RFC document and opens a pull request
1. **Review** - Team discusses in PR comments, author revises based on feedback
1. **Accepted/Rejected** - RFC is either merged (accepted) or closed (rejected)

Accepted RFCs live permanently in this directory as historical documentation of why decisions were made. When implementing an RFC, if there are any implementation details that need to be discussed or changed, the RFC should be updated to reflect those details.

## RFC Format

Each RFC should include:

```markdown
# RFC ####: Title

- **Status:** Draft | Accepted | Rejected
- **Author(s):** @username
- **Created:** YYYY-MM-DD
- **Updated:** YYYY-MM-DD

## Overview

Brief 2-3 sentence summary of the proposal.

## Motivation

Why are we doing this? What problem does it solve?

## Goals

What are we trying to achieve?

## Non-Goals

What is explicitly out of scope?

## Detailed Design

The meat of the RFC. Explain the design in enough detail that:
- Its interaction with other features is clear
- It's reasonably clear how to implement
- Corner cases are discussed

Include code examples, diagrams, or data models where helpful.

## Alternatives Considered

What other approaches were considered and why weren't they chosen?

## Open Questions

What parts of the design still need to be figured out?

## Implementation Phases

If this is a large change, how should it be broken down?
```

## Naming Convention

RFCs are numbered sequentially (zero-padded to 4 digits for consistent sorting) and use kebab-case:

```
####-short-descriptive-title.md
```

When submitting RFCs, create branches with the `rfc/` prefix, e.g. `rfc/0001-dev-environment-ci`. This makes it easy to identify RFC branches in the repository.

The filename should match the branch name, e.g. `0001-dev-environment-ci.md`.

## Current RFCs

| Number | Title | Status |
|--------|-------|--------|
| 0001 | [Dev Environment & CI](./0001-dev-environment-ci.md) | Accepted |
| 0002 | [Frontend](./0002-frontend.md) | Accepted |
| 0003 | [Database Schema & State Management](./0003-database-schema.md) | Accepted |
| 0004 | [Core Pipeline Architecture](./0004-core-pipeline.md) | Accepted |
| 0005 | [NixOS Module](./0005-nixos-module.md) | Accepted |
| 0006 | [DNS Management](./0006-dns-management.md) | Accepted |

*(This index will be updated as RFCs are added)*

## Questions?

Open an issue or ask in the [ScottyLabs Discord](https://go.scottylabs.org/discord).
