{ pkgs, inputs, ... }:

{
  imports = [ inputs.scottylabs.devenvModules.default ];

  scottylabs = {
    enable = true;
    project.name = "kennel";
    rust = {
      enable = true;
      cranelift.excludePackages = [
        "aws-lc-sys"
        "aws-lc-rs"
        "rustls"
        "linkme"
      ];
    };
    sqlite.enable = true;
    kennel.sites.docs = {
      customDomain = "docs.kennel.scottylabs.org";
    };
  };

  packages = with pkgs; [
    sea-orm-cli
  ];

  scripts = {
    server.exec = "cargo run -p kennel";
    migration.exec = ''sea-orm-cli migrate generate "$1" -d crates/migration'';
    migrate.exec = ''DATABASE_URL="sqlite://.devenv/state/kennel.db?mode=rwc" sea-orm-cli migrate up -d crates/migration'';
    generate-entities.exec = ''DATABASE_URL="sqlite://.devenv/state/kennel.db" sea-orm-cli generate entity -o crates/entity/src --with-serde both --lib'';
    docs.exec = "cd sites/docs && mdbook serve";
    clean.exec = "rm -rf .devenv/state";
  };
}
