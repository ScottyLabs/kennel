{ pkgs, config, inputs, ... }:

let
  cargoNix = pkgs.callPackage ./Cargo.nix { };
  kennel = cargoNix.workspaceMembers.kennel.build;
in
{
  imports = [ inputs.scottylabs.devenvModules.default ];

  scottylabs = {
    enable = true;
    project.name = "kennel";
    rust = {
      enable = true;
      cranelift.excludePackages = [ "aws-lc-sys" "aws-lc-rs" "rustls" "linkme" ];
    };
    sqlite.enable = true;
    secrets.enable = true;
  };

  packages = [
    kennel
  ] ++ (with pkgs; [
    sea-orm-cli
    just
  ]);

  outputs = { inherit kennel; };
}
