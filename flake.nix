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
      inputs = {
        pyproject-nix.follows = "pyproject-nix";
        nixpkgs.follows = "nixpkgs";
      };
    };

    pyproject-build-systems = {
      url = "github:pyproject-nix/build-system-pkgs";
      inputs = {
        pyproject-nix.follows = "pyproject-nix";
        uv2nix.follows = "uv2nix";
        nixpkgs.follows = "nixpkgs";
      };
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
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
        "x86_64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};

      # devenv module set shared with downstream projects
      devenvModules.default =
        { pkgs, ... }:
        {
          imports = [ ./nix/modules ];
          _module.args.ricochet = ricochet.packages.${pkgs.stdenv.hostPlatform.system}.ricochet;
        };

      # build helpers bound to a consumer pkgs, mirroring crane.mkLib
      mkLib = pkgs: {
        buildDenoTask = pkgs.callPackage ./nix/lib/build-deno-task.nix { };
        buildRustService = import ./nix/lib/build-rust-service.nix { inherit pkgs crane; };
        buildHaskellService = import ./nix/lib/build-haskell-service.nix { inherit pkgs; };
        buildPythonService = import ./nix/lib/build-python-service.nix {
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
        buildOptionsDoc = import ./nix/lib/options-doc.nix { inherit pkgs; };
      };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          lib' = mkLib pkgs;

          kennel = lib'.buildRustService {
            src = ./.;
            pname = "kennel";
            version = "0.1.0";
            buildArgs.cargoExtraArgs = "-p kennel";
          };

          options-doc = lib'.buildOptionsDoc {
            module = devenvModules.default;
            subtree = options: options.scottylabs;
            root = ./.;
            repoUrl = "https://codeberg.org/ScottyLabs/kennel/src/branch/main";
          };

          docsGen = import ./sites/docs/generate.nix {
            inherit pkgs mkLib;
            devenvOptionsMd = options-doc;
          };

          docs = lib'.buildMdbook {
            inherit (docsGen) src;
            name = "kennel-docs";
          };
        in
        {
          inherit kennel docs options-doc;
          docs-options = docsGen.generated;
        }
      );

      nixosModules.default = import ./nix/nixos.nix;

      inherit devenvModules mkLib;

      apps = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          login = {
            type = "app";
            program = "${pkgs.writeShellScript "scottylabs-login" ''
              export BAO_ADDR=https://secrets.scottylabs.org
              ${pkgs.openbao}/bin/bao login -method=oidc
              ${pkgs.openbao}/bin/bao kv get -field=CACHIX_AUTH_TOKEN secret/shared/cachix \
                | ${pkgs.cachix}/bin/cachix authtoken --stdin
            ''}";
          };
        }
      );

      formatter = forAllSystems (
        system:
        (treefmt-nix.lib.evalModule (pkgsFor system) {
          programs = {
            nixfmt.enable = true;
            mdformat.enable = true;
            yamlfmt.enable = true;
          };
        }).config.build.wrapper
      );
    };
}
