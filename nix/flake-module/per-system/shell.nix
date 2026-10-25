{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.shell;

  # TMPDIR differs per nix develop shell, so sockets use the runtime dir
  paths = pkgs.writeText "scottylabs-paths.sh" ''
    SCOTTYLABS_ROOT=$(${lib.getExe pkgs.git} rev-parse --show-toplevel 2>/dev/null || pwd)
    export SCOTTYLABS_ROOT
    export SCOTTYLABS_STATE="$SCOTTYLABS_ROOT/.scottylabs"
    _scottylabs_id=$(printf %s "$SCOTTYLABS_ROOT" | ${pkgs.coreutils}/bin/sha256sum | ${pkgs.coreutils}/bin/cut -c1-12)
    export SCOTTYLABS_RUNTIME="''${XDG_RUNTIME_DIR:-/tmp}/scottylabs-$_scottylabs_id"
    unset _scottylabs_id
    ${pkgs.coreutils}/bin/mkdir -p "$SCOTTYLABS_STATE" "$SCOTTYLABS_RUNTIME"
  '';

  exports = lib.concatStrings (
    lib.mapAttrsToList (name: value: "export ${name}=${lib.escapeShellArg value}\n") cfg.env
  );

  links = lib.concatStrings (
    lib.mapAttrsToList (path: target: ''
      _scottylabs_link ${lib.escapeShellArg path} ${target}
    '') cfg.files
  );

  scripts = lib.mapAttrsToList (
    name: body:
    pkgs.writeShellApplication {
      inherit name;
      # the paths script is a store path shellcheck cannot follow
      excludeShellChecks = [ "SC1091" ];
      text = ''
        source ${cfg.paths}
        cd "$SCOTTYLABS_ROOT"
        ${body}
      '';
    }
  ) cfg.scripts;
in
{
  options.scottylabs.shell = {
    packages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      description = "Packages added to the dev shell";
    };

    env = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = ''
        Variables exported in the dev shell. They are exported after the
        project's secrets, so they win on name collisions.
      '';
    };

    hook = lib.mkOption {
      type = lib.types.lines;
      default = "";
      description = ''
        Bash run on shell entry, after `env`. `$SCOTTYLABS_ROOT` (the checkout),
        `$SCOTTYLABS_STATE` (`.scottylabs/` in the checkout), and
        `$SCOTTYLABS_RUNTIME` (sockets) are set.
      '';
    };

    scripts = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Commands added to the dev shell, name to bash body, run from the project root";
    };

    initHook = lib.mkOption {
      type = lib.types.lines;
      default = "";
      internal = true;
      description = "Bash run on shell entry before `env`, for exports that `env` may override";
    };

    files = lib.mkOption {
      type = lib.types.attrsOf lib.types.path;
      default = { };
      internal = true;
      description = ''
        Generated files linked into the checkout on shell entry, path relative
        to the project root to store path. Each is added to `.git/info/exclude`.
      '';
    };

    paths = lib.mkOption {
      type = lib.types.package;
      default = paths;
      readOnly = true;
      internal = true;
      description = "Script setting `$SCOTTYLABS_ROOT`, `$SCOTTYLABS_STATE`, and `$SCOTTYLABS_RUNTIME`";
    };
  };

  config.devShells.default = pkgs.mkShell {
    packages =
      cfg.packages
      ++ scripts
      ++ config.pre-commit.settings.enabledPackages
      ++ [ config.pre-commit.settings.package ];

    shellHook = ''
      source ${cfg.paths}

      _scottylabs_exclude() {
        local exclude
        exclude=$(${lib.getExe pkgs.git} -C "$SCOTTYLABS_ROOT" rev-parse --git-path info/exclude 2>/dev/null) || return 0
        [[ $exclude = /* ]] || exclude="$SCOTTYLABS_ROOT/$exclude"
        mkdir -p "$(dirname "$exclude")"
        grep -qxF "$1" "$exclude" 2>/dev/null || echo "$1" >> "$exclude"
      }

      # an indirect gc root keeps the linked store path alive until the link changes
      _scottylabs_link() {
        mkdir -p "$(dirname "$SCOTTYLABS_ROOT/$1")"
        nix-store --add-root "$SCOTTYLABS_ROOT/$1" --realise "$2" >/dev/null
        _scottylabs_exclude "/$1"
      }

      _scottylabs_exclude /.scottylabs/

      ${cfg.initHook}
      ${exports}
      ${cfg.hook}
      ${links}
      unset -f _scottylabs_exclude _scottylabs_link
    '';
  };
}
