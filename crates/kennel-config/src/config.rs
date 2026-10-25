use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Deployment manifest a project's flake builds, read from `kennel.json` in
/// the artifact kennel receives from nixbot.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct KennelConfig {
    #[serde(default)]
    pub services: HashMap<String, ServiceConfig>,

    #[serde(default)]
    pub static_sites: HashMap<String, StaticSiteConfig>,

    pub preview_deployments: bool,

    #[serde(default)]
    pub version: u32,

    #[serde(default)]
    pub resources: Vec<String>,

    /// Extensions to create in the project's database
    #[serde(default)]
    pub postgres_extensions: Vec<String>,
}

impl KennelConfig {
    /// Whether this config matches the schema version kennel understands
    pub fn is_compatible(&self) -> bool {
        self.version == crate::constants::KENNEL_CONFIG_SCHEMA_VERSION
    }

    /// Whether the project declared the named resource provider
    pub fn provides(&self, resource: &str) -> bool {
        self.resources.iter().any(|r| r == resource)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServiceConfig {
    #[serde(default)]
    pub custom_domain: Option<String>,

    #[serde(default)]
    pub store_path: Option<String>,

    /// Program the unit runs, `meta.mainProgram` of the service's package
    #[serde(default)]
    pub exec: Option<String>,

    /// Non-secret variables the unit runs with, below resources and secrets in precedence
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StaticSiteConfig {
    #[serde(default)]
    pub spa: bool,

    #[serde(default)]
    pub custom_domain: Option<String>,

    #[serde(default)]
    pub store_path: Option<String>,
}
