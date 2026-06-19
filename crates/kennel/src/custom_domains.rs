use crate::AppState;
use anyhow::Context as _;
use std::os::unix::fs::PermissionsExt as _;

/// Writes the custom domains kennel serves to the `CUSTOM_DOMAINS_FILE` path, one per line.
pub async fn publish(state: &AppState) -> anyhow::Result<()> {
    let Some(path) = state.config.custom_domains_file.as_deref() else {
        return Ok(());
    };

    let deployments = state.store.deployments().list_all().await?;
    let mut hosts: Vec<String> = deployments
        .into_iter()
        .filter_map(|d| d.custom_domain)
        .collect();
    hosts.sort_unstable();
    hosts.dedup();

    let mut body = hosts.join("\n");
    body.push('\n');

    // write then rename so a reader never sees a partially written file
    let tmp = format!("{path}.tmp");
    tokio::fs::write(&tmp, &body)
        .await
        .with_context(|| format!("writing {tmp}"))?;
    tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644))
        .await
        .with_context(|| format!("chmod {tmp}"))?;
    tokio::fs::rename(&tmp, path)
        .await
        .with_context(|| format!("renaming {tmp} -> {path}"))?;

    Ok(())
}
