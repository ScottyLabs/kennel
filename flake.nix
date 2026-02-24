{
  description = "Kennel";

  nixConfig = {
    extra-substituters = [ "https://scottylabs.cachix.org" ];
    extra-trusted-public-keys = [
      "scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
    ];
  };

  inputs = {
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    devenv.url = "github:cachix/devenv";
    bun2nix = {
      url = "github:nix-community/bun2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, devenv, bun2nix, ... }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
          b2n = bun2nix.packages.${system}.default;
          cargoNix = pkgs.callPackage ./Cargo.nix { };
          kennel = cargoNix.workspaceMembers.kennel.build;

          kennelDocs = b2n.mkDerivation {
            pname = "kennel-docs";
            version = (builtins.fromJSON (builtins.readFile ./sites/docs/package.json)).version;
            src = ./sites/docs;

            bunDeps = b2n.fetchBunDeps {
              bunNix = ./sites/docs/bun.nix;
            };

            buildPhase = ''
              bun run build
            '';

            installPhase = ''
              mkdir -p $out
              cp -r dist/* $out/
            '';
          };

          kennelWeb = b2n.mkDerivation {
            pname = "kennel-web";
            version = (builtins.fromJSON (builtins.readFile ./sites/web/package.json)).version;
            src = ./sites/web;

            bunDeps = b2n.fetchBunDeps {
              bunNix = ./sites/web/bun.nix;
            };

            buildPhase = ''
              bun run build
            '';

            installPhase = ''
              mkdir -p $out
              cp -r dist/* $out/
            '';
          };
        in
        {
          inherit kennel kennelDocs kennelWeb;
          default = kennel;
          devenv = devenv.packages.${system}.devenv;
        }
      );

      nixosModules.default = import ./nixos;
    };
}
