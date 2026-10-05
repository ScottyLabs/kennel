use kennel_config::constants::BUILD_SCOPE;
use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::path::Path;

/// Resolve secrets from secretspec.toml via OpenBao for the given environment,
/// limited to `scope` if given. Returns environment variables to inject into the process.
pub fn resolve(
    repo_path: &Path,
    environment: &str,
    vault_endpoint: &str,
    scope: Option<&str>,
) -> anyhow::Result<HashMap<String, String>> {
    let secretspec_path = repo_path.join("secretspec.toml");
    if !secretspec_path.exists() {
        return Ok(HashMap::new());
    }

    let mut spec = secretspec::Secrets::load_from(&secretspec_path)?;
    spec.set_provider(vault_endpoint);
    spec.set_profile(environment);
    if let Some(scope) = scope {
        spec.set_scope(scope);
    }

    let validated = spec.ensure_secrets(None, None, false)?;

    Ok(validated
        .resolved
        .secrets
        .iter()
        .map(|(k, v)| (k.clone(), v.expose_secret().to_string()))
        .collect())
}

/// Resolve the build scope of the secretspec.toml in `spec_dir`
pub fn resolve_build_env(
    spec_dir: &Path,
    environment: &str,
    vault_endpoint: &str,
) -> anyhow::Result<Option<HashMap<String, String>>> {
    let contents = std::fs::read_to_string(spec_dir.join("secretspec.toml"))?;
    let config: secretspec::Config = contents.parse()?;
    if !config
        .scopes
        .is_some_and(|scopes| scopes.contains_key(BUILD_SCOPE))
    {
        return Ok(None);
    }

    resolve(spec_dir, environment, vault_endpoint, Some(BUILD_SCOPE)).map(Some)
}
