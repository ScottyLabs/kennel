{
  lib,
  exists,
  readTOML,
  root,
  ...
}:

let
  spec = readTOML "secretspec.toml";

  providerLists = [
    (spec.defaults.providers or [ ])
  ]
  ++ map (entry: entry.providers or [ ]) (
    lib.filter builtins.isAttrs (lib.concatMap lib.attrValues (lib.attrValues (spec.profiles or { })))
  );
  usesLocal = lib.any (lib.elem "local") providerLists;

  ignored = lib.optionals (exists ".gitignore") (
    lib.splitString "\n" (builtins.readFile "${root}/.gitignore")
  );
  ignoresEnv = lib.any (
    pattern:
    lib.elem (lib.trim pattern) [
      ".env"
      "/.env"
      ".env*"
      "/.env*"
    ]
  ) ignored;
in
lib.optionals (exists "secretspec.toml" && usesLocal) (
  lib.optional (!ignoresEnv) ".gitignore: add .env"
  ++ lib.optional (
    !exists ".env.example"
  ) ".env.example: list each secret that uses the local provider"
)
