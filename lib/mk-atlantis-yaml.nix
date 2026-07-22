{ pkgs }:

# One Atlantis project per terranix configuration
{
  terranixConfigurations,
  exclude ? [ ],
  whenModified ? [
    "modules/**"
    "flake.nix"
    "flake.lock"
  ],
}:
pkgs.writeText "atlantis.yaml" (
  builtins.toJSON {
    version = 3;
    parallel_plan = true;
    parallel_apply = true;
    projects = pkgs.lib.mapAttrsToList (name: _: {
      inherit name;
      dir = ".";
      workspace = name;
      autoplan.when_modified = whenModified;
    }) (pkgs.lib.removeAttrs terranixConfigurations exclude);
  }
)
