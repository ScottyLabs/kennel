{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.postgres;
  projectName = config.scottylabs.project.name;
  postgres = pkgs.postgresql_18.withPackages cfg.extensions;

  sqlNames = {
    pgvector = "vector";
  };
  sqlName = pkg: sqlNames.${lib.getName pkg} or (lib.getName pkg);
in
{
  options.scottylabs.postgres = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Run PostgreSQL 18 locally over a Unix socket, with a database named
        after `scottylabs.project.name`, and export `DATABASE_URL`. Kennel
        provisions a database for each deployment.
      '';
    };

    extensions = lib.mkOption {
      type = lib.types.functionTo (lib.types.listOf lib.types.package);
      default = e: [ e.pg_uuidv7 ];
      defaultText = lib.literalExpression "e: [ e.pg_uuidv7 ]";
      description = "PostgreSQL extensions as a function of the extensions set, used locally and in deployed databases";
    };
  };

  config = lib.mkIf cfg.enable {
    scottylabs = {
      shell = {
        packages = [ postgres ];
        hook = ''
          export DATABASE_URL="postgresql:///${projectName}?host=$SCOTTYLABS_RUNTIME/postgres"
        '';
      };

      processes = {
        postgres = {
          run = ''
            data="$SCOTTYLABS_STATE/postgres"
            mkdir -p "$SCOTTYLABS_RUNTIME/postgres"
            if [ ! -s "$data/PG_VERSION" ]; then
              ${postgres}/bin/initdb --no-instructions --auth=trust -D "$data"
            fi
            exec ${postgres}/bin/postgres -D "$data" -k "$SCOTTYLABS_RUNTIME/postgres" -c listen_addresses=""
          '';
          ready = ''${postgres}/bin/pg_isready -q -h "$SCOTTYLABS_RUNTIME/postgres" -d postgres'';
          stopSignal = "SIGINT";
        };

        postgres-setup = {
          run = ''
            host="$SCOTTYLABS_RUNTIME/postgres"
            if [ -z "$(${postgres}/bin/psql -h "$host" -d postgres -tAc "SELECT 1 FROM pg_database WHERE datname = '${projectName}'")" ]; then
              ${postgres}/bin/createdb -h "$host" ${lib.escapeShellArg projectName}
            fi
          '';
          depends = [ "postgres" ];
          oneshot = true;
        };
      };

      kennel = {
        resources = [ "postgres" ];
        postgresExtensions = map sqlName (cfg.extensions pkgs.postgresql_18.pkgs);
      };
    };
  };
}
