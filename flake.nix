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
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: import nixpkgs {
        inherit system;
        overlays = [ bun2nix.overlays.default ];
      };
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
          cargoNix = pkgs.callPackage ./Cargo.nix {
            defaultCrateOverrides = pkgs.defaultCrateOverrides // {
              libdbus-sys = attrs: {
                nativeBuildInputs = [ pkgs.pkg-config ];
                buildInputs = [ pkgs.dbus ];
              };
            };
          };
          kennel = cargoNix.workspaceMembers.kennel.build;

          docs = pkgs.stdenv.mkDerivation {
            pname = "kennel-docs";
            version = "0.1.0";
            src = ./sites/docs;
            nativeBuildInputs = [ pkgs.mdbook ];

            buildPhase = ''
              mdbook build
            '';

            installPhase = ''
              mkdir -p $out
              cp -r book/* $out/
            '';
          };

          web = pkgs.bun2nix.mkDerivation {
            pname = "kennel-web";
            version = (builtins.fromJSON (builtins.readFile ./sites/web/package.json)).version;
            src = ./sites/web;

            bunDeps = pkgs.bun2nix.fetchBunDeps {
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
          inherit kennel docs web;
          default = kennel;
          devenv = devenv.packages.${system}.devenv;
        }
      );

      nixosModules.default = import ./nixos;
    };
}
