{ pkgs, lib, config, ... }:

let
  cfg = config.scottylabs.secrets;

  # secretspec is not yet in nixpkgs at 0.11.0 (unstable ships 0.10.1), so pin
  # the upstream cargo-dist prebuilt binary
  secretspec =
    let
      version = "0.11.0";
      baseUrl = "https://github.com/cachix/secretspec/releases/download/v${version}";
      # target, SRI hash of secretspec-<target>.tar.xz from the release digests
      sources = {
        "aarch64-darwin" = { target = "aarch64-apple-darwin"; hash = "sha256-MmCaXr7wo94CISg5SCOFDZ3G0p1G4o3bjiQe+qOk+1M="; };
        "x86_64-darwin" = { target = "x86_64-apple-darwin"; hash = "sha256-LDKucmPitNWgt2vtM8y4LYeCkj7eP83tyUaHr+wMTe4="; };
        "aarch64-linux" = { target = "aarch64-unknown-linux-gnu"; hash = "sha256-GfJd95uBVIwKnn6b3y2yPmKmmrV2WTbLAjY+itB8zuE="; };
        "x86_64-linux" = { target = "x86_64-unknown-linux-gnu"; hash = "sha256-ppcepI0DyiDa8lVds7tCFr4VtaM9zj9G8LTzUc1jePM="; };
      };
      inherit (pkgs.stdenv.hostPlatform) system;
      source = sources.${system} or (throw "scottylabs.secrets: secretspec ${version} has no prebuilt binary for ${system}");
    in
    pkgs.stdenvNoCC.mkDerivation {
      pname = "secretspec";
      inherit version;

      src = pkgs.fetchurl {
        url = "${baseUrl}/secretspec-${source.target}.tar.xz";
        inherit (source) hash;
      };

      sourceRoot = "secretspec-${source.target}";

      nativeBuildInputs = lib.optionals pkgs.stdenv.isLinux [ pkgs.autoPatchelfHook ];
      buildInputs = lib.optionals pkgs.stdenv.isLinux [ pkgs.stdenv.cc.cc.lib pkgs.dbus ];

      installPhase = ''
        runHook preInstall
        install -Dm755 secretspec -t $out/bin
        runHook postInstall
      '';

      meta = {
        description = "Declarative secrets, every environment, any provider.";
        homepage = "https://secretspec.dev";
        license = lib.licenses.asl20;
        mainProgram = "secretspec";
        platforms = builtins.attrNames sources;
      };
    };
in
{
  options.scottylabs.secrets = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install the openbao and secretspec CLIs and set BAO_ADDR for OpenBao";
    };
  };

  config = lib.mkIf (config.scottylabs.enable && cfg.enable) {
    packages = [ pkgs.openbao secretspec ];

    # devenv resolves secretspec into config.secretspec.secrets, put them into the shell
    # mkDefault lets explicit env vars (e.g. DATABASE_URL) win on name collisions
    env = lib.mkMerge [
      { BAO_ADDR = "https://secrets2.scottylabs.org"; }
      (lib.mapAttrs (_: lib.mkDefault) config.secretspec.secrets)
    ];
  };
}
