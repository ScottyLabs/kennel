use crate::error::Result;
use entity::{deployments, services};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub enum RouteTarget {
    Service { port: u16 },
    StaticSite { path: PathBuf, spa: bool },
}

#[derive(Debug, Clone)]
pub struct Route {
    pub target: RouteTarget,
    pub deployment_id: i32,
}

pub struct RoutingTable {
    routes: Arc<RwLock<HashMap<String, Route>>>,
}

impl RoutingTable {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get(&self, domain: &str) -> Option<Route> {
        let routes = self.routes.read().await;
        routes.get(domain).cloned()
    }

    pub async fn insert(&self, domain: String, route: Route) {
        let mut routes = self.routes.write().await;
        routes.insert(domain, route);
    }

    pub async fn remove(&self, domain: &str) -> Option<Route> {
        let mut routes = self.routes.write().await;
        routes.remove(domain)
    }

    pub async fn len(&self) -> usize {
        let routes = self.routes.read().await;
        routes.len()
    }

    pub async fn is_empty(&self) -> bool {
        let routes = self.routes.read().await;
        routes.is_empty()
    }

    pub async fn load_from_deployments_with_services(
        &self,
        deployments_with_services: Vec<(deployments::Model, Option<services::Model>)>,
    ) -> Result<()> {
        let mut routes = self.routes.write().await;
        routes.clear();

        for (deployment, service) in deployments_with_services {
            let service = service.ok_or_else(|| {
                crate::RouterError::Other(anyhow::anyhow!(
                    "Service not found for deployment {}",
                    deployment.id
                ))
            })?;

            let target = if let Some(port) = deployment.port {
                RouteTarget::Service { port: port as u16 }
            } else {
                let path = deployment
                    .store_path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| crate::RouterError::Other(anyhow::anyhow!("No store path")))?;

                RouteTarget::StaticSite {
                    path,
                    spa: service.spa,
                }
            };

            let route = Route {
                target,
                deployment_id: deployment.id,
            };

            // Insert auto-generated domain
            routes.insert(deployment.domain.clone(), route.clone());

            // Also insert custom domain if configured
            if let Some(custom_domain) = service.custom_domain {
                routes.insert(custom_domain, route);
            }
        }

        Ok(())
    }
}

impl Default for RoutingTable {
    fn default() -> Self {
        Self::new()
    }
}
