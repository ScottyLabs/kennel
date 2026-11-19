{
  lib,
  cfg,
  exists,
  ...
}:

let
  toolchains = lib.filter exists [
    "rust-toolchain"
    "rust-toolchain.toml"
  ];
  cargoConfigs = lib.filter exists [
    ".cargo/config"
    ".cargo/config.toml"
  ];
in
lib.optionals cfg.rust.enable (
  map (file: "${file}: kennel ignores this file") toolchains
  ++ map (file: "${file}: conflicts with kennel's .cargo/config.toml") cargoConfigs
)
