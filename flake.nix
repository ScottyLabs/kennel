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
  };

  outputs = { self, nixpkgs, devenv, ... }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
          cargoNix = pkgs.callPackage ./Cargo.nix {
            defaultCrateOverrides = pkgs.defaultCrateOverrides // {
              libsqlite3-sys = attrs: {
                nativeBuildInputs = [ pkgs.pkg-config ];
                buildInputs = [ pkgs.sqlite ];
              };
            };
          };
          kennel = cargoNix.workspaceMembers.kennel.build;

          docs = pkgs.stdenv.mkDerivation {
            pname = "kennel-docs";
            version = "0.1.0";
            src = ./sites/docs;
            nativeBuildInputs = [ pkgs.mdbook ];
            buildPhase = "mdbook build";
            installPhase = "mkdir -p $out && cp -r book/* $out/";
          };
        in
        {
          inherit kennel docs;
          default = kennel;
          devenv = devenv.packages.${system}.devenv;
        }
      );

      nixosModules.default = import ./nixos;
    };
}
