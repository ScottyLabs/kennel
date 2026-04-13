use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::path::Path;

/// Resolve secrets from secretspec.toml via OpenBao for the given environment.
/// Returns environment variables to inject into the process.
pub fn resolve(
    repo_path: &Path,
    environment: &str,
    vault_endpoint: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let secretspec_path = repo_path.join("secretspec.toml");
    if !secretspec_path.exists() {
        return Ok(HashMap::new());
    }

    let mut spec = secretspec::Secrets::load_from(&secretspec_path)?;
    spec.set_provider(vault_endpoint);
    spec.set_profile(environment);

    let validated = spec.ensure_secrets(None, None, false)?;

    Ok(validated
        .resolved
        .secrets
        .iter()
        .map(|(k, v)| (k.clone(), v.expose_secret().to_string()))
        .collect())
}
