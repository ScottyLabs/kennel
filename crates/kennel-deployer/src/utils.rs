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

pub fn sanitize_username(project: &str, branch: &str, service: &str) -> String {
    format!(
        "kennel-{}-{}-{}",
        sanitize_identifier(project),
        sanitize_identifier(branch),
        sanitize_identifier(service)
    )
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
