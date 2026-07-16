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
    ./claude.nix
    ./kennel.nix
    ./rust.nix
    ./deno.nix
    ./haskell.nix
    ./python.nix
    ./garage.nix
    ./ricochet.nix
    ./postgres.nix
    ./sqlite.nix
    ./secrets.nix
    ./security.nix
    ./valkey.nix
  ];

  options.scottylabs = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Enable the shared ScottyLabs development configuration. Required for all
        other `scottylabs.*` options to take effect, and installs the formatters
        and commit-time checks shared across ScottyLabs projects.
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
      commitizen = {
        enable = cfg.conventionalCommits.enable;
        package = pkgs.commitizen;
      };

      # TODO: https://github.com/cachix/git-hooks.nix/pull/642
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

      block-ai-slop = {
        enable = true;
        name = "block-ai-slop";
        entry = "${pkgs.writeShellScript "block-ai-slop" ''
          [ "$#" -eq 0 ] && exit 0
          if ${pkgs.ripgrep}/bin/rg --color=never -n '[‒–—―‘’“”‚„…•‣←↑→↓↔⇐⇒⇔➜➡✓✔✅☑✗✘✕✖❌]' "$@"; then
            echo "Non-ASCII characters are not allowed (em and en dashes, smart quotes, ellipsis, arrows, check and x marks, bullets). Replace them with plain ASCII." >&2
            exit 1
          fi
        ''}";
        types = [ "text" ];
        language = "system";
      };
    };
  };
}
