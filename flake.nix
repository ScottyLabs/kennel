{
  description = "Kennel";

  nixConfig = {
    extra-substituters = [ "https://cache.scottylabs.org" ];
    extra-trusted-public-keys = [
      "deploy-01.scottylabs.org-1:TtFRYjsx76lUXkCDvobNkTFZGOeLZFKTzdKX9oyiCcY="
    ];
  };

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    # TODO: https://github.com/NixOS/nixpkgs/pull/585360
    nixpkgs-garage.url = "github:ap-1/nixpkgs/garage-darwin";
    # TODO: https://github.com/NixOS/nixpkgs/pull/589662
    nixpkgs-oxlint.url = "github:nixos/nixpkgs/01e3092a9cb8eadf1834a1aac8a9d6d3b03dba5b";
    # TODO: https://github.com/NixOS/nixpkgs/pull/589760
    nixpkgs-tsrs.url = "github:ap-1/nixpkgs/13c76696c889a751f83ba59fe6f74b7bb51b79dd";

    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };

    crane.url = "github:ipetkov/crane";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    git-hooks = {
      url = "github:cachix/git-hooks.nix";
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

    collar = {
      url = "git+https://git.cmu.dev/ScottyLabs/collar";
      # collar's kennel input is this flake
      inputs.scottylabs.follows = "";
    };

    # oxlint plugin with no npm release
    anti-slop = {
      url = "github:dmmulroy/anti-slop";
      flake = false;
    };
  };

  outputs =
    inputs@{ flake-parts, ... }:
    let
      flakeModule = import ./nix/flake-module inputs;
    in
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ flakeModule ];

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      flake = {
        flakeModules.default = flakeModule;
        nixosModules.default = import ./nix/nixos.nix;
      };

      perSystem =
        {
          pkgs,
          helpers,
          system,
          ...
        }:
        let
          kennel = helpers.buildRustService {
            src = ./.;
            pname = "kennel";
            version = "0.1.0";
            buildArgs.cargoExtraArgs = "--locked -p kennel";
          };

          docsGen = import ./sites/docs/generate.nix { inherit pkgs helpers inputs; };
        in
        {
          scottylabs = {
            project.name = "kennel";
            rust.enable = true;
            sqlite.enable = true;

            shell = {
              packages = [
                pkgs.sea-orm-cli
                pkgs.zensical
              ];
              scripts = {
                server = "cargo run -p kennel";
                migration = ''sea-orm-cli migrate generate "$1" -d crates/migration'';
                migrate = ''DATABASE_URL="sqlite://$DATABASE_PATH?mode=rwc" sea-orm-cli migrate up -d crates/migration'';
                generate-entities = ''DATABASE_URL="sqlite://$DATABASE_PATH" sea-orm-cli generate entity -o crates/entity/src --with-serde both --lib'';
                docs = ''
                  nix build .#docs-options --out-link sites/docs/src/reference/generated
                  cd sites/docs && zensical serve
                '';
              };
            };
          };

          # mdformat escapes the < in snippet markers
          treefmt.programs.mdformat.excludes = [
            "sites/docs/src/reference/flake-options.md"
            "sites/docs/src/reference/nixos-module.md"
          ];

          sites.docs = {
            package = helpers.buildZensical {
              inherit (docsGen) src;
              name = "kennel-docs";
            };
            customDomain = "docs.kennel.scottylabs.org";
          };

          packages = {
            inherit kennel;
            default = kennel;
            docs-options = docsGen.generated;
          };

          checks = pkgs.lib.mapAttrs' (name: pkgs.lib.nameValuePair "fixture-${name}") (
            import ./nix/fixture { inherit inputs flakeModule system; }
          );

          tools.kennel.package = kennel;

          apps.login = {
            type = "app";
            program = pkgs.lib.getExe (
              pkgs.writeShellApplication {
                name = "scottylabs-login";
                runtimeInputs = [ pkgs.openbao ];
                text = ''
                  export BAO_ADDR=https://secrets.scottylabs.org
                  bao login -method=oidc
                '';
              }
            );
          };
        };
    };
}
