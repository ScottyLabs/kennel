use std::collections::HashMap;
use std::path::PathBuf;

use tokio::sync::RwLock;

use crate::error::Result;
use entity::{deployments, services};

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
    routes: RwLock<HashMap<String, Route>>,
}

impl RoutingTable {
    pub fn new() -> Self {
        Self {
            routes: RwLock::new(HashMap::new()),
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

    /// Load static site routes from deployment records. Service routes are
    /// populated dynamically via supervisor events (since the port is only
    /// known at runtime).
    pub async fn load_static_sites_from_deployments(
        &self,
        deployments_with_services: Vec<(deployments::Model, Option<services::Model>)>,
    ) -> Result<()> {
        let mut routes = self.routes.write().await;

        for (deployment, service) in deployments_with_services {
            let service = match service {
                Some(s) => s,
                None => continue,
            };

            // Only load static sites from DB. Service routes depend on
            // supervisor-reported ports and are added via events.
            if service.r#type != entity::sea_orm_active_enums::ServiceType::Static {
                continue;
            }

            let path = match deployment.store_path.as_ref() {
                Some(p) => PathBuf::from(p),
                None => continue,
            };

            let route = Route {
                target: RouteTarget::StaticSite {
                    path,
                    spa: service.spa,
                },
                deployment_id: deployment.id,
            };

            routes.insert(deployment.domain.clone(), route.clone());

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
