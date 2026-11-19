{
  lib,
  config,
  cfg,
  injected,
  ...
}:

let
  dnsLabel = name: builtins.match "[a-z0-9]([a-z0-9-]*[a-z0-9])?" name != null;
  labelRule = "must be lowercase letters, digits, and inner hyphens";

  forTargets =
    kinds: f:
    lib.concatMap (
      kind: lib.concatLists (lib.mapAttrsToList (name: f "${kind}.${name}" name) config.${kind})
    ) kinds;

  service =
    at: _: service:
    lib.optional (
      !service.package.meta ? mainProgram
    ) "${at}: set package.meta.mainProgram to the binary kennel should start"
    ++ map (variable: "${at}.env: remove ${variable} (set by kennel)") (
      lib.intersectLists injected (lib.attrNames service.env)
    );

  hostname =
    at: name: _:
    let
      longest = 63 - lib.stringLength "${cfg.project.name}--pr-00000";
    in
    lib.optional (!dnsLabel name) "${at}: name ${labelRule}"
    ++ lib.optional (
      lib.stringLength name > longest
    ) "${at}: name must be at most ${toString longest} characters";

  checkNames =
    at: _: target:
    map (check: "${at}.checks.${check}: kennel already defines ${check}") (
      lib.intersectLists (lib.attrNames (target.package.passthru.checks or { })) (
        lib.attrNames target.checks
      )
    );

  deploys = config.services != { } || config.sites != { };
in
lib.optional (
  !dnsLabel cfg.project.name
) "scottylabs.project.name: \"${cfg.project.name}\" ${labelRule}"
++ lib.optional (
  deploys && cfg.ci.system != "x86_64-linux"
) "scottylabs.ci.system: must be x86_64-linux"
++ forTargets [ "services" ] service
++ forTargets [ "services" "sites" ] hostname
++ forTargets [ "services" "sites" "tools" ] checkNames
