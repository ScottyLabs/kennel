# Offline npm cache for deno, built from a deno.lock
{
  lib,
  stdenv,
  fetchurl,
  runCommand,
  writeText,
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
in
{
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

      # deno 2.9+ only treats a packument as cached when it carries the _deno markers
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
    in
    # The npm cache deno reads is the extracted tarball plus a packument per package
    runCommand "${name}-deno-cache" { } ''
      mkdir -p "$out/npm/registry.npmjs.org"
      ${lib.concatStringsSep "\n" (
        map (e: ''
          dest="$out/npm/registry.npmjs.org/${e.name}/${e.version}"
          mkdir -p "$dest"
          tar -xzf ${e.tarball} -C "$dest" --strip-components=1
        '') npmEntries
      )}
      ${lib.concatStringsSep "\n" (
        lib.mapAttrsToList (pkg: file: ''
          cp ${file} "$out/npm/registry.npmjs.org/${pkg}/registry.json"
        '') packuments
      )}
    '';

  # patchelf linux addons, dropping musl packages it cannot fix
  patchAddons = lib.optionalString stdenv.isLinux ''
    if [ -d node_modules ]; then
      find node_modules -type d -name '*musl*' -prune -exec rm -rf {} +
      autoPatchelf node_modules
    fi
  '';
}
