{
  lib,
  exists,
  ...
}:

lib.optional (!exists ".forgejo/workflows/ci.yml") ".forgejo/workflows/ci.yml: missing"
