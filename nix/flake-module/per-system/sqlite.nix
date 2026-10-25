{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.sqlite;
  projectName = config.scottylabs.project.name;
in
{
  options.scottylabs.sqlite.enable = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = ''
      Add the `sqlite` package and export `DATABASE_PATH`, a database file under
      `.scottylabs/`. Kennel provisions a database file for each deployment.
    '';
  };

  config = lib.mkIf cfg.enable {
    scottylabs = {
      shell = {
        packages = [ pkgs.sqlite ];
        hook = ''
          export DATABASE_PATH="$SCOTTYLABS_STATE/${projectName}.db"
        '';
      };

      kennel.resources = [ "sqlite" ];
    };
  };
}
