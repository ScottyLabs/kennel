# Builds a deno project with npm deps from deno.lock
{
  lib,
  stdenv,
  deno,
  autoPatchelfHook,
  fetchurl,
  runCommand,
}:

{
  src,
  pname,
  version ? "0.1.0",
  task ? "build",
  output ? "dist",
  entrypoint ? null,
}:

let
  lock = builtins.fromJSON (builtins.readFile (src + "/deno.lock"));

  parse =
    key:
    let
      # Split the scoped name from the version, bounding the version at the
      # first "_" so Deno's optional "_peer@ver" suffix is dropped. A version
      # never contains "_", while a package name may
      m = builtins.match "(@?[^@]+)@([^_]+).*" key;
    in
    {
      name = builtins.elemAt m 0;
      version = builtins.elemAt m 1;
    };

  tarballs = lib.mapAttrs' (
    key: info:
    let
      p = parse key;
      unscoped = lib.last (lib.splitString "/" p.name);
    in
    lib.nameValuePair "${p.name}@${p.version}" {
      inherit (p) name version;
      tarball = fetchurl {
        url = "https://registry.npmjs.org/${p.name}/-/${unscoped}-${p.version}.tgz";
        hash = info.integrity;
      };
    }
  ) (lock.npm or { });

  # The npm cache deno reads is just the extracted tarball per version
  denoCache = runCommand "${pname}-deno-cache" { } ''
    mkdir -p "$out/npm/registry.npmjs.org"
    ${lib.concatStringsSep "\n" (
      lib.mapAttrsToList (_: p: ''
        dest="$out/npm/registry.npmjs.org/${p.name}/${p.version}"
        mkdir -p "$dest"
        tar -xzf ${p.tarball} -C "$dest" --strip-components=1
      '') tarballs
    )}
  '';

  # Patchelf linux addons, dropping musl which it cannot fix
  patchAddons = lib.optionalString stdenv.isLinux ''
    find node_modules -path '*-musl*' -name '*.node' -delete
    autoPatchelf node_modules
  '';
in
stdenv.mkDerivation {
  inherit pname version src;

  nativeBuildInputs = [ deno ] ++ lib.optional stdenv.isLinux autoPatchelfHook;
  buildInputs = lib.optionals stdenv.isLinux [ stdenv.cc.cc.lib ];
  dontAutoPatchelf = true;

  configurePhase = ''
    runHook preConfigure
    export HOME="$TMPDIR"
    export DENO_DIR="$TMPDIR/deno"
    mkdir -p "$DENO_DIR"
    cp -r ${denoCache}/npm "$DENO_DIR/npm"
    chmod -R u+w "$DENO_DIR"
    deno install --cached-only --frozen${lib.optionalString (entrypoint != null) " --entrypoint ${lib.escapeShellArg entrypoint}"}
    ${patchAddons}
    runHook postConfigure
  '';

  buildPhase = ''
    runHook preBuild
    deno task ${task}
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    cp -r ${output} "$out"
    runHook postInstall
  '';
}
