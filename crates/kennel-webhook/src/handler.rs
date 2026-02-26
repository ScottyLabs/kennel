use crate::WebhookConfig;
use crate::error::{Result, WebhookError};
use crate::events::WebhookEvent;
use crate::parse::parse_webhook_event;
use crate::verify::verify_signature;
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{error, info, warn};

pub async fn handle_webhook(
    State(config): State<Arc<WebhookConfig>>,
    Path(project_name): Path<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode> {
    info!("Received webhook for project: {}", project_name);

    // Look up project
    let project = config
        .store
        .projects()
        .find_by_name(&project_name)
        .await?
        .ok_or_else(|| WebhookError::ProjectNotFound(project_name.clone()))?;

    // Determine event type from headers for logging
    let event_type = headers
        .get("X-Forgejo-Event")
        .or_else(|| headers.get("X-GitHub-Event"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    // Verify signature
    if let Err(e) = verify_signature(&headers, &body, &project.webhook_secret) {
        error!(
            "Signature verification failed for project '{}', IP: {}, event type: {}",
            project_name,
            addr.ip(),
            event_type
        );
        return Err(e);
    }

    // Parse event
    let event = parse_webhook_event(&headers, &body)?;

    match event {
        WebhookEvent::Push {
            git_ref,
            commit_sha,
            author,
            deleted,
        } => {
            if deleted {
                info!(
                    "Branch deleted: {}/{}, marking deployments for teardown",
                    project_name, git_ref
                );
                config
                    .store
                    .deployments()
                    .mark_for_teardown(&project.name, &git_ref)
                    .await?;
                return Ok(StatusCode::ACCEPTED);
            }

            // Check for duplicate build
            if config
                .store
                .builds()
                .exists(&project.name, &git_ref, &commit_sha)
                .await?
            {
                info!(
                    "Build already exists for {}/{}/{}",
                    project_name, git_ref, commit_sha
                );
                return Ok(StatusCode::OK);
            }

            // Create build record
            let build = config
                .store
                .builds()
                .create_build(
                    project.name.clone(),
                    git_ref.clone(),
                    commit_sha.clone(),
                    author,
                )
                .await?;

            info!(
                "Created build {} for {}/{}/{}",
                build.id, project_name, git_ref, commit_sha
            );

            // Send to builder
            config
                .build_tx
                .send(build.id as i64)
                .await
                .map_err(|_| WebhookError::BuilderUnavailable)?;

            Ok(StatusCode::OK)
        }
        WebhookEvent::PullRequest {
            action,
            pr_number,
            commit_sha,
            author,
        } => {
            match action.as_str() {
                "opened" | "synchronize" | "synchronized" | "reopened" => {
                    let git_ref = format!("pr-{}", pr_number);

                    // Check for duplicate build
                    if config
                        .store
                        .builds()
                        .exists(&project.name, &git_ref, &commit_sha)
                        .await?
                    {
                        info!(
                            "Build already exists for {}/{}/{}",
                            project_name, git_ref, commit_sha
                        );
                        return Ok(StatusCode::OK);
                    }

                    // Create build record
                    let build = config
                        .store
                        .builds()
                        .create_build(
                            project.name.clone(),
                            git_ref.clone(),
                            commit_sha.clone(),
                            author,
                        )
                        .await?;

                    info!(
                        "Created PR build {} for {}/PR#{}/{}",
                        build.id, project_name, pr_number, commit_sha
                    );

                    // Send to builder
                    config
                        .build_tx
                        .send(build.id as i64)
                        .await
                        .map_err(|_| WebhookError::BuilderUnavailable)?;

                    Ok(StatusCode::OK)
                }
                "closed" => {
                    let git_ref = format!("pr-{}", pr_number);
                    info!(
                        "PR closed: {}/PR#{}, marking deployments for teardown",
                        project_name, pr_number
                    );
                    config
                        .store
                        .deployments()
                        .mark_for_teardown(&project.name, &git_ref)
                        .await?;
                    Ok(StatusCode::ACCEPTED)
                }
                _ => {
                    warn!("Ignoring PR action: {}", action);
                    Ok(StatusCode::ACCEPTED)
                }
            }
        }
    }
}
