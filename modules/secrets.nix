{ pkgs, lib, config, ... }:

let
  cfg = config.scottylabs.secrets;
in
{
  options.scottylabs.secrets = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install the openbao and secretspec CLIs and set BAO_ADDR for OpenBao";
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [ pkgs.openbao pkgs.secretspec ];

    # secretspec sources per-developer secrets from a gitignored .env via its dotenv provider
    # devenv's own dotenv integration should stay off
    dotenv.enable = lib.mkForce false;
    dotenv.disableHint = true;

    # devenv resolves secretspec into config.secretspec.secrets, put them into the shell
    # mkDefault lets explicit env vars (e.g. DATABASE_URL) win on name collisions
    env = lib.mkMerge [
      { BAO_ADDR = "https://secrets2.scottylabs.org"; }
      (lib.mapAttrs (_: lib.mkDefault) config.secretspec.secrets)
    ];

    # Renew the OpenBao token on entry
    enterShell = ''
      if bao token lookup >/dev/null 2>&1; then
        bao token renew >/dev/null 2>&1 || true
      fi
    '';
  };
}
