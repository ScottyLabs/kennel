use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use hmac::{Hmac, Mac, digest::KeyInit};
use sea_orm::ActiveValue::Set;
use sha2::Sha256;
use std::sync::Arc;

use crate::AppState;
use crate::caddy::CaddyClient;
use crate::systemd::SystemdClient;
use crate::teardown::teardown_deployment;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/webhook", post(handle_webhook))
        .route(
            "/internal/caddy/check-domain",
            axum::routing::get(check_domain),
        )
        .with_state(state)
}

async fn handle_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, StatusCode> {
    verify_signature(&headers, &body, &state.config.webhook_secret).map_err(|_| {
        tracing::warn!("webhook signature verification failed");
        StatusCode::UNAUTHORIZED
    })?;

    let event = parse_event(&headers, &body).map_err(|e| {
        tracing::warn!(error = %e, "failed to parse webhook event");
        StatusCode::BAD_REQUEST
    })?;

    let project = find_or_create_project(&state, &event.repo_name, &event.repo_url)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to find or create project");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    match event.kind {
        EventKind::Push {
            branch,
            commit_sha,
            deleted,
        } => {
            if deleted {
                teardown_branch(&state, &project, &branch).await.map_err(|e| {
                    tracing::error!(project = %project.name, branch = %branch, error = %e, "teardown_branch failed");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
                return Ok(StatusCode::ACCEPTED);
            }

            let git_ref = if branch.starts_with("pr-") {
                let num = branch.strip_prefix("pr-").unwrap();
                format!("refs/pull/{num}/head")
            } else {
                format!("refs/heads/{branch}")
            };

            create_build(&state, &project, &branch, &git_ref, &commit_sha).await.map_err(|e| {
                tracing::error!(project = %project.name, branch = %branch, commit = %commit_sha, error = %e, "create_build failed");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            Ok(StatusCode::OK)
        }
        EventKind::PullRequest {
            action,
            pr_number,
            commit_sha,
        } => {
            let branch = format!("pr-{pr_number}");
            let git_ref = format!("refs/pull/{pr_number}/head");

            match action.as_str() {
                "opened" | "synchronize" | "synchronized" | "reopened" => {
                    create_build(&state, &project, &branch, &git_ref, &commit_sha).await.map_err(|e| {
                        tracing::error!(project = %project.name, branch = %branch, commit = %commit_sha, error = %e, "create_build failed");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
                    Ok(StatusCode::OK)
                }
                "closed" => {
                    teardown_branch(&state, &project, &branch).await.map_err(|e| {
                        tracing::error!(project = %project.name, branch = %branch, error = %e, "teardown_branch failed");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
                    Ok(StatusCode::ACCEPTED)
                }
                _ => Ok(StatusCode::ACCEPTED),
            }
        }
    }
}

async fn teardown_branch(
    state: &AppState,
    project: &::entity::projects::Model,
    branch: &str,
) -> anyhow::Result<()> {
    let deployments = state
        .store
        .deployments()
        .find_by_project_branch(&project.id, branch)
        .await?;

    if deployments.is_empty() {
        return Ok(());
    }

    let systemd = SystemdClient::connect().await?;
    let caddy = CaddyClient::new(state.config.caddy_admin_url.clone());

    for deployment in &deployments {
        if let Err(e) = teardown_deployment(state, deployment, &systemd, &caddy).await {
            tracing::error!(deployment = %deployment.id, error = %e, "teardown failed");
        }
    }

    tracing::info!(
        project = %project.name,
        branch = %branch,
        count = deployments.len(),
        "torn down branch deployments"
    );

    if let Some(pr_number) = crate::forgejo::pr_number_from_branch(branch)
        && let Some((owner, repo)) = crate::forgejo::parse_owner_repo(&project.repo_url)
    {
        let body = format!(
            "### Kennel Deployments\n\nTorn down at {}.\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );
        if let Err(e) = state
            .forgejo
            .upsert_pr_comment(&owner, &repo, pr_number, &body)
            .await
        {
            tracing::warn!(pr = pr_number, error = %e, "failed to post teardown comment");
        }
    }

    Ok(())
}

async fn find_or_create_project(
    state: &AppState,
    repo_name: &str,
    repo_url: &str,
) -> anyhow::Result<::entity::projects::Model> {
    if let Some(project) = state.store.projects().find_by_name(repo_name).await? {
        return Ok(project);
    }

    let id = uuid::Uuid::now_v7().to_string();
    let model = ::entity::projects::ActiveModel {
        id: Set(id),
        name: Set(repo_name.to_string()),
        repo_url: Set(repo_url.to_string()),
        repo_type: Set("forgejo".to_string()),
        default_branch: Set("main".to_string()),
        ..Default::default()
    };

    let project = state.store.projects().upsert(model).await?;
    tracing::info!(project = %repo_name, "auto-registered project");
    Ok(project)
}

async fn create_build(
    state: &AppState,
    project: &::entity::projects::Model,
    branch: &str,
    git_ref: &str,
    commit_sha: &str,
) -> anyhow::Result<()> {
    let build_id = uuid::Uuid::now_v7().to_string();

    let model = ::entity::builds::ActiveModel {
        id: Set(build_id.clone()),
        project_id: Set(project.id.clone()),
        branch: Set(branch.to_string()),
        git_ref: Set(git_ref.to_string()),
        commit_sha: Set(commit_sha.to_string()),
        status: Set("queued".to_string()),
        ..Default::default()
    };

    match state.store.builds().create(model).await {
        Ok(_) => {
            let cancelled = state
                .store
                .builds()
                .cancel_stale(&project.id, branch, &build_id)
                .await?;
            if cancelled > 0 {
                tracing::info!(
                    project = %project.name,
                    branch = %branch,
                    cancelled = cancelled,
                    "cancelled stale builds"
                );
            }
            state.signal.notify_one();
            Ok(())
        }
        Err(e) if e.to_string().contains("UNIQUE constraint failed") => {
            tracing::debug!(project = %project.name, commit = %commit_sha, "build already exists");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

async fn check_domain(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> StatusCode {
    let Some(domain) = params.get("domain") else {
        return StatusCode::BAD_REQUEST;
    };

    match state.store.deployments().find_by_domain(domain).await {
        Ok(Some(_)) => StatusCode::OK,
        _ => StatusCode::NOT_FOUND,
    }
}

struct ParsedEvent {
    repo_name: String,
    repo_url: String,
    kind: EventKind,
}

enum EventKind {
    Push {
        branch: String,
        commit_sha: String,
        deleted: bool,
    },
    PullRequest {
        action: String,
        pr_number: u64,
        commit_sha: String,
    },
}

fn verify_signature(headers: &HeaderMap, body: &[u8], secret: &str) -> anyhow::Result<()> {
    let sig_header = headers
        .get("X-Forgejo-Signature")
        .or_else(|| headers.get("X-Hub-Signature-256"))
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("missing signature header"))?;

    let sig_hex = sig_header.strip_prefix("sha256=").unwrap_or(sig_header);
    let expected = hex::decode(sig_hex)?;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
    mac.update(body);
    mac.verify_slice(&expected)?;

    Ok(())
}

const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

fn parse_event(headers: &HeaderMap, body: &[u8]) -> anyhow::Result<ParsedEvent> {
    let event_type = headers
        .get("X-Forgejo-Event")
        .or_else(|| headers.get("X-GitHub-Event"))
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("missing event header"))?;

    let json: serde_json::Value = serde_json::from_slice(body)?;

    let repo_name = json["repository"]["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing repository.name"))?
        .to_string();

    let repo_url = json["repository"]["clone_url"]
        .as_str()
        .or_else(|| json["repository"]["html_url"].as_str())
        .ok_or_else(|| anyhow::anyhow!("missing repository.clone_url"))?
        .to_string();

    let kind = match event_type {
        "push" => {
            let git_ref = json["ref"].as_str().unwrap_or("");
            let branch = git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref);
            let sha = json["after"].as_str().unwrap_or("");
            let deleted = json["deleted"].as_bool().unwrap_or(false) || sha == ZERO_SHA;

            EventKind::Push {
                branch: branch.to_string(),
                commit_sha: sha.to_string(),
                deleted,
            }
        }
        "pull_request" => {
            let action = json["action"].as_str().unwrap_or("").to_string();
            let pr_number = json["number"].as_u64().unwrap_or(0);
            let sha = json["pull_request"]["head"]["sha"]
                .as_str()
                .unwrap_or("")
                .to_string();

            EventKind::PullRequest {
                action,
                pr_number,
                commit_sha: sha,
            }
        }
        other => anyhow::bail!("unsupported event type: {other}"),
    };

    Ok(ParsedEvent {
        repo_name,
        repo_url,
        kind,
    })
}
