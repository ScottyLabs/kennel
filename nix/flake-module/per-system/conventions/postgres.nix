{
  lib,
  cfg,
  ...
}:

let
  trusted = [
    "pg_uuidv7"
    "vector"
    "postgis"
  ];
in
map (
  extension:
  "scottylabs.postgres.extensions: kennel's hosts don't trust ${extension}, only pg_uuidv7, pgvector, and postgis"
) (lib.subtractLists trusted cfg.kennel.postgresExtensions)
