pub fn sanitize_identifier(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

pub fn process_name(project: &str, branch_slug: &str, service: &str) -> String {
    format!(
        "kennel-{}-{}-{}",
        sanitize_identifier(project),
        sanitize_identifier(branch_slug),
        sanitize_identifier(service)
    )
}

pub fn sanitize_username(project: &str, branch: &str, service: &str) -> String {
    process_name(project, branch, service)
}

pub async fn create_custom_domain_dns(
    dns_manager: &kennel_dns::DnsManager,
    store: &kennel_store::Store,
    deployment_id: uuid::Uuid,
    custom_domain: &str,
) {
    tracing::info!("Creating DNS records for custom domain: {custom_domain}");
    match dns_manager
        .create_record_for_deployment(deployment_id, custom_domain)
        .await
    {
        Ok(_) => {
            tracing::info!("DNS records created for {custom_domain}");
            if let Err(e) = store
                .deployments()
                .update_dns_status(
                    deployment_id,
                    entity::sea_orm_active_enums::DnsStatus::Active,
                )
                .await
            {
                tracing::warn!("Failed to update dns_status: {e}");
            }
        }
        Err(e) => tracing::warn!("Failed to create DNS records for {custom_domain}: {e}"),
    }
}

pub fn generate_deployment_domain(
    service_name: &str,
    branch: &str,
    project_name: &str,
    base_domain: &str,
) -> String {
    format!(
        "{}-{}.{}.{}",
        sanitize_identifier(service_name),
        sanitize_identifier(branch),
        project_name,
        base_domain
    )
}
