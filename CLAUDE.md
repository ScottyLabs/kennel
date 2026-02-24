# Claude Code Guidelines

## Dev Environment

This project uses [devenv](https://devenv.sh/) for development environments. If you need to install new dependencies, tools, or modify the environment, you can do so within the [`devenv.nix`](./devenv.nix) file.

To run commands within the development environment, use the `devenv shell` command followed by the desired command. For example:

```bash
devenv shell -- cargo build
```

## Commenting Style

1. *No meta-commentary.* Prohibit all references to the chat, user requests, or AI actions (e.g., "Fixed X," "As requested," or "Increased/changed X").
1. *State-only.* Describe the current intent, logic, inputs, and outputs of the code. Assume the reader has zero context of previous versions or this conversation. If a comment describes a bug that has been fixed, or describes changes instead of function at all, delete it.
1. *Human-style.* Avoid special characters, like '→' (prefer '->') and '—' / '–' (prefer '---' / '--'). Especially avoid emojis and symbol characters like '✓' and '✗'.

## Dependencies

- When creating new crates, use `cargo init`. Check the patterns used in the other `Cargo.toml` in this workspace to figure out what changes should be made afterward.
- Always use `cargo add` to add Rust dependencies to ensure you're getting the latest version.
- Do not manually edit `Cargo.toml` dependency sections or add inline comments to dependencies.
- The same applies to `bun.lock` and web dependencies.

## Markdown Style

- URLs must always be written as proper markdown links with descriptive text: `[description](url)`. Never use bare URLs or URLs in backticks as a substitute for links.
