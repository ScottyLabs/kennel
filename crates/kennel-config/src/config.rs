use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct KennelConfig {
    #[serde(default)]
    pub services: HashMap<String, ServiceConfig>,

    #[serde(default)]
    pub static_sites: HashMap<String, StaticSiteConfig>,

    #[serde(default)]
    pub cachix: Option<CachixConfig>,
}

/// Kennel-specific metadata per service. Process configuration (exec, probes,
/// restart, ports, dependencies) comes from devenv's process definitions.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServiceConfig {
    #[serde(default)]
    pub custom_domain: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StaticSiteConfig {
    pub flake_output: Option<String>,

    #[serde(default)]
    pub spa: bool,

    #[serde(default)]
    pub custom_domain: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CachixConfig {
    pub cache_name: String,
}

pub async fn parse_kennel_toml(repo_path: &Path) -> std::io::Result<KennelConfig> {
    let config_path = repo_path.join("kennel.toml");

    if !config_path.exists() {
        return Ok(KennelConfig {
            services: HashMap::new(),
            static_sites: HashMap::new(),
            cachix: None,
        });
    }

    let content = tokio::fs::read_to_string(&config_path).await?;
    let config: KennelConfig = toml::from_str(&content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_config() {
        let toml_str = "";
        let config: KennelConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.services.len(), 0);
        assert_eq!(config.static_sites.len(), 0);
        assert!(config.cachix.is_none());
    }

    #[test]
    fn test_parse_service_config() {
        let toml_str = r#"
[services.api]
custom_domain = "api.myapp.com"
"#;
        let config: KennelConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.services.len(), 1);

        let api = config.services.get("api").unwrap();
        assert_eq!(api.custom_domain, Some("api.myapp.com".to_string()));
    }

    #[test]
    fn test_parse_static_site_config() {
        let toml_str = r#"
[static_sites.docs]
flake_output = "docs"
spa = false

[static_sites.web]
spa = true
"#;
        let config: KennelConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.static_sites.len(), 2);

        let docs = config.static_sites.get("docs").unwrap();
        assert_eq!(docs.flake_output, Some("docs".to_string()));
        assert!(!docs.spa);

        let web = config.static_sites.get("web").unwrap();
        assert!(web.spa);
    }

    #[test]
    fn test_parse_cachix_config() {
        let toml_str = r#"
[cachix]
cache_name = "my-cache"
"#;
        let config: KennelConfig = toml::from_str(toml_str).unwrap();
        assert!(config.cachix.is_some());

        let cachix = config.cachix.unwrap();
        assert_eq!(cachix.cache_name, "my-cache");
    }
}
