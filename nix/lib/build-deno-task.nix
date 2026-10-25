# Builds a deno project (or workspace member) with npm deps from deno.lock
{
  lib,
  stdenv,
  deno,
  autoPatchelfHook,
  callPackage,
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
  denoLib = callPackage ./deno.nix { };
  denoCache = denoLib.mkCache {
    name = pname;
    lockfile = src + "/deno.lock";
  };
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
    ${denoLib.patchAddons}
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
