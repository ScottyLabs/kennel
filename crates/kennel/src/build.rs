use crate::AppState;
use kennel_config::KennelConfig;
use std::collections::HashMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
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

        let mut log = String::new();
        let result = process_build(&state, &build, &mut log).await;

        if let Err(e) = state.store.builds().set_log(&build.id, &log).await {
            tracing::warn!(build_id = %build.id, error = %e, "failed to persist build log");
        }

        match result {
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

async fn process_build(
    state: &AppState,
    build: &::entity::builds::Model,
    log: &mut String,
) -> anyhow::Result<()> {
    let work_dir = PathBuf::from(&state.config.work_dir).join(&build.id);
    let _ = tokio::fs::remove_dir_all(&work_dir).await;

    let project = state
        .store
        .projects()
        .find_by_id(&build.project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project {} not found", build.project_id))?;

    let repo_path = git_clone(
        &build.id,
        log,
        &project.repo_url,
        &build.git_ref,
        &build.commit_sha,
        &work_dir,
    )
    .await?;

    let (kennel_config, config_store_path) = eval_kennel_config(&build.id, log, &repo_path).await?;

    if kennel_config.services.is_empty() && kennel_config.static_sites.is_empty() {
        anyhow::bail!("no services or static sites defined");
    }

    let mut store_paths: HashMap<String, String> = HashMap::new();

    for name in kennel_config.services.keys() {
        let store_path = nix_build(&build.id, log, &repo_path, name).await?;
        store_paths.insert(name.clone(), store_path);
    }

    for (name, site_config) in &kennel_config.static_sites {
        let attr = site_config.package_attr.as_deref().unwrap_or(name.as_str());
        let store_path = nix_build(&build.id, log, &repo_path, attr).await?;
        store_paths.insert(name.clone(), store_path);
    }

    if let Ok(cache_name) = dotenvy::var("CACHIX_CACHE_NAME") {
        let paths: Vec<&str> = store_paths.values().map(String::as_str).collect();
        if let Err(e) = cachix_push(&build.id, log, &cache_name, &paths).await {
            tracing::warn!(error = %e, "cachix push failed");
        }
    }

    let config_path = if config_store_path.is_empty() {
        None
    } else {
        Some(config_store_path.as_str())
    };

    state
        .store
        .builds()
        .set_result(
            &build.id,
            &serde_json::to_string(&store_paths)?,
            &serde_json::to_string(&kennel_config)?,
            config_path,
        )
        .await?;

    let _ = tokio::fs::remove_dir_all(&work_dir).await;

    Ok(())
}

async fn git_clone(
    build_id: &str,
    log: &mut String,
    repo_url: &str,
    git_ref: &str,
    expected_sha: &str,
    work_dir: &Path,
) -> anyhow::Result<PathBuf> {
    tokio::fs::create_dir_all(work_dir).await?;
    let repo_path = work_dir.join("repo");

    run_cmd(
        build_id,
        "git-init",
        log,
        "git",
        &["init", "repo"],
        work_dir,
    )
    .await?;
    run_cmd(
        build_id,
        "git-remote",
        log,
        "git",
        &["remote", "add", "origin", repo_url],
        &repo_path,
    )
    .await?;
    run_cmd(
        build_id,
        "git-fetch",
        log,
        "git",
        &["fetch", "--depth", "1", "origin", "--", git_ref],
        &repo_path,
    )
    .await?;
    run_cmd(
        build_id,
        "git-checkout",
        log,
        "git",
        &["checkout", "FETCH_HEAD"],
        &repo_path,
    )
    .await?;

    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", "HEAD"]).current_dir(&repo_path);
    let phase = run_streamed(build_id, "git-rev-parse", log, &mut cmd).await?;
    let head = phase.stdout.trim().to_string();
    anyhow::ensure!(
        head.starts_with(expected_sha) || expected_sha.starts_with(&head),
        "SHA mismatch: expected {expected_sha}, got {head}"
    );

    Ok(repo_path)
}

/// `devenv build scottylabs.kennel.config` runs secretspec validation for the dev
/// profile. Projects that source dev secrets from `dotenv://.env` need a file on
/// disk even though kennel.config itself never consumes those values.
async fn ensure_stub_env_for_kennel_config(repo_path: &Path) -> anyhow::Result<()> {
    let env_path = repo_path.join(".env");
    if env_path.exists() {
        return Ok(());
    }

    let secretspec_path = repo_path.join("secretspec.toml");
    if !secretspec_path.exists() {
        return Ok(());
    }

    let content = tokio::fs::read_to_string(&secretspec_path).await?;
    let stub = local_dev_env_stub(&content);
    if stub.is_empty() {
        return Ok(());
    }

    tokio::fs::write(&env_path, stub).await?;
    Ok(())
}

fn local_dev_env_stub(secretspec: &str) -> String {
    let mut in_dev = false;
    let mut lines = Vec::new();

    for line in secretspec.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_dev = trimmed == "[profiles.dev]";
            continue;
        }
        if !in_dev || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !trimmed.contains("local") {
            continue;
        }
        let Some(key) = trimmed.split('=').next().map(str::trim) else {
            continue;
        };
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            continue;
        }
        lines.push(format!("{key}=kennel-build-placeholder"));
    }

    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

