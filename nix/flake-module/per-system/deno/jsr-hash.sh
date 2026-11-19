#!/usr/bin/env bash

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

jsr_hash() {
  rm -rf "$tmp/cache"
  fetch-jsr "$1" "$tmp/cache"
  nix --extra-experimental-features nix-command hash path "$tmp/cache"
}

changed=0
for lock in "$@"; do
  if ! jq -e '.jsr // {} | length > 0' "$lock" >/dev/null; then
    continue
  fi

  hash_file="$(dirname "$lock")/deno.jsr.hash"
  hash=$(jsr_hash "$lock")

  if [ "$(cat "$hash_file" 2>/dev/null)" != "$hash" ]; then
    echo "$hash" > "$hash_file"
    echo "updated $hash_file"
    changed=1
  fi
done

# fails the hook so the updated hash gets staged
exit "$changed"
