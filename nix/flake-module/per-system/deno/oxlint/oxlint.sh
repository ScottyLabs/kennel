#!/usr/bin/env bash

config=$KENNEL_OXLINT_CONFIG
for candidate in .oxlintrc.json .oxlintrc.jsonc oxlint.config.ts oxlint.config.mts; do
  if [ -e "$candidate" ]; then
    config=$candidate
    break
  fi
done

if [ "$config" != "$KENNEL_OXLINT_CONFIG" ] && ! oxlint --print-config --config "$config" \
  | jq -e '.jsPlugins // [] | any(.name == "anti-slop")' >/dev/null; then
  echo "$config: add \"./.scottylabs/oxlint/oxlintrc.json\" to \"extends\" so kennel's oxlint config applies" >&2
  exit 1
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
