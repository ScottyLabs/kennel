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
    enable = lib.mkEnableOption "SQLite for local development";
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [ pkgs.sqlite ];

    env.DATABASE_PATH = "${config.devenv.root}/.devenv/state/${projectName}.db";
  };
}
