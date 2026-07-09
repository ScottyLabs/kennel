{
  description = "Kennel";

  nixConfig = {
    extra-substituters = [ "https://scottylabs.cachix.org" ];
    extra-trusted-public-keys = [
      "scottylabs.cachix.org-1:hajjEX5SLi/Y7yYloiXTt2IOr3towcTGRhMh1vu6Tjg="
    ];
  };

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    devenv.url = "github:cachix/devenv";
    scottylabs = {
      url = "git+https://codeberg.org/ScottyLabs/devenv";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      devenv,
      scottylabs,
      ...
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
        "x86_64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;

          kennel = (scottylabs.mkLib pkgs).buildRustService {
            src = ./.;
            pname = "kennel";
            version = "0.1.0";
            buildArgs.cargoExtraArgs = "-p kennel";
          };

          docsGen = import ./sites/docs/generate.nix { inherit pkgs scottylabs; };

          docs = (scottylabs.mkLib pkgs).buildMdbook {
            src = docsGen.src;
            name = "kennel-docs";
          };
        in
        {
          inherit kennel docs;
          docs-options = docsGen.generated;
          default = kennel;
          devenv = devenv.packages.${system}.devenv;
        }
      );

      nixosModules.default = import ./nixos;
    };
}
