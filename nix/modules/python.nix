{
  pkgs,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.python;
  # uv sync needs a pyproject.toml; skip it while bootstrapping the project
  hasPyproject = builtins.pathExists /${config.devenv.root}/pyproject.toml;
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
        ruff and ty on every commit.
      '';
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = with pkgs; [
      ruff
      ty
    ];

    env.UV_CACHE_DIR = "${config.devenv.root}/.devenv/state/uv";

    languages.python = {
      enable = true;
      lsp.enable = false; # disable pyright in favor of ty
      venv.enable = true;
      uv = {
        enable = true;
        sync.enable = hasPyproject;
      };
    };

    treefmt.config.programs.ruff-format.enable = true;

    git-hooks.hooks = {
      ruff.enable = true; # lint
      ty = {
        enable = true;
        # check the whole project, not just staged files
        pass_filenames = false;
      };
    };
  };
}
