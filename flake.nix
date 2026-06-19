{
  description = "ScottyLabs shared devenv configuration";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    ricochet = {
      url = "git+https://codeberg.org/anish/ricochet";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, ricochet, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devenvModules.default = { pkgs, ... }: {
        imports = [ ./modules ];
        _module.args.ricochet = ricochet.packages.${pkgs.system}.ricochet;
      };

      # Authenticate to OpenBao before any project shell can build
      apps = forAllSystems (pkgs: {
        login = {
          type = "app";
          program = "${pkgs.writeShellScript "bao-login" ''
            export BAO_ADDR=https://secrets2.scottylabs.org
            exec ${pkgs.openbao}/bin/bao login -method=oidc
          ''}";
        };
      });
    };
}
