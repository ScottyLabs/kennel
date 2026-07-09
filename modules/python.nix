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
        Enable the Python development toolchain. Manages a virtual environment
        with [uv](https://docs.astral.sh/uv/) (running `uv sync` on shell entry
        once a `pyproject.toml` exists), adds [ruff](https://docs.astral.sh/ruff/)
        (lint pre-commit hook, formatting via treefmt's `ruff-format`) and
        [ty](https://github.com/astral-sh/ty) (type-check pre-commit hook) on
        `PATH`.
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
      # TODO: https://github.com/cachix/git-hooks.nix/pull/724
      ty = {
        enable = true;
        name = "ty";
        entry = "${pkgs.ty}/bin/ty check";
        files = "\\.pyi?$";
        pass_filenames = false;
        language = "system";
      };
    };
  };
}
