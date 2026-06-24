{
  description = "ScottyLabs shared devenv configuration";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    # https://github.com/NixOS/nixpkgs/pull/534873
    nixpkgs-deno.url = "github:ap-1/nixpkgs/deno-keep-denort";
    ricochet = {
      url = "git+https://codeberg.org/anish/ricochet";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    pyproject-nix = {
      url = "github:pyproject-nix/pyproject.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    uv2nix = {
      url = "github:pyproject-nix/uv2nix";
      inputs.pyproject-nix.follows = "pyproject-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    pyproject-build-systems = {
      url = "github:pyproject-nix/build-system-pkgs";
      inputs.pyproject-nix.follows = "pyproject-nix";
      inputs.uv2nix.follows = "uv2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      nixpkgs-deno,
      crane,
      ricochet,
      pyproject-nix,
      uv2nix,
      pyproject-build-systems,
      ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
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
        buildDenoTask = pkgs.callPackage ./lib/build-deno-task.nix {
          deno = nixpkgs-deno.legacyPackages.${pkgs.system}.deno;
        };
        buildRustService = import ./lib/build-rust-service.nix { inherit pkgs crane; };
        buildPythonService = import ./lib/build-python-service.nix {
          inherit
            pkgs
            uv2nix
            pyproject-nix
            pyproject-build-systems
            ;
        };
        buildMdbook =
          {
            src,
            name ? "docs",
          }:
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
