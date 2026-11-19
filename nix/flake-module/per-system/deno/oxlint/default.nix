{
  pkgs,
  kennelInputs,
  react,
}:

let
  inherit (kennelInputs) anti-slop;
  inherit (kennelInputs.nixpkgs-oxlint.legacyPackages.${pkgs.stdenv.hostPlatform.system}) oxlint;
  plugins = (pkgs.lib.importJSON ./package-lock.json).packages."node_modules/@oxlint/plugins".version;

  # node resolves anti-slop's imports from the node_modules beside it
  bundle = pkgs.importNpmLock.buildNodeModules {
    npmRoot = ./.;
    nodejs = pkgs.nodejs_24;
    derivationArgs = {
      # landmines' oxlint peer dependency would duplicate nixpkgs' oxlint
      npmFlags = [ "--legacy-peer-deps" ];
      postInstall = ''
        cp ${pkgs.callPackage ./config.nix { inherit anti-slop react; }} "$out/oxlintrc.json"
        cp -r ${anti-slop}/src "$out/anti-slop"
        chmod -R u+w "$out/anti-slop"
        find "$out/anti-slop" -name '*.test.ts' -delete
      '';
    };
  };
in

assert pkgs.lib.assertMsg (plugins == oxlint.version)
  "@oxlint/plugins is ${plugins} but oxlint is ${oxlint.version}; update deno/oxlint/package.json and its lock";

{
  inherit bundle;

  # requires the repo's oxlint config to extend the bundle's
  wrapper = pkgs.writeShellApplication {
    name = "oxlint";
    runtimeInputs = [
      oxlint
      pkgs.jq
    ];
    runtimeEnv.KENNEL_OXLINT_CONFIG = "${bundle}/oxlintrc.json";
    text = builtins.readFile ./oxlint.sh;
  };
}
