# Offline npm and JSR cache for deno
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

      # the locked version metadata pins every file's hash
      jsrEntries = lib.mapAttrsToList (
        key: info:
        let
          p = parse key;
          base = "https://jsr.io/${p.name}/${p.version}";
          meta = fetchurl {
            name = "jsr-meta";
            url = "${base}_meta.json";
            sha256 = info.integrity;
          };
        in
        {
          inherit (p) name version;
          files = [
            {
              url = "${base}_meta.json";
              file = meta;
            }
          ]
          ++ lib.mapAttrsToList (path: entry: {
            url = "${base}${path}";
            file = fetchurl {
              name = "jsr-file";
              url = "${base}${path}";
              sha256 = lib.removePrefix "sha256-" entry.checksum;
            };
          }) (lib.importJSON meta).manifest;
        }
      ) (lock.jsr or { });

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

      # deno's http cache keys each file by the hash of its URL path
      jsrFiles = jsrPackages ++ lib.concatMap (e: e.files) jsrEntries;
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
        '') jsrFiles
      )}
    '';

  # patchelf linux addons, dropping musl packages it cannot fix
  patchAddons = lib.optionalString stdenv.hostPlatform.isLinux ''
    if [ -d node_modules ]; then
      find node_modules -type d -name '*musl*' -prune -exec rm -rf {} +
      autoPatchelf node_modules
    fi
  '';
}
