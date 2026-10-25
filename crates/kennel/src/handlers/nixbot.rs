use axum::{
    Form,
    extract::State,
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::AppState;
use kennel_config::Environment;

/// Properties nixbot's post-build step sends for each built attribute
#[derive(Deserialize)]
pub struct BuiltAttribute {
    /// `owner/repo`
    project: String,
    attr: String,
    /// The pushed branch, or the target branch of a pull request
    branch: String,
    revision: String,
    #[serde(default)]
    pr_number: String,
    out_path: String,
}

pub async fn handle(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(built): Form<BuiltAttribute>,
) -> Result<StatusCode, StatusCode> {
    if !authorized(&headers, &state.config.nixbot_token) {
        tracing::warn!("nixbot notification with a bad token");
        return Err(StatusCode::UNAUTHORIZED);
    }

    if built.attr != manifest_attr() {
        return Ok(StatusCode::NO_CONTENT);
    }

    if !is_store_path(&built.out_path) || !is_commit(&built.revision) {
        tracing::warn!(out_path = %built.out_path, revision = %built.revision, "malformed nixbot notification");
        return Err(StatusCode::BAD_REQUEST);
    }

    let branch = match built.pr_number.as_str() {
        "" => built.branch.clone(),
        pr => format!("pr-{pr}"),
    };
    if Environment::from_branch(&branch).is_none() {
        return Ok(StatusCode::NO_CONTENT);
    }

    let repo = built.project.rsplit('/').next().unwrap_or(&built.project);
    let project = state
        .store
        .projects()
        .find_by_name(repo)
        .await
        .map_err(internal)?
        .ok_or_else(|| {
            tracing::warn!(project = %built.project, "nixbot built a project kennel has no webhook for");
            StatusCode::NOT_FOUND
        })?;

    state
        .store
        .artifacts()
        .upsert(&project.id, &branch, &built.revision, &built.out_path)
        .await
        .map_err(internal)?;

    tracing::info!(
        project = %project.name,
        branch = %branch,
        commit = %built.revision,
        out_path = %built.out_path,
        "recorded artifact",
    );

    state.reconcile_signal.notify_one();
    Ok(StatusCode::ACCEPTED)
}

/// The deploy manifests the flake module adds to `checks`
fn manifest_attr() -> String {
    format!("checks.{}-linux.kennel", std::env::consts::ARCH)
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    let Some(presented) = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return false;
    };

    // Comparing digests keeps the comparison from leaking the token's prefix
    Sha256::digest(presented.as_bytes()) == Sha256::digest(token.as_bytes())
}

fn is_store_path(path: &str) -> bool {
    let Some(name) = path.strip_prefix("/nix/store/") else {
        return false;
    };

    let (hash, rest) = name.split_at_checked(32).unwrap_or(("", ""));
    hash.bytes().all(|b| b.is_ascii_alphanumeric())
        && rest.starts_with('-')
        && !rest.contains('/')
        && !hash.is_empty()
}

fn is_commit(sha: &str) -> bool {
    matches!(sha.len(), 40 | 64) && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

fn internal<E: std::fmt::Display>(e: E) -> StatusCode {
    tracing::error!(error = %e, "nixbot notification failed");
    StatusCode::INTERNAL_SERVER_ERROR
}
