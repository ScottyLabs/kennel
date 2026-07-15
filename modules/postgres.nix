{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.postgres;
  projectName = config.scottylabs.project.name;
in
{
  options.scottylabs.postgres = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable a local PostgreSQL 18 instance with Unix socket access. Creates
        an initial database named after `scottylabs.project.name` and exports
        `DATABASE_URL` into the shell environment.
      '';
    };

    extensions = lib.mkOption {
      type = lib.types.functionTo (lib.types.listOf lib.types.package);
      default = e: [ e.pg_uuidv7 ];
      defaultText = lib.literalExpression "e: [ e.pg_uuidv7 ]";
      description = "PostgreSQL extensions as a function of the extensions set";
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [ pkgs.postgresql_18 ];

    services.postgres = {
      enable = true;
      package = pkgs.postgresql_18;
      extensions = cfg.extensions;
      listen_addresses = "";
      initialDatabases = [
        { name = projectName; }
      ];
    };

    enterShell = ''
      export DATABASE_URL="postgresql:///${projectName}?host=$DEVENV_RUNTIME/postgres"
    '';

    scottylabs.kennel.requestedResources = [ "postgres" ];
  };
}
