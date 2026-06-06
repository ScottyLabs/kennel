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
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, devenv, crane, ... }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
          craneLib = crane.mkLib pkgs;

          commonArgs = {
            pname = "kennel";
            version = "0.1.0";
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          kennel = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            cargoExtraArgs = "-p kennel";
            doCheck = false;
          });

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
