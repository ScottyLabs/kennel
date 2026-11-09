kennelInputs:
{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.processes;

  pitchfork =
    kennelInputs.nixpkgs-pitchfork.legacyPackages.${pkgs.stdenv.hostPlatform.system}.pitchfork;

  script =
    name: body:
    pkgs.writeShellScript name ''
      source ${config.scottylabs.shell.environment}
      cd "$SCOTTYLABS_ROOT"
      ${body}
    '';

  daemon =
    name: p:
    {
      run = [ (script name p.run) ];
      inherit (p) oneshot;
    }
    // lib.optionalAttrs (p.depends != [ ]) { inherit (p) depends; }
    // lib.optionalAttrs (p.ready != null) {
      ready_cmd = {
        run = [ (script "${name}-ready" p.ready) ];
        timeout = "60s";
      };
    }
    // lib.optionalAttrs (p.stopSignal != null) { stop_signal = p.stopSignal; };

  toml = (pkgs.formats.toml { }).generate "pitchfork.toml" {
    daemons = lib.mapAttrs daemon cfg;
  };
in
{
  options.scottylabs.processes = lib.mkOption {
    type = lib.types.attrsOf (
      lib.types.submodule {
        options = {
          run = lib.mkOption {
            type = lib.types.str;
            description = "Bash that runs the process in the foreground, from the project root";
          };

          ready = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            description = "Bash that exits 0 once the process accepts work";
          };

          depends = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [ ];
            description = "Processes that must be ready, or completed for a oneshot, before this one starts";
          };

          oneshot = lib.mkOption {
            type = lib.types.bool;
            default = false;
            description = "Whether `run` is a task that exits on success, such as creating a database";
          };

          stopSignal = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            description = "Signal sent to stop the process, `SIGTERM` when unset";
          };
        };
      }
    );
    default = { };
    description = ''
      Local services started by [pitchfork](https://pitchfork.jdx.dev) with
      `pitchfork start --local`. Each script runs from the project root with the
      dev shell's packages, `env`, and `exports`, but without its secrets, so a
      process that needs them runs under `secretspec run`.
    '';
  };

  config = lib.mkIf (cfg != { }) {
    scottylabs.shell = {
      packages = [ pitchfork ];
      files.".config/pitchfork.toml" = toml;
    };
  };
}
