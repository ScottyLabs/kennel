use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use hmac::{Hmac, Mac, digest::KeyInit};
use sea_orm::ActiveValue::Set;
use sha2::Sha256;
use std::sync::Arc;

use crate::AppState;
use crate::caddy::CaddyClient;
use crate::systemd::SystemdClient;
use crate::teardown::teardown_deployment;
use kennel_config::Environment;

pub async fn handle(
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

    let project = find_or_create_project(&state, &event.repo_name, &event.repo_url, &event.owner)
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

            if !matches!(
                Environment::from_branch(&branch),
                Some(Environment::Prod | Environment::Staging | Environment::Dev)
            ) {
                tracing::info!(
                    project = %project.name,
                    branch = %branch,
                    "ignoring push to non-deployable branch; open a PR for a preview"
                );
                return Ok(StatusCode::ACCEPTED);
            }

            let git_ref = format!("refs/heads/{branch}");

            enqueue_deploy(&state, &project, &branch, &git_ref, &commit_sha).await.map_err(|e| {
                tracing::error!(project = %project.name, branch = %branch, commit = %commit_sha, error = %e, "enqueue_deploy failed");
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
                    enqueue_deploy(&state, &project, &branch, &git_ref, &commit_sha).await.map_err(|e| {
                        tracing::error!(project = %project.name, branch = %branch, commit = %commit_sha, error = %e, "enqueue_deploy failed");
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
    let _ = state
        .store
        .deploy_requests()
        .delete_by_project_branch(&project.id, branch)
        .await;

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
        && let Some(owner) = project.owner.as_deref()
    {
        let body = format!(
            "### Kennel Deployments\n\nTorn down at {}.\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );
        if let Err(e) = state
            .forgejo
            .upsert_pr_comment(owner, &project.name, pr_number, &body)
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
    owner: &str,
) -> anyhow::Result<::entity::projects::Model> {
    if let Some(mut project) = state.store.projects().find_by_name(repo_name).await? {
        if project.owner.as_deref() != Some(owner) {
            state.store.projects().set_owner(&project.id, owner).await?;
            project.owner = Some(owner.to_string());
        }
        return Ok(project);
    }

    let id = uuid::Uuid::now_v7().to_string();
    let model = ::entity::projects::ActiveModel {
        id: Set(id),
        name: Set(repo_name.to_string()),
        repo_url: Set(repo_url.to_string()),
        owner: Set(Some(owner.to_string())),
        repo_type: Set("forgejo".to_string()),
        default_branch: Set("main".to_string()),
        ..Default::default()
    };

    let project = state.store.projects().upsert(model).await?;
    tracing::info!(project = %repo_name, "auto-registered project");
    Ok(project)
}

/// Ensure the commit's artifacts are built and record the branch's deploy intent
async fn enqueue_deploy(
    state: &AppState,
    project: &::entity::projects::Model,
    branch: &str,
    git_ref: &str,
    commit_sha: &str,
) -> anyhow::Result<()> {
    // One build per commit is reused across every branch
    match state
        .store
        .builds()
        .find_by_project_commit(&project.id, commit_sha)
        .await?
    {
        Some(existing) => {
            if matches!(existing.status.as_str(), "failed" | "cancelled") {
                state.store.builds().requeue(&existing.id).await?;
                tracing::info!(
                    project = %project.name,
                    commit = %commit_sha,
                    previous_status = %existing.status,
                    "requeued build for redelivery"
                );
            }
        }
        None => {
            let model = ::entity::builds::ActiveModel {
                id: Set(uuid::Uuid::now_v7().to_string()),
                project_id: Set(project.id.clone()),
                branch: Set(branch.to_string()),
                git_ref: Set(git_ref.to_string()),
                commit_sha: Set(commit_sha.to_string()),
                status: Set("queued".to_string()),
                ..Default::default()
            };
            state.store.builds().create(model).await?;
        }
    }

    state
        .store
        .deploy_requests()
        .upsert(&project.id, branch, git_ref, commit_sha)
        .await?;

    let referenced = state
        .store
        .deploy_requests()
        .active_commits(&project.id)
        .await?;
    if let Ok(cancelled) = state
        .store
        .builds()
        .cancel_unreferenced(&project.id, &referenced)
        .await
        && cancelled > 0
    {
        tracing::info!(
            project = %project.name,
            cancelled = cancelled,
            "cancelled superseded builds"
        );
    }

    state.signal.notify_one();
    Ok(())
}

struct ParsedEvent {
    repo_name: String,
    repo_url: String,
    owner: String,
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

    let owner = json["repository"]["owner"]["login"]
        .as_str()
        .or_else(|| json["repository"]["owner"]["username"].as_str())
        .ok_or_else(|| anyhow::anyhow!("missing repository.owner.login"))?
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
        "delete" => {
            let ref_name = json["ref"].as_str().unwrap_or("");
            let ref_type = json["ref_type"].as_str().unwrap_or("");
            if ref_type != "branch" {
                anyhow::bail!("ignoring delete of {ref_type}");
            }
            EventKind::Push {
                branch: ref_name.to_string(),
                commit_sha: ZERO_SHA.to_string(),
                deleted: true,
            }
        }
        other => anyhow::bail!("unsupported event type: {other}"),
    };

    Ok(ParsedEvent {
        repo_name,
        repo_url,
        owner,
        kind,
    })
}
