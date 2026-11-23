{
  lib,
  stdenv,
  callPackage,
  deno,
  autoPatchelfHook,
  mobile,
}:

let
  denoLib = callPackage ./build-deno-task/deno.nix { };

  nodeModules =
    { src, pname }:
    let
      cache = denoLib.mkCache {
        name = pname;
        lockfile = src + "/deno.lock";
      };
    in
    {
      nativeBuildInputs = [ deno ] ++ lib.optional stdenv.hostPlatform.isLinux autoPatchelfHook;
      buildInputs = lib.optionals stdenv.hostPlatform.isLinux [ stdenv.cc.cc.lib ];
      install = ''
        export DENO_DIR="$TMPDIR/deno"
        mkdir -p "$DENO_DIR"
        cp -r ${cache}/. "$DENO_DIR"
        chmod -R u+w "$DENO_DIR"
        deno install --cached-only --frozen
        ${denoLib.patchAddons}
      '';
    };

  withNodeModules =
    build: args: build (args // { nodeModules = nodeModules { inherit (args) src pname; }; });
in
{
  buildAndroidApp = withNodeModules mobile.capacitor.buildAndroidApp;
  buildIosApp = withNodeModules mobile.capacitor.buildIosApp;
}
