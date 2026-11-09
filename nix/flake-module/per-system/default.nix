kennelInputs: [
  (import ./core.nix kennelInputs)
  (import ./shell.nix kennelInputs)
  ./hooks.nix
  (import ./processes.nix kennelInputs)
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
  (import ./garage.nix kennelInputs)
  (import ./ricochet.nix kennelInputs)
]
