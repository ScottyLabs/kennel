{
  description = "ScottyLabs shared devenv configuration";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { nixpkgs }:
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
      devenvModules.default = import ./modules;

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
