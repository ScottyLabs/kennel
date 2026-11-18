{ pkgs, anti-slop }:

rec {
  # node resolves anti-slop's imports from the node_modules beside it
  bundle = pkgs.importNpmLock.buildNodeModules {
    npmRoot = ./.;
    nodejs = pkgs.nodejs_24;
    derivationArgs = {
      # landmines' oxlint peer dependency would duplicate nixpkgs' oxlint
      npmFlags = [ "--legacy-peer-deps" ];
      postInstall = ''
        cp ${pkgs.callPackage ./config.nix { inherit anti-slop; }} "$out/oxlintrc.json"
        cp -r ${anti-slop}/src "$out/anti-slop"
        chmod -R u+w "$out/anti-slop"
        find "$out/anti-slop" -name '*.test.ts' -delete
      '';
    };
  };

  # without a project config, lint with the bundle's
  wrapper = pkgs.writeShellApplication {
    name = "oxlint";
    runtimeInputs = [ pkgs.oxlint ];
    text = ''
      for config in .oxlintrc.json .oxlintrc.jsonc oxlint.config.ts oxlint.config.mts; do
        if [ -e "$config" ]; then
          exec oxlint "$@"
        fi
      done
      exec oxlint --config ${bundle}/oxlintrc.json "$@"
    '';
  };
}
