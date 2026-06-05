pub mod garage;
pub mod postgres;
pub mod valkey;

use kennel_config::Environment;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ResourceRequest {
    pub project_name: String,
    pub service_name: String,
    pub branch_slug: String,
    pub environment: Environment,
}

// Dispatched concretely through the `Provider` enum, never behind `dyn` or a
// spawning generic, so the futures' un-nameable `Send` bound is never required.
#[allow(async_fn_in_trait)]
pub trait ResourceProvider {
    fn name(&self) -> &str;
    async fn provision(&self, request: &ResourceRequest)
    -> anyhow::Result<HashMap<String, String>>;
    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()>;
    async fn reconcile(&self, active: &[ResourceRequest]) -> anyhow::Result<()>;
}

pub enum Provider {
    Postgres(postgres::PostgresProvider),
    Valkey(valkey::ValkeyProvider),
    Garage(garage::GarageProvider),
}

impl ResourceProvider for Provider {
    fn name(&self) -> &str {
        match self {
            Self::Postgres(p) => p.name(),
            Self::Valkey(v) => v.name(),
            Self::Garage(g) => g.name(),
        }
    }

    async fn provision(
        &self,
        request: &ResourceRequest,
    ) -> anyhow::Result<HashMap<String, String>> {
        match self {
            Self::Postgres(p) => p.provision(request).await,
            Self::Valkey(v) => v.provision(request).await,
            Self::Garage(g) => g.provision(request).await,
        }
    }

    async fn teardown(&self, request: &ResourceRequest) -> anyhow::Result<()> {
        match self {
            Self::Postgres(p) => p.teardown(request).await,
            Self::Valkey(v) => v.teardown(request).await,
            Self::Garage(g) => g.teardown(request).await,
        }
    }

    async fn reconcile(&self, active: &[ResourceRequest]) -> anyhow::Result<()> {
        match self {
            Self::Postgres(p) => p.reconcile(active).await,
            Self::Valkey(v) => v.reconcile(active).await,
            Self::Garage(g) => g.reconcile(active).await,
        }
    }
}
