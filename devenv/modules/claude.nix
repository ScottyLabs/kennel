{ lib, config, ... }:

let
  cfg = config.scottylabs.claude;
in
{
  options.scottylabs.claude = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Enable Claude Code integration. Generates the `.mcp.json` configuration
        with the devenv MCP server.
      '';
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    claude.code.enable = true;
  };
}
