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
        { pkgs, helpers, ... }:
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
            rust = {
              enable = true;
              cranelift.excludePackages = [
                "aws-lc-sys"
                "aws-lc-rs"
                "rustls"
                "linkme"
              ];
            };
            sqlite.enable = true;

            shell = {
              packages = [ pkgs.sea-orm-cli ];
              scripts = {
                server = "cargo run -p kennel";
                migration = ''sea-orm-cli migrate generate "$1" -d crates/migration'';
                migrate = ''DATABASE_URL="sqlite://$DATABASE_PATH?mode=rwc" sea-orm-cli migrate up -d crates/migration'';
                generate-entities = ''DATABASE_URL="sqlite://$DATABASE_PATH" sea-orm-cli generate entity -o crates/entity/src --with-serde both --lib'';
                docs = ''
                  nix build .#docs-options --out-link sites/docs/src/reference/generated
                  cd sites/docs && mdbook serve
                '';
              };
            };
          };

          sites.docs = {
            package = helpers.buildMdbook {
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

          checks = kennel.passthru.checks;

          apps.login = {
            type = "app";
            program = "${pkgs.writeShellScript "scottylabs-login" ''
              export BAO_ADDR=https://secrets.scottylabs.org
              ${pkgs.openbao}/bin/bao login -method=oidc
            ''}";
          };
        };
    };
}
