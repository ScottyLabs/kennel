use crate::AppState;
use kennel_config::KennelConfig;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

pub async fn run_worker(state: Arc<AppState>, cancel: CancellationToken) {
    loop {
        let build = match state.store.builds().find_by_status("queued").await {
            Ok(builds) if !builds.is_empty() => builds.into_iter().next().unwrap(),
            Ok(_) => {
                tokio::select! {
                    _ = state.signal.notified() => continue,
                    _ = cancel.cancelled() => break,
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to query builds");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        if let Err(e) = state.store.builds().set_status(&build.id, "building").await {
            tracing::error!(build_id = %build.id, error = %e, "failed to mark building");
            continue;
        }

        tracing::info!(build_id = %build.id, project = %build.project_id, "processing build");

        match process_build(&state, &build).await {
            Ok(()) => {
                state.signal.notify_one();
            }
            Err(e) => {
                tracing::error!(build_id = %build.id, error = %e, "build failed");
                let _ = state.store.builds().set_status(&build.id, "failed").await;
            }
        }
    }
}

async fn process_build(state: &AppState, build: &::entity::builds::Model) -> anyhow::Result<()> {
    let work_dir = PathBuf::from(&state.config.work_dir).join(&build.id);
    let _ = tokio::fs::remove_dir_all(&work_dir).await;

    let project = state
        .store
        .projects()
        .find_by_id(&build.project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project {} not found", build.project_id))?;

    let repo_path = git_clone(
        &project.repo_url,
        &build.git_ref,
        &build.commit_sha,
        &work_dir,
    )
    .await?;

    let kennel_config = eval_kennel_config(&repo_path).await?;

    if kennel_config.services.is_empty() && kennel_config.static_sites.is_empty() {
        anyhow::bail!("no services or static sites defined");
    }

    let mut store_paths: HashMap<String, String> = HashMap::new();

    for name in kennel_config.services.keys() {
        let store_path = nix_build(&repo_path, name).await?;
        store_paths.insert(name.clone(), store_path);
    }

    for (name, site_config) in &kennel_config.static_sites {
        let attr = site_config.package_attr.as_deref().unwrap_or(name.as_str());
        let store_path = nix_build(&repo_path, attr).await?;
        store_paths.insert(name.clone(), store_path);
    }

    if let Ok(cache_name) = dotenvy::var("CACHIX_CACHE_NAME") {
        let paths: Vec<&str> = store_paths.values().map(String::as_str).collect();
        if let Err(e) = cachix_push(&cache_name, &paths).await {
            tracing::warn!(error = %e, "cachix push failed");
        }
    }

    state
        .store
        .builds()
        .set_result(
            &build.id,
            &serde_json::to_string(&store_paths)?,
            &serde_json::to_string(&kennel_config)?,
        )
        .await?;

    let _ = tokio::fs::remove_dir_all(&work_dir).await;

    Ok(())
}

async fn git_clone(
    repo_url: &str,
    git_ref: &str,
    expected_sha: &str,
    work_dir: &Path,
) -> anyhow::Result<PathBuf> {
    tokio::fs::create_dir_all(work_dir).await?;
    let repo_path = work_dir.join("repo");

    run_cmd("git", &["init", "repo"], work_dir).await?;
    run_cmd("git", &["remote", "add", "origin", repo_url], &repo_path).await?;
    run_cmd(
        "git",
        &["fetch", "--depth", "1", "origin", "--", git_ref],
        &repo_path,
    )
    .await?;
    run_cmd("git", &["checkout", "FETCH_HEAD"], &repo_path).await?;

    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repo_path)
        .output()
        .await?;
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    anyhow::ensure!(
        head.starts_with(expected_sha) || expected_sha.starts_with(&head),
        "SHA mismatch: expected {expected_sha}, got {head}"
    );

    Ok(repo_path)
}

async fn eval_kennel_config(repo_path: &Path) -> anyhow::Result<KennelConfig> {
    let output = Command::new("nix")
        .args([
            "build",
            ".#devenv.shells.default.config.kennel.config",
            "--no-link",
            "--print-out-paths",
        ])
        .current_dir(repo_path)
        .output()
        .await?;

    if !output.status.success() {
        tracing::info!("no devenv kennel config found, using empty config");
        return Ok(KennelConfig::default());
    }

    let store_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let json_path = PathBuf::from(&store_path).join("kennel.json");
    let content = tokio::fs::read_to_string(&json_path).await?;
    Ok(serde_json::from_str(&content)?)
}

async fn nix_build(repo_path: &Path, name: &str) -> anyhow::Result<String> {
    let system = format!("{}-linux", std::env::consts::ARCH);
    let flake_ref = format!(".#packages.{system}.{name}");

    tracing::info!(package = %name, "building");

    let output = tokio::time::timeout(
        kennel_config::constants::BUILD_TIMEOUT,
        Command::new("nix")
            .args(["build", &flake_ref, "--no-link", "--print-out-paths"])
            .current_dir(repo_path)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("build timed out for {name}"))??;

    anyhow::ensure!(
        output.status.success(),
        "nix build failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn run_cmd(program: &str, args: &[&str], cwd: &Path) -> anyhow::Result<()> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .await?;
    anyhow::ensure!(
        output.status.success(),
        "{} {} failed: {}",
        program,
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

async fn cachix_push(cache_name: &str, paths: &[&str]) -> anyhow::Result<()> {
    let mut args = vec!["push", cache_name];
    args.extend(paths);

    let output = Command::new("cachix").args(&args).output().await?;

    anyhow::ensure!(
        output.status.success(),
        "cachix push failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    tracing::info!(cache = %cache_name, count = paths.len(), "pushed to cachix");
    Ok(())
}
