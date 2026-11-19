{
  lib,
  config,
  cfg,
  exists,
  readTOML,
  injected,
  ...
}:

let
  spec = readTOML "secretspec.toml";
  profiles = spec.profiles or { };
  declared = lib.unique (lib.concatMap lib.attrNames (lib.attrValues profiles));
  public = spec.scopes.public.secrets or [ ];

  deploys = config.services != { } || (config.sites != { } && spec ? scopes.public);
  environments =
    lib.optionals deploys (
      [
        "prod"
        "staging"
      ]
      ++ lib.optional cfg.previewDeployments "preview"
    )
    ++ [ "dev" ];
  missingProfiles = lib.subtractLists (lib.attrNames profiles) environments;

  unprefixed = lib.filter (name: !lib.hasPrefix "PUBLIC_" name) public;
  unlisted = lib.filter (name: lib.hasPrefix "PUBLIC_" name && !lib.elem name public) declared;
in
lib.optionals (exists "secretspec.toml") (
  lib.optional (spec.project.name or null != cfg.project.name)
    "secretspec.toml: [project] name must be \"${cfg.project.name}\", matching scottylabs.project.name"
  ++ lib.optional (
    spec.project.revision or null != "1.0"
  ) "secretspec.toml: [project] revision must be \"1.0\""
  ++ map (profile: "secretspec.toml: add a [profiles.${profile}] header") missingProfiles
  ++ map (
    name: "secretspec.toml: [scopes.public] names must start with PUBLIC_, found ${name}"
  ) unprefixed
  ++ map (name: "secretspec.toml: ${name} starts with PUBLIC_ but isn't in [scopes.public]") unlisted
  ++ map (name: "secretspec.toml: remove ${name} (set by kennel)") (
    lib.intersectLists injected declared
  )
  ++ lib.optional (
    lib.elem "OIDC_CLIENT_ID" declared && !cfg.ricochet.enable
  ) "scottylabs.ricochet.enable: must be true for projects using OIDC"
)
