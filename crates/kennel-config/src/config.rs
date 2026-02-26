use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct KennelConfig {
    #[serde(default)]
    pub services: HashMap<String, ServiceConfig>,

    #[serde(default)]
    pub static_sites: HashMap<String, StaticSiteConfig>,

    #[serde(default)]
    pub cachix: Option<CachixConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceConfig {
    pub flake_output: Option<String>,

    #[serde(default = "default_health_check_path")]
    pub health_check_path: String,

    #[serde(default = "default_health_check_timeout")]
    pub health_check_timeout_secs: u64,

    #[serde(default)]
    pub preview_database: bool,

    #[serde(default)]
    pub spa: bool,

    #[serde(default)]
    pub env: HashMap<String, String>,

    #[serde(default)]
    pub secrets: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StaticSiteConfig {
    pub flake_output: Option<String>,

    #[serde(default)]
    pub spa: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CachixConfig {
    pub cache_name: String,
    pub auth_token: Option<String>,
}

fn default_health_check_path() -> String {
    "/health".to_string()
}

fn default_health_check_timeout() -> u64 {
    30
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
flake_output = "api"
health_check_path = "/health"
health_check_timeout_secs = 60
secrets = ["DATABASE_PASSWORD", "JWT_SECRET"]

[services.api.env]
PORT = "8080"
DATABASE_URL = "postgres://localhost/db"
"#;
        let config: KennelConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.services.len(), 1);

        let api = config.services.get("api").unwrap();
        assert_eq!(api.flake_output, Some("api".to_string()));
        assert_eq!(api.health_check_path, "/health");
        assert_eq!(api.health_check_timeout_secs, 60);
        assert_eq!(api.env.get("PORT"), Some(&"8080".to_string()));
        assert_eq!(api.secrets.len(), 2);
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
auth_token = "secret-token"
"#;
        let config: KennelConfig = toml::from_str(toml_str).unwrap();
        assert!(config.cachix.is_some());

        let cachix = config.cachix.unwrap();
        assert_eq!(cachix.cache_name, "my-cache");
        assert_eq!(cachix.auth_token, Some("secret-token".to_string()));
    }
}
