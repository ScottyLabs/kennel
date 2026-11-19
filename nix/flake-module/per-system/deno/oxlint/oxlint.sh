#!/usr/bin/env bash

config=$KENNEL_OXLINT_CONFIG
if [ -e .oxlintrc.json ]; then
  config=.oxlintrc.json
fi

ignore=(
  # mostly generated API types
  '*.d.ts'
  # agent tool directories hold vendored skill scripts
  '.agent/**'
  '.agents/**'
  '.claude/**'
  '.codex/**'
  '.continue/**'
  '.cursor/**'
  '.gemini/**'
  '.opencode/**'
  '.pi/**'
  '.roo/**'
  '.windsurf/**'
)

# --config also skips nested configs
exec oxlint --config "$config" "${ignore[@]/#/--ignore-pattern=}" "$@"
