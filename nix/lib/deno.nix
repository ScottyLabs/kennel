# Offline npm and JSR cache for deno
{
  lib,
  stdenv,
  fetchurl,
  runCommand,
  writeText,
  writeShellApplication,
  curl,
  jq,
  cacert,
}:

let
  parse =
    key:
    let
      # Split name@version, bounding the version at the first "_" to drop Deno's "_peer" suffix
      m = builtins.match "(@?[^@]+)@([^_]+).*" key;
    in
    {
      name = builtins.elemAt m 0;
      version = builtins.elemAt m 1;
    };

  # fetch the JSR files in deno.lock into deno's cache layout
  fetchJsr = writeShellApplication {
    name = "fetch-jsr";
    runtimeInputs = [
      curl
      jq
    ];
    # $p and $url are jq variables
    excludeShellChecks = [ "SC2016" ];
    text = ''
      lock=$1
      out=$2
      tmp=$(mktemp -d)
      trap 'rm -rf "$tmp"' EXIT
      mkdir -p "$out"

      fetch() {
        curl -fsSL --retry 3 "$1" -o "$tmp/file"
        echo "$2  $tmp/file" | sha256sum -c --quiet -
        # deno's http cache keys each file by the hash of its URL path
        key=$(printf '%s' "''${1#https://jsr.io}" | sha256sum | cut -d' ' -f1)
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
          fetch "''${base}_meta.json" "$integrity"
          cp "$tmp/file" "$tmp/meta"
          jq -r '.manifest | to_entries[] | [.key, (.value.checksum | ltrimstr("sha256-"))] | @tsv' "$tmp/meta" |
            while IFS=$'\t' read -r path checksum; do
              fetch "$base$path" "$checksum"
            done
        done
    '';
  };
in
{
  inherit fetchJsr;

  mkCache =
    { name, lockfile }:
    let
      lock = builtins.fromJSON (builtins.readFile lockfile);

      npmEntries = lib.mapAttrsToList (
        key: info:
        let
          p = parse key;
          unscoped = lib.last (lib.splitString "/" p.name);
        in
        {
          inherit (p) name version;
          inherit unscoped;
          inherit (info) integrity;
          tarball = fetchurl {
            url = "https://registry.npmjs.org/${p.name}/-/${unscoped}-${p.version}.tgz";
            hash = info.integrity;
          };
        }
      ) (lock.npm or { });

      # deno 2.9+ only treats a packument as cached when it carries the _deno markers,
      # so synthesize one per package from the lock beside the extracted tarballs
      packuments = lib.mapAttrs (
        pkg: entries:
        writeText "registry.json" (
          builtins.toJSON {
            name = pkg;
            "dist-tags".latest = lib.last (lib.sort (a: b: a < b) (map (e: e.version) entries));
            versions = lib.listToAttrs (
              map (
                e:
                lib.nameValuePair e.version {
                  inherit (e) version;
                  dependencies = { };
                  dist = {
                    tarball = "https://registry.npmjs.org/${pkg}/-/${e.unscoped}-${e.version}.tgz";
                    inherit (e) integrity;
                  };
                }
              ) entries
            );
            "_deno.etag" = "W/\"nix\"";
            "_deno.packumentFormat" = "full";
          }
        )
      ) (lib.groupBy (e: e.name) npmEntries);

      jsrEntries = map parse (lib.attrNames (lock.jsr or { }));
      jsrHashFile = dirOf lockfile + "/deno.jsr.hash";

      jsrCache = runCommand "${name}-jsr" {
        nativeBuildInputs = [ fetchJsr ];
        SSL_CERT_FILE = "${cacert}/etc/ssl/certs/ca-bundle.crt";
        outputHashMode = "recursive";
        outputHash =
          if builtins.pathExists jsrHashFile then
            lib.trim (builtins.readFile jsrHashFile)
          else
            throw "missing ${toString jsrHashFile}; run `prek run deno-jsr-hash --all-files`";
      } ''fetch-jsr ${lockfile} "$out"'';

      # unpinned package metadata listing only the locked versions
      jsrPackages = lib.mapAttrsToList (
        pkg: entries:
        let
          versions = lib.sort (a: b: a < b) (map (e: e.version) entries);
        in
        {
          url = "https://jsr.io/${pkg}/meta.json";
          file = writeText "jsr-package-meta" (
            builtins.toJSON {
              scope = lib.removePrefix "@" (builtins.head (lib.splitString "/" pkg));
              name = lib.last (lib.splitString "/" pkg);
              latest = lib.last versions;
              versions = lib.genAttrs versions (_: { });
            }
          );
        }
      ) (lib.groupBy (e: e.name) jsrEntries);
    in
    # The npm cache deno reads is the extracted tarball plus a packument per package
    runCommand "${name}-deno-cache" { } ''
      mkdir -p "$out/npm/registry.npmjs.org" "$out/remote/https/jsr.io"
      ${lib.concatStringsSep "\n" (
        map (e: ''
          dest="$out/npm/registry.npmjs.org/${e.name}/${e.version}"
          mkdir -p "$dest"
          tar -xzf ${e.tarball} -C "$dest" --strip-components=1 --delay-directory-restore
        '') npmEntries
      )}
      ${lib.concatStringsSep "\n" (
        lib.mapAttrsToList (pkg: file: ''
          cp ${file} "$out/npm/registry.npmjs.org/${pkg}/registry.json"
        '') packuments
      )}
      ${lib.concatStringsSep "\n" (
        map (f: ''
          {
            cat ${f.file}
            printf '\n// denoCacheMetadata=%s' ${
              lib.escapeShellArg (
                builtins.toJSON {
                  headers = { };
                  inherit (f) url;
                  time = 0;
                }
              )
            }
          } > "$out/remote/https/jsr.io/${builtins.hashString "sha256" (lib.removePrefix "https://jsr.io" f.url)}"
        '') jsrPackages
      )}
      ${lib.optionalString (jsrEntries != [ ]) ''cp -r ${jsrCache}/. "$out/remote/https/jsr.io/"''}
    '';

  # patchelf linux addons, dropping musl packages it cannot fix
  patchAddons = lib.optionalString stdenv.hostPlatform.isLinux ''
    if [ -d node_modules ]; then
      find node_modules -type d -name '*musl*' -prune -exec rm -rf {} +
      autoPatchelf node_modules
    fi
  '';
}
