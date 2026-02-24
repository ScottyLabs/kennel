use ::entity::{port_allocations, prelude::*};
use sea_orm::*;
use std::collections::HashSet;

use crate::{Result, StoreError};

const PORT_MIN: i32 = 18000;
const PORT_MAX: i32 = 19999;

pub struct PortAllocationRepository<'a> {
    db: &'a DatabaseConnection,
}

impl<'a> PortAllocationRepository<'a> {
    pub fn new(db: &'a DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_port(&self, port: i32) -> Result<Option<port_allocations::Model>> {
        Ok(PortAllocations::find_by_id(port).one(self.db).await?)
    }

    pub async fn find_by_deployment(
        &self,
        deployment_id: i32,
    ) -> Result<Option<port_allocations::Model>> {
        Ok(PortAllocations::find()
            .filter(port_allocations::Column::DeploymentId.eq(deployment_id))
            .one(self.db)
            .await?)
    }

    pub async fn list_allocated(&self) -> Result<Vec<port_allocations::Model>> {
        Ok(PortAllocations::find().all(self.db).await?)
    }

    pub async fn allocate_port(
        &self,
        deployment_id: i32,
        project_name: &str,
        service_name: &str,
        branch: &str,
    ) -> Result<i32> {
        let allocated_ports: Vec<i32> = PortAllocations::find()
            .select_only()
            .column(port_allocations::Column::Port)
            .into_tuple()
            .all(self.db)
            .await?;

        let allocated_set: HashSet<i32> = allocated_ports.into_iter().collect();

        let port = (PORT_MIN..=PORT_MAX)
            .find(|p| !allocated_set.contains(p))
            .ok_or(StoreError::PortPoolExhausted)?;

        port_allocations::ActiveModel {
            port: Set(port),
            deployment_id: Set(Some(deployment_id)),
            project_name: Set(Some(project_name.to_string())),
            service_name: Set(Some(service_name.to_string())),
            branch: Set(Some(branch.to_string())),
            ..Default::default()
        }
        .insert(self.db)
        .await?;

        Ok(port)
    }

    pub async fn release_port(&self, port: i32) -> Result<()> {
        PortAllocations::delete_by_id(port).exec(self.db).await?;
        Ok(())
    }

    pub async fn is_port_available(&self, port: i32) -> Result<bool> {
        if !(PORT_MIN..=PORT_MAX).contains(&port) {
            return Ok(false);
        }

        Ok(self.find_by_port(port).await?.is_none())
    }

    pub fn is_port_in_range(port: i32) -> bool {
        (PORT_MIN..=PORT_MAX).contains(&port)
    }
}
