{
  inputs,
  flakeModule,
  system,
}:

(inputs.flake-parts.lib.mkFlake
  {
    inputs = inputs // {
      self.outPath = ./.;
    };
  }
  {
    imports = [ flakeModule ];
    systems = [ system ];
    perSystem =
      { lib, ... }:
      {
        scottylabs = {
          project.name = "fixture";
          rust.enable = true;
          # svelte would need a SvelteKit app here
          deno = {
            enable = true;
            react.enable = true;
          };
          python.enable = true;
          postgres.enable = true;
          sqlite.enable = true;
          valkey.enable = true;
          garage.enable = true;
          ricochet = {
            enable = true;
            appUrl = "http://localhost:3000";
          };
        };

        # the fixture has no flake.nix
        treefmt.projectRootFile = lib.mkForce "default.nix";
      };
  }
).checks.${system}
