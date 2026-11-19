{
  lib,
  exists,
  flake,
  ...
}:

let
  substituter = "https://cache.scottylabs.org";
  key = "deploy-01.scottylabs.org-1:TtFRYjsx76lUXkCDvobNkTFZGOeLZFKTzdKX9oyiCcY=";

  listed = settings: lib.concatMap (setting: flake.nixConfig.${setting} or [ ]) settings;
  substituters = listed [
    "substituters"
    "extra-substituters"
  ];
  keys = listed [
    "trusted-public-keys"
    "extra-trusted-public-keys"
  ];
in
lib.optionals (exists "flake.nix") (
  lib.optional (
    !lib.elem substituter substituters
  ) "flake.nix: add \"${substituter}\" to nixConfig.extra-substituters"
  ++ lib.optional (
    !lib.elem key keys
  ) "flake.nix: add \"${key}\" to nixConfig.extra-trusted-public-keys"
)
