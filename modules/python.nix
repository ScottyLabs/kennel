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
    enable = lib.mkEnableOption "Python development toolchain (uv, ruff, ty)";
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
