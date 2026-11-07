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

  # the dev shell's environment, which pitchfork processes also start from
  environment = pkgs.writeText "scottylabs-environment.sh" ''
    source ${paths}
    export PATH=${
      lib.escapeShellArg (lib.makeBinPath (cfg.packages ++ scripts ++ pkgs.stdenv.initialPath))
    }:"$PATH"
    ${lib.concatStrings (
      lib.mapAttrsToList (name: value: "export ${name}=${lib.escapeShellArg value}\n") cfg.env
    )}
    ${cfg.exports}
  '';

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
        source ${paths}
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
        Variables exported in the dev shell and in
        [local processes](#scottylabsprocesses). They are exported after the
        project's secrets, so they win on name collisions.
      '';
    };

    exports = lib.mkOption {
      type = lib.types.lines;
      default = "";
      description = ''
        Bash `export` lines for values built from `$SCOTTYLABS_ROOT` (the
        checkout), `$SCOTTYLABS_STATE` (`.scottylabs/` in the checkout), or
        `$SCOTTYLABS_RUNTIME` (sockets). They run after `env`, in the dev shell
        and in local processes, so keep them free of side effects.
      '';
    };

    hook = lib.mkOption {
      type = lib.types.lines;
      default = "";
      description = ''
        Bash run on shell entry, after `env` and `exports`, for setup such as
        installing dependencies. Local processes don't run it.
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

    environment = lib.mkOption {
      type = lib.types.package;
      default = environment;
      readOnly = true;
      internal = true;
      description = "Script that sets the dev shell's paths, packages, `env`, and `exports`";
    };
  };

  config.devShells.default = pkgs.mkShell {
    packages =
      cfg.packages
      ++ scripts
      ++ config.pre-commit.settings.enabledPackages
      ++ [ config.pre-commit.settings.package ];

    shellHook = ''
      source ${paths}

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
      source ${environment}
      ${cfg.hook}
      ${links}
      unset -f _scottylabs_exclude _scottylabs_link
    '';
  };
}