async fn eval_kennel_config(
    build_id: &str,
    log: &mut String,
    repo_path: &Path,
) -> anyhow::Result<(KennelConfig, String)> {
    ensure_stub_env_for_kennel_config(repo_path).await?;

    let mut cmd = Command::new("devenv");
    cmd.args(["build", "scottylabs.kennel.config"])
        .current_dir(repo_path);
    let phase = run_streamed(build_id, "devenv-build", log, &mut cmd).await?;

    if !phase.status.success() {
        tracing::info!("no devenv kennel config found, using empty config");
        return Ok((KennelConfig::default(), String::new()));
    }

    let lines: Vec<&str> = phase.stdout.lines().collect();
    let json_start = lines
        .iter()
        .rposition(|line| line.starts_with('{'))
        .ok_or_else(|| anyhow::anyhow!("no JSON object in devenv build output"))?;
    let build_result: serde_json::Value = serde_json::from_str(&lines[json_start..].join("\n"))?;
    let store_path = build_result["scottylabs.kennel.config"]
        .as_str()
        .ok_or_else(|| {
            anyhow::anyhow!("missing scottylabs.kennel.config in devenv build output")
        })?;

    let json_path = PathBuf::from(store_path).join("kennel.json");
    let content = tokio::fs::read_to_string(&json_path).await?;
    Ok((serde_json::from_str(&content)?, store_path.to_string()))
}

async fn nix_build(
    build_id: &str,
    log: &mut String,
    repo_path: &Path,
    name: &str,
) -> anyhow::Result<String> {
    let system = format!("{}-linux", std::env::consts::ARCH);
    let flake_ref = format!(".#packages.{system}.{name}");

    tracing::info!(build_id, package = %name, "building");

    let mut cmd = Command::new("nix");
    cmd.args(["build", &flake_ref, "--no-link", "--print-out-paths"])
        .current_dir(repo_path);

    let phase = tokio::time::timeout(
        kennel_config::constants::BUILD_TIMEOUT,
        run_streamed(build_id, &format!("nix-build:{name}"), log, &mut cmd),
    )
    .await
    .map_err(|_| anyhow::anyhow!("build timed out for {name}"))??;

    anyhow::ensure!(phase.status.success(), "nix build failed for {name}");
    Ok(phase.stdout.trim().to_string())
}

async fn run_cmd(
    build_id: &str,
    phase: &str,
    log: &mut String,
    program: &str,
    args: &[&str],
    cwd: &Path,
) -> anyhow::Result<()> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd);
    let result = run_streamed(build_id, phase, log, &mut cmd).await?;
    anyhow::ensure!(
        result.status.success(),
        "{program} {} failed",
        args.join(" ")
    );
    Ok(())
}

async fn cachix_push(
    build_id: &str,
    log: &mut String,
    cache_name: &str,
    paths: &[&str],
) -> anyhow::Result<()> {
    let mut args = vec!["push", cache_name];
    args.extend(paths);

    let mut cmd = Command::new("cachix");
    cmd.args(&args);
    let phase = run_streamed(build_id, "cachix-push", log, &mut cmd).await?;
    anyhow::ensure!(phase.status.success(), "cachix push failed");

    tracing::info!(build_id, cache = %cache_name, count = paths.len(), "pushed to cachix");
    Ok(())
}

struct PhaseRun {
    status: std::process::ExitStatus,
    stdout: String,
}

async fn run_streamed(
    build_id: &str,
    phase: &str,
    log: &mut String,
    cmd: &mut Command,
) -> anyhow::Result<PhaseRun> {
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    writeln!(log, "=== phase: {phase} ===").ok();

    let mut child = cmd.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stderr"))?;

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();
    let mut captured_stdout = String::new();

    loop {
        tokio::select! {
            line = stdout_reader.next_line() => match line {
                Ok(Some(line)) => {
                    tracing::info!(build_id, phase, "{}", line);
                    writeln!(log, "{}", line).ok();
                    captured_stdout.push_str(&line);
                    captured_stdout.push('\n');
                }
                Ok(None) => break,
                Err(e) => return Err(e.into()),
            },
            line = stderr_reader.next_line() => match line {
                Ok(Some(line)) => {
                    tracing::warn!(build_id, phase, "{}", line);
                    writeln!(log, "{}", line).ok();
                }
                Ok(None) => {}
                Err(e) => return Err(e.into()),
            },
        }
    }

    while let Ok(Some(line)) = stderr_reader.next_line().await {
        tracing::warn!(build_id, phase, "{}", line);
        writeln!(log, "{}", line).ok();
    }

    let status = child.wait().await?;
    Ok(PhaseRun {
        status,
        stdout: captured_stdout,
    })
}

#[cfg(test)]
mod tests {
    use super::local_dev_env_stub;

    #[test]
    fn stub_env_includes_only_dev_secrets_using_local_provider() {
        let secretspec = r#"
[providers]
local = "dotenv://.env"

[profiles.dev]
DISCORD_TOKEN = { providers = ["local"] }
DISCORD_CLIENT_ID = { providers = ["local"] }
GOOGLE_MAPS_API_KEY = { required = false, providers = ["vault", "local"] }

[profiles.prod]
DISCORD_TOKEN = { description = "prod" }
"#;
        let stub = local_dev_env_stub(secretspec);
        assert!(stub.contains("DISCORD_TOKEN=kennel-build-placeholder"));
        assert!(stub.contains("DISCORD_CLIENT_ID=kennel-build-placeholder"));
        assert!(stub.contains("GOOGLE_MAPS_API_KEY=kennel-build-placeholder"));
        assert!(!stub.contains("prod"));
    }

    #[test]
    fn stub_env_empty_when_dev_profile_has_no_local_provider() {
        let secretspec = r#"
[profiles.dev]

[profiles.prod]
DISCORD_TOKEN = { description = "prod" }
"#;
        assert!(local_dev_env_stub(secretspec).is_empty());
    }
}
