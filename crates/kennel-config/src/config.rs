use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Deployment configuration produced by the kennel devenv module.
/// Deserialized from the JSON output of evaluating
/// `.#devenv.shells.default.config.kennel.config`.
#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct KennelConfig {
    #[serde(default)]
    pub services: HashMap<String, ServiceConfig>,

    #[serde(default)]
    pub static_sites: HashMap<String, StaticSiteConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServiceConfig {
    #[serde(default)]
    pub custom_domain: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StaticSiteConfig {
    #[serde(default)]
    pub spa: bool,

    #[serde(default)]
    pub custom_domain: Option<String>,

    #[serde(default)]
    pub package_attr: Option<String>,
}
