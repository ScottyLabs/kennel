use crate::{ResourceProvider, ResourceRequest};
use std::collections::HashMap;

pub struct ValkeyProvider {
    socket_path: String,
}

impl ValkeyProvider {
    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }
}

impl ResourceProvider for ValkeyProvider {
    fn name(&self) -> &str {
        "valkey"
    }

    async fn provision(
        &self,
        _request: &ResourceRequest,
    ) -> anyhow::Result<HashMap<String, String>> {
        // TODO: allocate DB number via HSET in DB 0, return VALKEY_URL
        let mut env = HashMap::new();
        env.insert(
            "VALKEY_URL".into(),
            format!("redis+unix://{}", self.socket_path),
        );
        Ok(env)
    }

    async fn teardown(&self, _request: &ResourceRequest) -> anyhow::Result<()> {
        // TODO: FLUSHDB, HDEL allocation
        Ok(())
    }

    async fn reconcile(&self, _active: &[ResourceRequest]) -> anyhow::Result<()> {
        Ok(())
    }
}
