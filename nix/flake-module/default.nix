kennelInputs:
{
  lib,
  self,
  inputs,
  flake-parts-lib,
  ...
}:

{
  imports = [
    kennelInputs.git-hooks.flakeModule
    kennelInputs.treefmt-nix.flakeModule
  ];

  options.perSystem = flake-parts-lib.mkPerSystemOption {
    imports = import ./per-system kennelInputs;
  };

  config = {
    transposition = {
      services = { };
      sites = { };
    };

    perSystem =
      {
        config,
        pkgs,
        system,
        ...
      }:
      {
        # above flake-parts' mkOptionDefault, below any pkgs the project sets
        _module.args.pkgs = lib.mkOverride 1400 (
          assert lib.assertMsg (
            (inputs.nixpkgs.outPath or null) == kennelInputs.nixpkgs.outPath
          ) "inputs.nixpkgs must follow kennel's nixpkgs, e.g. nixpkgs.follows = \"scottylabs/nixpkgs\"";
          inputs.nixpkgs.legacyPackages.${system}
        );

        scottylabs.root = self.outPath;

        checks = lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          kennel = config.scottylabs.kennel.manifest;
        };
      };
  };
}
