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

fn normalize_branch(git_ref: &str) -> &str {
    git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref)
}

async fn create_build_and_notify(
    config: &WebhookConfig,
    project_id: uuid::Uuid,
    project_name: &str,
    git_ref: &str,
    commit_sha: &str,
    branch: &str,
) -> Result<StatusCode> {
    let build = match config
        .store
        .builds()
        .create_build(
            project_id,
            project_name.to_string(),
            git_ref.to_string(),
            commit_sha.to_string(),
        )
        .await
    {
        Ok(b) => b,
        Err(e) => {
            if e.is_unique_violation() {
                info!("Build already exists for {project_name}/{git_ref}/{commit_sha}");
                return Ok(StatusCode::OK);
            }
            return Err(e.into());
        }
    };

    if let Err(e) = config
        .store
        .builds()
        .cancel_stale_builds(project_id, branch, build.id)
        .await
    {
        warn!("Failed to cancel stale builds: {e}");
    }

    info!(
        "Created build {} for {project_name}/{git_ref}/{commit_sha}",
        build.id
    );

    config.build_signal.notify_one();
    Ok(StatusCode::OK)
}

async fn mark_teardown_and_notify(
    config: &WebhookConfig,
    project_id: uuid::Uuid,
    branch: &str,
) -> Result<StatusCode> {
    config
        .store
        .deployments()
        .mark_for_teardown(project_id, branch)
        .await?;
    config.teardown_signal.notify_one();
    Ok(StatusCode::ACCEPTED)
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
                info!("Branch deleted: {project_name}/{branch}, marking deployments for teardown");
                return mark_teardown_and_notify(&config, project.id, branch).await;
            }

            create_build_and_notify(
                &config,
                project.id,
                &project.name,
                &git_ref,
                &commit_sha,
                branch,
            )
            .await
        }
        WebhookEvent::PullRequest {
            action,
            pr_number,
            commit_sha,
            ..
        } => match action.as_str() {
            "opened" | "synchronize" | "synchronized" | "reopened" => {
                let git_ref = format!("pr-{pr_number}");
                create_build_and_notify(
                    &config,
                    project.id,
                    &project.name,
                    &git_ref,
                    &commit_sha,
                    &git_ref,
                )
                .await
            }
            "closed" => {
                let branch = format!("pr-{pr_number}");
                info!("PR closed: {project_name}/PR#{pr_number}, marking deployments for teardown");
                mark_teardown_and_notify(&config, project.id, &branch).await
            }
            _ => {
                warn!("Ignoring PR action: {action}");
                Ok(StatusCode::ACCEPTED)
            }
        },
    }
}
