use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use hmac::{Hmac, Mac, digest::KeyInit};
use sha2::Sha256;
use std::sync::Arc;

use crate::AppState;

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
    let event = parse_event(&headers, &body).map_err(|e| {
        tracing::warn!(error = %e, "failed to parse webhook event");
        StatusCode::BAD_REQUEST
    })?;

    let project = state
        .store
        .projects()
        .find_by_name(&event.repo_name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    verify_signature(&headers, &body, &project.webhook_secret).map_err(|_| {
        tracing::warn!(project = %event.repo_name, "signature verification failed");
        StatusCode::UNAUTHORIZED
    })?;

    match event.kind {
        EventKind::Push {
            branch,
            commit_sha,
            deleted,
        } => {
            if deleted {
                // TODO: mark deployments for teardown
                state.signal.notify_one();
                return Ok(StatusCode::ACCEPTED);
            }

            let git_ref = if branch.starts_with("pr-") {
                let num = branch.strip_prefix("pr-").unwrap();
                format!("refs/pull/{num}/head")
            } else {
                format!("refs/heads/{branch}")
            };

            // TODO: create build record, cancel stale builds
            state.signal.notify_one();
            Ok(StatusCode::OK)
        }
        EventKind::PullRequest {
            action,
            pr_number,
            commit_sha,
        } => {
            let branch = format!("pr-{pr_number}");

            match action.as_str() {
                "opened" | "synchronize" | "synchronized" | "reopened" => {
                    // TODO: create build record
                    state.signal.notify_one();
                    Ok(StatusCode::OK)
                }
                "closed" => {
                    // TODO: mark deployments for teardown
                    state.signal.notify_one();
                    Ok(StatusCode::ACCEPTED)
                }
                _ => Ok(StatusCode::ACCEPTED),
            }
        }
    }
}

async fn check_domain(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> StatusCode {
    let Some(domain) = params.get("domain") else {
        return StatusCode::BAD_REQUEST;
    };

    // TODO: check if domain belongs to an active deployment
    StatusCode::NOT_FOUND
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
