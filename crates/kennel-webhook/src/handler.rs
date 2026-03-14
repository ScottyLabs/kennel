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

/// Normalize a git ref to a short branch name by stripping the refs/heads/ prefix.
fn normalize_branch(git_ref: &str) -> &str {
    git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref)
}

pub async fn handle_webhook(
    State(config): State<Arc<WebhookConfig>>,
    Path(project_name): Path<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode> {
    info!("Received webhook for project: {}", project_name);

    let project = config
        .store
        .projects()
        .find_by_name(&project_name)
        .await?
        .ok_or_else(|| WebhookError::ProjectNotFound(project_name.clone()))?;

    let event_type = headers
        .get("X-Forgejo-Event")
        .or_else(|| headers.get("X-GitHub-Event"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    if let Err(e) = verify_signature(&headers, &body, &project.webhook_secret) {
        error!(
            "Signature verification failed for project '{}', IP: {}, event type: {}",
            project_name,
            addr.ip(),
            event_type
        );
        return Err(e);
    }

    let event = parse_webhook_event(&headers, &body)?;

    match event {
        WebhookEvent::Push {
            git_ref,
            commit_sha,
            deleted,
            ..
        } => {
            let branch = normalize_branch(&git_ref);

            if deleted {
                info!(
                    "Branch deleted: {}/{}, marking deployments for teardown",
                    project_name, branch
                );
                config
                    .store
                    .deployments()
                    .mark_for_teardown(&project.name, branch)
                    .await?;
                config.teardown_signal.notify_one();
                return Ok(StatusCode::ACCEPTED);
            }

            let build = match config
                .store
                .builds()
                .create_build(project.name.clone(), git_ref.clone(), commit_sha.clone())
                .await
            {
                Ok(b) => b,
                Err(e) => {
                    if e.is_unique_violation() {
                        info!(
                            "Build already exists for {}/{}/{}",
                            project_name, git_ref, commit_sha
                        );
                        return Ok(StatusCode::OK);
                    }
                    return Err(e.into());
                }
            };

            // Cancel stale builds for the same branch.
            if let Err(e) = config
                .store
                .builds()
                .cancel_stale_builds(&project.name, branch, build.id)
                .await
            {
                warn!("Failed to cancel stale builds: {e}");
            }

            info!(
                "Created build {} for {}/{}/{}",
                build.id, project_name, git_ref, commit_sha
            );

            config.build_signal.notify_one();
            Ok(StatusCode::OK)
        }
        WebhookEvent::PullRequest {
            action,
            pr_number,
            commit_sha,
            ..
        } => {
            match action.as_str() {
                "opened" | "synchronize" | "synchronized" | "reopened" => {
                    let git_ref = format!("pr-{}", pr_number);

                    let build = match config
                        .store
                        .builds()
                        .create_build(project.name.clone(), git_ref.clone(), commit_sha.clone())
                        .await
                    {
                        Ok(b) => b,
                        Err(e) => {
                            if e.is_unique_violation() {
                                info!(
                                    "Build already exists for {}/{}/{}",
                                    project_name, git_ref, commit_sha
                                );
                                return Ok(StatusCode::OK);
                            }
                            return Err(e.into());
                        }
                    };

                    // Cancel stale builds for this PR branch.
                    if let Err(e) = config
                        .store
                        .builds()
                        .cancel_stale_builds(&project.name, &git_ref, build.id)
                        .await
                    {
                        warn!("Failed to cancel stale builds: {e}");
                    }

                    info!(
                        "Created PR build {} for {}/PR#{}/{}",
                        build.id, project_name, pr_number, commit_sha
                    );

                    config.build_signal.notify_one();
                    Ok(StatusCode::OK)
                }
                "closed" => {
                    let branch = format!("pr-{}", pr_number);
                    info!(
                        "PR closed: {}/PR#{}, marking deployments for teardown",
                        project_name, pr_number
                    );
                    config
                        .store
                        .deployments()
                        .mark_for_teardown(&project.name, &branch)
                        .await?;
                    config.teardown_signal.notify_one();
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
