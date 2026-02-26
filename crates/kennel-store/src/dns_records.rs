use crate::Result;
use entity::dns_records;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};

pub struct Repository<'a> {
    pub db: &'a DatabaseConnection,
}

impl<'a> Repository<'a> {
    pub async fn create(
        &self,
        domain: &str,
        deployment_id: impl Into<Option<i32>>,
        provider_record_id: &str,
        record_type: &str,
        ip_address: &str,
    ) -> Result<dns_records::Model> {
        let model = dns_records::ActiveModel {
            domain: ActiveValue::Set(domain.to_string()),
            deployment_id: ActiveValue::Set(deployment_id.into()),
            provider_record_id: ActiveValue::Set(provider_record_id.to_string()),
            record_type: ActiveValue::Set(record_type.to_string()),
            ip_address: ActiveValue::Set(ip_address.to_string()),
            ..Default::default()
        };

        let record = model.insert(self.db).await?;
        Ok(record)
    }

    pub async fn find_by_deployment(&self, deployment_id: i32) -> Result<Vec<dns_records::Model>> {
        let records = dns_records::Entity::find()
            .filter(dns_records::Column::DeploymentId.eq(deployment_id))
            .all(self.db)
            .await?;

        Ok(records)
    }

    pub async fn find_by_domain(&self, domain: &str) -> Result<Vec<dns_records::Model>> {
        let records = dns_records::Entity::find()
            .filter(dns_records::Column::Domain.eq(domain))
            .all(self.db)
            .await?;

        Ok(records)
    }

    pub async fn find_all(&self) -> Result<Vec<dns_records::Model>> {
        let records = dns_records::Entity::find().all(self.db).await?;
        Ok(records)
    }

    pub async fn delete(&self, id: i32) -> Result<()> {
        dns_records::Entity::delete_by_id(id).exec(self.db).await?;
        Ok(())
    }
}
