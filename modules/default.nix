{
  lib,
  pkgs,
  config,
  ...
}:

let
  cfg = config.scottylabs;
in
{
  imports = [
    ./auto-update.nix
    ./claude.nix
    ./kennel.nix
    ./rust.nix
    ./deno.nix
    ./python.nix
    ./garage.nix
    ./ricochet.nix
    ./postgres.nix
    ./sqlite.nix
    ./secrets.nix
    ./valkey.nix
  ];

  options.scottylabs = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable the shared ScottyLabs development configuration. Required for all
        other `scottylabs.*` options to take effect. Also installs always-on
        hooks: two that refresh and stage `flake.lock` (`nix flake update`) and
        `devenv.lock` (`devenv update`) on every commit, one that rejects
        commits carrying AI tool co-author trailers, and one that scans staged
        changes for secrets via
        [gitleaks](https://github.com/gitleaks/gitleaks).
      '';
    };

    project.name = lib.mkOption {
      type = lib.types.str;
      description = "Used for database naming, log filtering, and secrets path resolution";
    };

    conventionalCommits.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Enforce [Conventional Commits](https://www.conventionalcommits.org/) on
        `git commit` via the commitizen git hook. Commit messages that do not
        match the conventional format are rejected at commit time.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    cachix = {
      pull = [ "scottylabs" ];
      push = "scottylabs";
    };

    treefmt = {
      enable = true;
      config.programs = {
        nixfmt.enable = true;
        mdformat.enable = true;
      };
    };

    git-hooks.hooks = {
      treefmt.enable = true;
      commitizen.enable = cfg.conventionalCommits.enable;

      gitleaks = {
        enable = true;
        name = "gitleaks";
        entry = "${pkgs.gitleaks}/bin/gitleaks git --pre-commit --staged --redact";
        pass_filenames = false;
        language = "system";
      };

      block-ai-coauthors = {
        enable = true;
        name = "block-ai-coauthors";
        entry = "${pkgs.writeShellScript "block-ai-coauthors" ''
          if grep -iqE '^[[:space:]]*co-authored-by:.*(claude|cursor|copilot|codex)' "$1"; then
            echo "AI tool co-author trailers are not allowed. Remove the Co-authored-by line." >&2
            exit 1
          fi
        ''}";
        stages = [ "commit-msg" ];
      };
    };
  };
}
