{
  description = "ScottyLabs shared devenv configuration";

  inputs = {
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    crane.url = "github:ipetkov/crane";
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

      # build helpers bound to a consumer pkgs, mirroring crane.mkLib
      mkLib = pkgs: {
        buildDenoTask = pkgs.callPackage ./lib/build-deno-task.nix { };
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

      # This authenticates to OpenBao and stores CACHIX_AUTH_TOKEN in
      # ~/.config/cachix/cachix.dhall. cachix could read directly from
      # CACHIX_AUTH_TOKEN instead, but devenv tries reading that before
      # enterShell, so we can't directly inject it there.
      # https://github.com/cachix/devenv/pull/2783 adds pre-flight env
      # injection, which lets us eliminate the extra authtoken step
      apps = forAllSystems (pkgs: {
        login = {
          type = "app";
          program = "${pkgs.writeShellScript "scottylabs-login" ''
            export BAO_ADDR=https://secrets2.scottylabs.org
            ${pkgs.openbao}/bin/bao login -method=oidc
            ${pkgs.openbao}/bin/bao kv get -field=CACHIX_AUTH_TOKEN secret/shared/cachix \
              | ${pkgs.cachix}/bin/cachix authtoken --stdin
          ''}";
        };
      });
    };
}
