{
  lib,
  config,
  system,
  ...
}:

let
  targetChecks = lib.concatMapAttrs (
    name: target:
    lib.mapAttrs' (check: lib.nameValuePair "${name}-${check}") (target.package.passthru.checks or { })
  ) (config.services // config.sites);
in
{
  checks = lib.mkMerge [
    { devShell = config.devShells.default; }
    (lib.mkIf (system == config.scottylabs.ci.system) targetChecks)
  ];

  # Run on demand with `nix build .#<service>-mutants`
  packages = lib.concatMapAttrs (
    name: target:
    lib.optionalAttrs (target.package.passthru ? mutants) {
      "${name}-mutants" = target.package.passthru.mutants;
    }
  ) config.services;
}
