{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.processes;

  # TODO: https://github.com/NixOS/nixpkgs/pull/578689
  pitchfork = pkgs.pitchfork.overrideAttrs (
    final: _: {
      version = "2.32.0";
      src = pkgs.fetchFromGitHub {
        owner = "jdx";
        repo = "pitchfork";
        tag = "v${final.version}";
        hash = "sha256-QUi3fjUL2Gme0DQxLq2c1S6bxtaNqYrWLROl1ZibUF8=";
      };
      cargoHash = "sha256-ZKF3cSIkp0SUVbdsNEhXea/FDW+rjbKNL/2D0GTUacI=";
      pnpmDeps = pkgs.fetchPnpmDeps {
        inherit (final) pname version src;
        pnpm = pkgs.pnpm_10;
        sourceRoot = "${final.src.name}/ui";
        fetcherVersion = 3;
        hash = "sha256-SATk+Pd7avNDHx/Sx1/ndJtWyh1BL1e9gIoz6EXslmA=";
      };
    }
  );

  script =
    name: body:
    pkgs.writeShellScript name ''
      source ${config.scottylabs.shell.paths}
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
      `pitchfork start --local`. Scripts see `$SCOTTYLABS_ROOT`,
      `$SCOTTYLABS_STATE`, and `$SCOTTYLABS_RUNTIME`.
    '';
  };

  config = lib.mkIf (cfg != { }) {
    scottylabs.shell = {
      packages = [ pitchfork ];
      files.".config/pitchfork.toml" = toml;
    };
  };
}
