{
  description = "ScottyLabs shared devenv configuration";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
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
      treefmt-nix,
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
      devenvModules.default =
        { pkgs, ... }:
        {
          imports = [ ./modules ];
          _module.args.ricochet = ricochet.packages.${pkgs.stdenv.hostPlatform.system}.ricochet;
        };

      # build helpers bound to a consumer pkgs, mirroring crane.mkLib
      mkLib = pkgs: {
        buildDenoTask = pkgs.callPackage ./lib/build-deno-task.nix { };
        buildRustService = import ./lib/build-rust-service.nix { inherit pkgs crane; };
        buildHaskellService = import ./lib/build-haskell-service.nix { inherit pkgs; };
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
        buildOptionsDoc = import ./lib/options-doc.nix { inherit pkgs; };
      };
    in
    {
      inherit devenvModules mkLib;

      packages = forAllSystems (pkgs: {
        options-doc = (mkLib pkgs).buildOptionsDoc {
          module = devenvModules.default;
          subtree = options: options.scottylabs;
          root = ./.;
          repoUrl = "https://codeberg.org/ScottyLabs/devenv/src/branch/main";
        };
      });

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
            export BAO_ADDR=https://secrets.scottylabs.org
            ${pkgs.openbao}/bin/bao login -method=oidc
            ${pkgs.openbao}/bin/bao kv get -field=CACHIX_AUTH_TOKEN secret/shared/cachix \
              | ${pkgs.cachix}/bin/cachix authtoken --stdin
          ''}";
        };
      });

      formatter = forAllSystems (
        pkgs:
        (treefmt-nix.lib.evalModule pkgs {
          programs.nixfmt.enable = true;
          programs.mdformat.enable = true;
          programs.yamlfmt.enable = true;
        }).config.build.wrapper
      );
    };
}
