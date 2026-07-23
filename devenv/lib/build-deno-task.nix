# Builds a deno project (or workspace member) with npm deps from deno.lock
{
  lib,
  stdenv,
  deno,
  autoPatchelfHook,
  fetchurl,
  runCommand,
  writeText,
}:

{
  src,
  # Workspace member subdir under src ("." for a single-project src)
  cwd ? ".",
  pname,
  version ? "0.1.0",
  task ? "build",
  output ? "dist",
  entrypoint ? null,
  # Provision denort so `deno compile` runs inside the offline sandbox
  compile ? false,
}:

let
  lock = builtins.fromJSON (builtins.readFile (src + "/deno.lock"));

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

  npmEntries = lib.mapAttrsToList (
    key: info:
    let
      p = parse key;
      unscoped = lib.last (lib.splitString "/" p.name);
    in
    {
      inherit (p) name version;
      inherit unscoped;
      integrity = info.integrity;
      tarball = fetchurl {
        url = "https://registry.npmjs.org/${p.name}/-/${unscoped}-${p.version}.tgz";
        hash = info.integrity;
      };
    }
  ) (lock.npm or { });

  # deno 2.9+ only treats a packument as cached when it carries the _deno markers
  # so synthesize one per package from the lock beside the extracted tarballs
  packuments = lib.mapAttrs (
    name: entries:
    writeText "registry.json" (
      builtins.toJSON {
        inherit name;
        "dist-tags".latest = lib.last (lib.sort (a: b: a < b) (map (e: e.version) entries));
        versions = lib.listToAttrs (
          map (
            e:
            lib.nameValuePair e.version {
              inherit (e) version;
              dependencies = { };
              dist = {
                tarball = "https://registry.npmjs.org/${name}/-/${e.unscoped}-${e.version}.tgz";
                integrity = e.integrity;
              };
            }
          ) entries
        );
        "_deno.etag" = "W/\"nix\"";
        "_deno.packumentFormat" = "full";
      }
    )
  ) (lib.groupBy (e: e.name) npmEntries);

  # The npm cache deno reads is the extracted tarball plus a packument per package
  denoCache = runCommand "${pname}-deno-cache" { } ''
    mkdir -p "$out/npm/registry.npmjs.org"
    ${lib.concatStringsSep "\n" (
      map (e: ''
        dest="$out/npm/registry.npmjs.org/${e.name}/${e.version}"
        mkdir -p "$dest"
        tar -xzf ${e.tarball} -C "$dest" --strip-components=1
      '') npmEntries
    )}
    ${lib.concatStringsSep "\n" (
      lib.mapAttrsToList (name: file: ''
        cp ${file} "$out/npm/registry.npmjs.org/${name}/registry.json"
      '') packuments
    )}
  '';

  # patchelf linux addons, dropping musl which it cannot fix
  patchAddons = lib.optionalString stdenv.isLinux ''
    if [ -d node_modules ]; then
      find node_modules -path '*-musl*' -name '*.node' -delete
      autoPatchelf node_modules
    fi
  '';
in
stdenv.mkDerivation {
  inherit pname version src;

  nativeBuildInputs = [ deno ] ++ lib.optional stdenv.isLinux autoPatchelfHook;
  buildInputs = lib.optionals stdenv.isLinux [ stdenv.cc.cc.lib ];
  dontAutoPatchelf = true;
  # strip/patchelf would corrupt the trailer deno compile appends
  dontStrip = compile;
  dontPatchELF = compile;

  configurePhase = ''
    runHook preConfigure
    export HOME="$TMPDIR"
    export DENO_DIR="$TMPDIR/deno"
    mkdir -p "$DENO_DIR"
    cp -r ${denoCache}/npm "$DENO_DIR/npm"
    chmod -R u+w "$DENO_DIR"
    ${lib.optionalString compile ''export DENORT_BIN="${deno.denort}/bin/denort"''}
    deno install --cached-only --frozen${
      lib.optionalString (entrypoint != null) " --entrypoint ${lib.escapeShellArg "${cwd}/${entrypoint}"}"
    }
    ${patchAddons}
    runHook postConfigure
  '';

  buildPhase = ''
    runHook preBuild
    ( cd ${lib.escapeShellArg cwd} && deno task ${task} )
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    cp -r ${lib.escapeShellArg "${cwd}/${output}"} "$out"
    runHook postInstall
  '';
}
