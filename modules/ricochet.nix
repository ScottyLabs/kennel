{
  ricochet,
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs.ricochet;

  # fixed so governance can register one dev redirect for every app
  port = 8090;
in
{
  options.scottylabs.ricochet = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Run a local OAuth relay on `127.0.0.1:8090` for development, the
        loopback callback the `dev` Keycloak client is registered against.
        Enable it alongside `oidc_client` to complete an OAuth flow locally the
        way production does: the IdP redirects to the relay, which forwards the
        authorization code on to your service's own callback.
      '';
    };

    appUrl = lib.mkOption {
      type = lib.types.str;
      description = ''
        Public URL the service is reached at in local development, exported as
        `APP_URL` so it can build OAuth redirect targets and absolute links.
        This is the development value only; deployed environments receive
        `APP_URL` from kennel, derived from the deployment domain (see
        [Deploying a Project](../guides/deploying.md#runtime-environment)), so
        it is never declared in `secretspec.toml`.
      '';
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    processes.ricochet.exec = "RICOCHET_DEV=1 RICOCHET_BIND=127.0.0.1:${toString port} ${ricochet}/bin/ricochet";
    env.APP_URL = cfg.appUrl;
  };
}
