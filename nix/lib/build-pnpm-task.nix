# Builds a pnpm project (or workspace member) with deps from pnpm-lock.yaml
{
  lib,
  stdenv,
  nodejs_24,
  pnpm,
  fetchPnpmDeps,
  pnpmConfigHook,
  autoPatchelfHook,
  makeWrapper,
  # Environment variables from mkLib
  buildEnv ? { },
}:

{
  src,
  # Workspace member subdir under src ("." for a single-project src)
  cwd ? ".",
  pname,
  version ? "0.1.0",
  # Hash of the fetched pnpm store, "" to have nix report it
  hash,
  script ? "build",
  output ? "dist",
  # Script under cwd to run with node, making the result a service instead of output
  main ? null,
  nodejs ? nodejs_24,
}:

let
  # patchelf linux addons, dropping musl packages it cannot fix and the links left pointing at them
  patchAddons = lib.optionalString stdenv.hostPlatform.isLinux ''
    if [ -d node_modules ]; then
      find node_modules -type d -name '*musl*' -prune -exec rm -rf {} +
      find . -xtype l -delete
      autoPatchelf node_modules
    fi
  '';
in
stdenv.mkDerivation {
  inherit pname version src;
  env = buildEnv;

  pnpmDeps = fetchPnpmDeps {
    inherit
      pname
      version
      src
      hash
      pnpm
      ;
    fetcherVersion = 4;
  };

  nativeBuildInputs = [
    nodejs
    pnpm
    pnpmConfigHook
  ]
  ++ lib.optional stdenv.hostPlatform.isLinux autoPatchelfHook
  ++ lib.optional (main != null) makeWrapper;
  buildInputs = lib.optionals stdenv.hostPlatform.isLinux [ stdenv.cc.cc.lib ];
  dontAutoPatchelf = true;

  buildPhase = ''
    runHook preBuild
    ${patchAddons}
    ( cd ${lib.escapeShellArg cwd} && pnpm run ${lib.escapeShellArg script} )
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
  ''
  + (
    if main == null then
      ''
        cp -r ${lib.escapeShellArg "${cwd}/${output}"} "$out"
      ''
    else
      # node_modules links into the workspace root, so the whole tree ships
      ''
        mkdir -p "$out/bin"
        cp -r . "$out/lib"
        makeWrapper ${lib.getExe nodejs} "$out/bin/${pname}" \
          --add-flags "$out/lib/"${lib.escapeShellArg "${cwd}/${main}"}
      ''
  )
  + ''
    runHook postInstall
  '';
}
