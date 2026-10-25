use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::path::Path;

/// Resolves the secretspec.toml in `dir` from OpenBao once its project matches `namespace`
pub fn resolve(
    dir: &Path,
    environment: &str,
    vault_endpoint: &str,
    namespace: Option<&str>,
) -> anyhow::Result<HashMap<String, String>> {
    resolve_scope(dir, environment, vault_endpoint, namespace, None)
}

/// Resolves the `[scopes.public]` values a site is served, empty when the scope is absent
pub fn resolve_public(
    dir: &Path,
    environment: &str,
    vault_endpoint: &str,
    namespace: Option<&str>,
) -> anyhow::Result<HashMap<String, String>> {
    resolve_scope(dir, environment, vault_endpoint, namespace, Some("public"))
}

fn resolve_scope(
    dir: &Path,
    environment: &str,
    vault_endpoint: &str,
    namespace: Option<&str>,
    scope: Option<&str>,
) -> anyhow::Result<HashMap<String, String>> {
    let secretspec_path = dir.join("secretspec.toml");
    if !secretspec_path.exists() {
        return Ok(HashMap::new());
    }

    let config: secretspec::Config = std::fs::read_to_string(&secretspec_path)?.parse()?;
    check_namespace(&config.project.name, namespace)?;
    if let Some(scope) = scope
        && !config
            .scopes
            .as_ref()
            .is_some_and(|s| s.contains_key(scope))
    {
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

fn check_namespace(project_name: &str, namespace: Option<&str>) -> anyhow::Result<()> {
    if let Some(namespace) = namespace
        && project_name != namespace
    {
        anyhow::bail!(
            "secretspec.toml project name \"{project_name}\" does not match this repo's governance namespace \"{namespace}\""
        );
    }

    Ok(())
}
