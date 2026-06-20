{
  description = "ScottyLabs shared devenv configuration";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    ricochet = {
      url = "git+https://codeberg.org/anish/ricochet";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, crane, ricochet, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devenvModules.default = { pkgs, ... }: {
        imports = [ ./modules ];
        _module.args.ricochet = ricochet.packages.${pkgs.system}.ricochet;
      };

      # Build helpers bound to a consumer pkgs, mirroring crane.mkLib
      mkLib = pkgs: {
        buildDenoTask = pkgs.callPackage ./lib/build-deno-task.nix { };
        buildRustService = import ./lib/build-rust-service.nix { inherit pkgs crane; };
        buildMdbook =
          { src, name ? "docs" }:
          pkgs.runCommand name { nativeBuildInputs = [ pkgs.mdbook ]; } ''
            mdbook build ${src} --dest-dir "$out"
          '';
      };

      # Authenticate to OpenBao before any project shell can build
      apps = forAllSystems (pkgs: {
        login = {
          type = "app";
          program = "${pkgs.writeShellScript "bao-login" ''
            export BAO_ADDR=https://secrets2.scottylabs.org
            exec ${pkgs.openbao}/bin/bao login -method=oidc
          ''}";
        };
      });
    };
}
