{
  lib,
  config,
  ...
}:

let
  cfg = config.scottylabs;
  root = "${cfg.root}";

  resourceVariables = {
    postgres = [
      "DATABASE_URL"
      "PGUSER"
    ];
    sqlite = [ "DATABASE_PATH" ];
    valkey = [ "VALKEY_URL" ];
    garage = [
      "S3_ENDPOINT"
      "S3_REGION"
      "S3_BUCKET"
      "S3_ACCESS_KEY"
      "S3_SECRET_KEY"
    ];
  };

  context = {
    inherit
      lib
      config
      cfg
      root
      ;

    exists = file: builtins.pathExists "${root}/${file}";
    readJSON =
      file:
      builtins.addErrorContext "while reading ${file}, which kennel reads as JSON without comments" (
        lib.importJSON "${root}/${file}"
      );
    readTOML = file: lib.importTOML "${root}/${file}";
    flake = import "${root}/flake.nix";
    files = map (lib.removePrefix "${root}/") (lib.filesystem.listFilesRecursive root);

    injected = [
      "PORT"
      "COMMIT_HASH"
    ]
    ++ lib.concatMap (resource: resourceVariables.${resource} or [ ]) cfg.kennel.resources;
  };

  violations = lib.concatMap (file: import file context) [
    ./secretspec.nix
    ./local.nix
    ./targets.nix
    ./cache.nix
    ./ci.nix
    ./postgres.nix
    ./deno.nix
    ./rust.nix
  ];
in
{
  options.scottylabs.conventions.enforce = lib.mkOption {
    type = lib.types.functionTo lib.types.raw;
    readOnly = true;
    internal = true;
    description = "Returns its argument, or fails listing every convention the project breaks";
    default =
      value:
      if violations == [ ] then
        value
      else
        throw "the project breaks kennel's conventions:\n${
          lib.concatMapStrings (violation: "- ${violation}\n") violations
        }";
  };
}
