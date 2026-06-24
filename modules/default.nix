{ lib, config, ... }:

let
  cfg = config.scottylabs;
in
{
  imports = [
    ./auto-update.nix
    ./claude.nix
    ./kennel.nix
    ./rust.nix
    ./deno.nix
    ./python.nix
    ./garage.nix
    ./ricochet.nix
    ./postgres.nix
    ./sqlite.nix
    ./secrets.nix
    ./valkey.nix
  ];

  options.scottylabs = {
    enable = lib.mkEnableOption "ScottyLabs shared development configuration";

    project.name = lib.mkOption {
      type = lib.types.str;
      description = "Project name, used for database naming, log filtering, and secrets path";
    };

    conventionalCommits.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enforce Conventional Commits via the commitizen git hook";
    };
  };

  config = lib.mkIf cfg.enable {
    cachix.pull = [ "scottylabs" ];

    treefmt = {
      enable = true;
      config.programs = {
        nixpkgs-fmt.enable = true;
        mdformat.enable = true;
      };
    };

    git-hooks.hooks = {
      treefmt.enable = true;
      commitizen.enable = cfg.conventionalCommits.enable;
    };
  };
}
