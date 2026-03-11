use std::collections::HashMap;
use std::path::Path;

use secrecy::ExposeSecret;
use secretspec::Secrets;

/// Resolve secrets from a project's secretspec.toml using the configured
/// Vault/OpenBao provider. Returns a map of secret names to values for
/// injection into the process environment.
///
/// If no secretspec.toml exists in the repository, returns an empty map.
/// The profile maps to the deployment environment (prod, staging, dev, preview).
pub fn resolve_secrets(
    repo_path: &Path,
    environment: &str,
    vault_endpoint: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let secretspec_path = repo_path.join("secretspec.toml");
    if !secretspec_path.exists() {
        tracing::debug!(
            path = %secretspec_path.display(),
            "no secretspec.toml found, skipping secret resolution"
        );
        return Ok(HashMap::new());
    }

    tracing::info!(
        path = %secretspec_path.display(),
        environment,
        "resolving secrets from secretspec.toml"
    );

    let mut spec = Secrets::load_from(&secretspec_path)?;
    spec.set_provider(vault_endpoint);
    spec.set_profile(environment);

    let validated = spec.ensure_secrets(None, None, false)?;

    let secrets: HashMap<String, String> = validated
        .resolved
        .secrets
        .into_iter()
        .map(|(k, v)| (k, v.expose_secret().to_string()))
        .collect();

    tracing::info!(
        count = secrets.len(),
        environment,
        "resolved {} secrets",
        secrets.len()
    );

    Ok(secrets)
}
