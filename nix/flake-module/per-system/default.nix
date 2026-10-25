kennelInputs: [
  (import ./core.nix kennelInputs)
  ./shell.nix
  ./hooks.nix
  ./processes.nix
  ./kennel.nix
  ./checks.nix
  ./secrets.nix
  ./ci.nix
  ./security.nix
  (import ./rust.nix kennelInputs)
  ./deno.nix
  (import ./python.nix kennelInputs)
  ./haskell.nix
  ./postgres.nix
  ./sqlite.nix
  ./valkey.nix
  ./garage.nix
  (import ./ricochet.nix kennelInputs)
]
