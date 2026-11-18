kennelInputs: [
  (import ./core.nix kennelInputs)
  (import ./shell.nix kennelInputs)
  ./hooks.nix
  ./processes.nix
  ./kennel.nix
  ./checks.nix
  ./secrets.nix
  ./ci.nix
  ./security.nix
  (import ./rust kennelInputs)
  (import ./deno kennelInputs)
  (import ./python.nix kennelInputs)
  ./haskell.nix
  ./postgres.nix
  ./sqlite.nix
  ./valkey.nix
  (import ./garage.nix kennelInputs)
  (import ./ricochet.nix kennelInputs)
]
