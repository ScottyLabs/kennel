#!/usr/bin/env bash
# shellcheck disable=SC2016 # $p and $url are jq variables

lock=$1
out=$2
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$out"

fetch() {
  curl -fsSL --retry 3 "$1" -o "$tmp/file"
  echo "$2  $tmp/file" | sha256sum -c --quiet -
  # deno's http cache keys each file by the hash of its URL path
  key=$(printf '%s' "${1#https://jsr.io}" | sha256sum | cut -d' ' -f1)
  {
    cat "$tmp/file"
    printf '\n// denoCacheMetadata=%s' "$(jq -cn --arg url "$1" '{headers: {}, time: 0, url: $url}')"
  } > "$out/$key"
}

jq -r '.jsr // {} | to_entries[]
  | (.key | capture("^(?<name>@?[^@]+)@(?<version>[^_]+)")) as $p
  | [$p.name, $p.version, .value.integrity] | @tsv' "$lock" |
  while IFS=$'\t' read -r name version integrity; do
    base="https://jsr.io/$name/$version"
    # deno.lock pins _meta.json, which lists every file's checksum
    fetch "${base}_meta.json" "$integrity"
    cp "$tmp/file" "$tmp/meta"
    jq -r '.manifest | to_entries[] | [.key, (.value.checksum | ltrimstr("sha256-"))] | @tsv' "$tmp/meta" |
      while IFS=$'\t' read -r path checksum; do
        fetch "$base$path" "$checksum"
      done
  done
