kennelInputs:
{
  lib,
  pkgs,
  ...
}:

{
  options.scottylabs = {
    project.name = lib.mkOption {
      type = lib.types.str;
      description = ''
        The project's governance slug. Used for database naming, log filtering,
        and secrets path resolution, and must match `[project] name` in
        `secretspec.toml`.
      '';
    };

    root = lib.mkOption {
      type = lib.types.path;
      internal = true;
      description = "The project's flake source, set by the flake module";
    };

    conventionalCommits.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Enforce [Conventional Commits](https://www.conventionalcommits.org/)
        via the commitizen git hook.
      '';
    };

    editorconfig.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enforce [EditorConfig](https://editorconfig.org/) rules via the
        editorconfig-checker git hook.
      '';
    };
  };

  config._module.args.helpers = import ../../lib { inherit pkgs kennelInputs; };
}
