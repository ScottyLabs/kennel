{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.sqlite;
  projectName = config.scottylabs.project.name;
in
{
  options.scottylabs.sqlite = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable SQLite for local development. Adds the `sqlite` package and
        exports `DATABASE_PATH` pointing to a database file in the devenv state
        directory.
      '';
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [ pkgs.sqlite ];

    env.DATABASE_PATH = "${config.devenv.root}/.devenv/state/${projectName}.db";
  };
}
