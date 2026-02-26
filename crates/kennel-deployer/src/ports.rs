use crate::error::Result;
use kennel_config::constants;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct PortAllocator {
    allocated: Arc<Mutex<HashSet<u16>>>,
}

impl Default for PortAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl PortAllocator {
    pub fn new() -> Self {
        Self {
            allocated: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub async fn allocate(&self) -> Result<u16> {
        let mut allocated = self.allocated.lock().await;

        for port in constants::PORT_RANGE_START..=constants::PORT_RANGE_END {
            if !allocated.contains(&port) && is_port_available(port).await {
                allocated.insert(port);
                return Ok(port);
            }
        }

        Err(crate::DeployerError::PortAllocation(
            "No available ports in range".to_string(),
        ))
    }

    pub async fn release(&self, port: u16) {
        let mut allocated = self.allocated.lock().await;
        allocated.remove(&port);
    }
}

async fn is_port_available(port: u16) -> bool {
    tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_port_allocation() {
        let allocator = PortAllocator::new();

        let port1 = allocator.allocate().await.unwrap();
        let port2 = allocator.allocate().await.unwrap();

        assert_ne!(port1, port2);
        assert!(port1 >= constants::PORT_RANGE_START && port1 <= constants::PORT_RANGE_END);
        assert!(port2 >= constants::PORT_RANGE_START && port2 <= constants::PORT_RANGE_END);
    }
}
