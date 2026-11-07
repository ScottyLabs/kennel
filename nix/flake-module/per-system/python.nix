kennelInputs:
{
  lib,
  config,
  pkgs,
  ...
}:

let
  cfg = config.scottylabs.python;
  inherit (config.scottylabs) root;
  project = root + "/${cfg.dir}";

  # every locked dependency group so ty and pytest see the dev tools
  devEnv =
    let
      inherit
        (import ../../lib/python-set.nix
          {
            inherit pkgs;
            inherit (kennelInputs) uv2nix pyproject-nix pyproject-build-systems;
          }
          {
            src = project;
            python = cfg.package;
            inherit (cfg) overrides;
          }
        )
        workspace
        pythonSet
        ;
    in
    pythonSet.mkVirtualEnv "${config.scottylabs.project.name}-dev-env" workspace.deps.all;

  # pytest exits 5 when it collects no tests
  pytest = pkgs.writeShellScript "pytest" ''
    cd ${lib.escapeShellArg cfg.dir}
    python -m pytest "$@"
    rc=$?
    [ "$rc" -eq 5 ] && exit 0
    exit "$rc"
  '';
in
{
  options.scottylabs.python = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable the Python toolchain. Adds [uv](https://docs.astral.sh/uv/),
        [ruff](https://docs.astral.sh/ruff/), and
        [ty](https://github.com/astral-sh/ty). Runs `uv sync` on shell entry and
        ruff, ty, and [pytest](https://docs.pytest.org) on every commit. pytest
        must be in the project's dev dependencies.
      '';
    };

    dir = lib.mkOption {
      type = lib.types.str;
      default = ".";
      description = ''
        Directory, relative to the project root, holding the `pyproject.toml`
        and `uv.lock` that the shell syncs and the hooks check.
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.python3;
      defaultText = lib.literalExpression "pkgs.python3";
      description = ''
        The Python interpreter for the shell and the `hooks` check. Pass the
        same one as `python` to `buildPythonService`.
      '';
    };

    overrides = lib.mkOption {
      type = lib.types.raw;
      default = _final: _prev: { };
      defaultText = lib.literalExpression "_final: _prev: { }";
      description = ''
        Overlay over the uv2nix package set the `hooks` check builds, for
        dependencies that need system libraries or an undeclared build system.
        Pass the same one as `overrides` to `buildPythonService`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    scottylabs = {
      hooksCheck.setup = lib.optionalString (builtins.pathExists (project + "/uv.lock")) ''
        export VIRTUAL_ENV=${devEnv}
        export PATH="$VIRTUAL_ENV/bin:$PATH"
      '';

      shell = {
        packages = with pkgs; [
          cfg.package
          uv
          ruff
          ty
        ];

        env = {
          UV_PYTHON = lib.getExe cfg.package;
          UV_PYTHON_DOWNLOADS = "never";
        };

        exports = ''
          export UV_CACHE_DIR="$SCOTTYLABS_STATE/uv"
          if [ -f "$SCOTTYLABS_ROOT/${cfg.dir}/pyproject.toml" ]; then
            export VIRTUAL_ENV="$SCOTTYLABS_ROOT/${cfg.dir}/.venv"
            export PATH="$VIRTUAL_ENV/bin:$PATH"
          fi
        ''
        # native wheels from uv look for these outside the store
        + lib.optionalString pkgs.stdenv.isLinux ''
          export LD_LIBRARY_PATH=${
            lib.makeLibraryPath [
              pkgs.stdenv.cc.cc.lib
              pkgs.zlib
            ]
          }''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
        '';

        # every extra and group, matching the hooks check
        hook = ''
          if [ -f "$SCOTTYLABS_ROOT/${cfg.dir}/pyproject.toml" ]; then
            (cd "$SCOTTYLABS_ROOT/${cfg.dir}" && uv sync --quiet --all-extras --all-groups)
          fi
        '';
      };
    };

    treefmt.programs.ruff-format.enable = true;

    pre-commit.settings.hooks = {
      ruff.enable = true;
      ty = {
        enable = true;
        pass_filenames = false;
        args = [
          "--project"
          cfg.dir
        ];
      };
      pytest = {
        enable = true;
        name = "pytest";
        entry = "${pytest}";
        files = "\\.py$";
        pass_filenames = false;
        language = "system";
      };
    };
  };
}
