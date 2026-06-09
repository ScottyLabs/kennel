{ pkgs, config, inputs, ... }:

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
    kennel.sites.docs = {
      customDomain = "docs.kennel.scottylabs.org";
    };
  };

  packages = with pkgs; [
    sea-orm-cli
    just
  ];
}
