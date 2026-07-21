use crate::AppState;
use crate::forgejo::CommitStatus;
use kennel_config::KennelConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

        let project = state
            .store
            .projects()
            .find_by_id(&build.project_id)
            .await
            .ok()
            .flatten();
        let owner_repo = project.as_ref().and_then(|p| {
            p.owner
                .as_ref()
                .map(|owner| (owner.clone(), p.name.clone()))
        });

        let build_log_url: Option<String> =
            match (state.config.grafana_url.as_deref(), project.as_ref()) {
                (Some(base), Some(p)) => {
                    let unit = format!(
                        "{}.service",
                        crate::deploy::build_unit_name(&p.name, &build.branch)
                    );
                    Some(crate::deploy::drilldown_unit_url(
                        base, &unit, "now-3h", "now",
                    ))
                }
                _ => None,
            };

        if let Some((ref owner, ref repo)) = owner_repo
            && let Err(e) = state
                .forgejo
                .create_commit_status(
                    owner,
                    repo,
                    &build.commit_sha,
                    CommitStatus {
                        state: "pending",
                        description: "build started",
                        context: "kennel/build",
                        target_url: build_log_url.as_deref(),
                    },
                )
                .await
        {
            tracing::error!(build_id = %build.id, error = %e, "failed to post commit status");
            let _ = state.store.builds().set_status(&build.id, "failed").await;
            continue;
        }

        tracing::info!(build_id = %build.id, project = %build.project_id, "processing build");

        let task_state = state.clone();
        let task_build = build.clone();
        let outcome =
            tokio::spawn(async move { process_build(&task_state, &task_build).await }).await;

        match outcome {
            Ok(result) => match result {
                Ok(build_outcome) => {
                    let description = match build_outcome {
                        BuildOutcome::Built => "build succeeded",
                        BuildOutcome::Skipped => "skipped: no services or static sites defined",
                    };
                    if let Some((ref owner, ref repo)) = owner_repo
                        && let Err(e) = state
                            .forgejo
                            .create_commit_status(
                                owner,
                                repo,
                                &build.commit_sha,
                                CommitStatus {
                                    state: "success",
                                    description,
                                    context: "kennel/build",
                                    target_url: build_log_url.as_deref(),
                                },
                            )
                            .await
                    {
                        tracing::error!(build_id = %build.id, error = %e, "failed to post commit status");
                        let _ = state.store.builds().set_status(&build.id, "failed").await;
                        continue;
                    }
                    state.signal.notify_one();
                }
                Err(e) => {
                    tracing::error!(build_id = %build.id, error = %e, "build failed");
                    if let Some((ref owner, ref repo)) = owner_repo
                        && let Err(post_err) = state
                            .forgejo
                            .create_commit_status(
                                owner,
                                repo,
                                &build.commit_sha,
                                CommitStatus {
                                    state: "failure",
                                    description: &format!("{e:#}"),
                                    context: "kennel/build",
                                    target_url: build_log_url.as_deref(),
                                },
                            )
                            .await
                    {
                        tracing::error!(build_id = %build.id, error = %post_err, "failed to post commit status");
                    }
                    let _ = state.store.builds().set_status(&build.id, "failed").await;
                }
            },
            Err(join_err) => {
                tracing::error!(build_id = %build.id, error = %join_err, "build task panicked");
                if let Some((ref owner, ref repo)) = owner_repo
                    && let Err(post_err) = state
                        .forgejo
                        .create_commit_status(
                            owner,
                            repo,
                            &build.commit_sha,
                            CommitStatus {
                                state: "error",
                                description: "build panicked",
                                context: "kennel/build",
                                target_url: build_log_url.as_deref(),
                            },
                        )
                        .await
                {
                    tracing::error!(build_id = %build.id, error = %post_err, "failed to post commit status");
                }
                let _ = state.store.builds().set_status(&build.id, "failed").await;
            }
        }
    }
}

/// The daemon hands these non-secret inputs to the isolated build through a work-dir file.
#[derive(Serialize, Deserialize)]
struct BuildInput {
    repo_url: String,
    git_ref: String,
    commit_sha: String,
}

/// Result of a finished build for a commit
enum BuildOutcome {
    Built,
    Skipped,
}

/// The isolated build hands these results back through a work-dir file.
#[derive(Serialize, Deserialize)]
struct BuildOutput {
    store_paths: HashMap<String, String>,
    kennel_config: KennelConfig,
    config_store_path: Option<String>,
}

const INPUT_FILE: &str = "input.json";
const OUTPUT_FILE: &str = "result.json";
const LOG_FILE: &str = "build.log";

// Stream a line to stdout for journald and accumulate it for the daemon's archive.
fn log_line(buf: &mut String, line: &str) {
    println!("{line}");
    buf.push_str(line);
    buf.push('\n');
}

fn set_group_writable(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o2770);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

/// Runs a build in a separate systemd unit and records its result.
async fn process_build(
    state: &AppState,
    build: &::entity::builds::Model,
) -> anyhow::Result<BuildOutcome> {
    let work_dir = PathBuf::from(&state.config.work_dir).join(&build.id);
    let _ = tokio::fs::remove_dir_all(&work_dir).await;
    tokio::fs::create_dir_all(&work_dir).await?;

    let project = state
        .store
        .projects()
        .find_by_id(&build.project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project {} not found", build.project_id))?;

    let input = BuildInput {
        repo_url: project.repo_url.clone(),
        git_ref: build.git_ref.clone(),
        commit_sha: build.commit_sha.clone(),
    };
    tokio::fs::write(work_dir.join(INPUT_FILE), serde_json::to_vec(&input)?).await?;

    let home = work_dir.join("home");
    // Build-exec creates $HOME so the ephemeral build user owns it
    set_group_writable(&work_dir)?;

    let unit_name = crate::deploy::build_unit_name(&project.name, &build.branch);
    let kennel_bin = std::env::current_exe()?.to_string_lossy().into_owned();
    let argv = vec![kennel_bin, "build-exec".to_string(), build.id.clone()];

    let mut env: HashMap<String, String> = HashMap::new();
    if let Ok(path) = std::env::var("PATH") {
        env.insert("PATH".to_string(), path);
    }
    env.insert("HOME".to_string(), home.to_string_lossy().into_owned());
    env.insert("WORK_DIR".to_string(), state.config.work_dir.clone());

    let systemd = crate::systemd::SystemdClient::connect().await?;
    let success = systemd
        .run_build_unit(
            &unit_name,
            &argv,
            &work_dir.to_string_lossy(),
            &env,
            kennel_config::constants::KENNEL_BUILD_GROUP,
            kennel_config::constants::BUILD_TIMEOUT,
        )
        .await?;

    if let Ok(log) = tokio::fs::read_to_string(work_dir.join(LOG_FILE)).await
        && let Err(e) = state.store.builds().set_log(&build.id, &log).await
    {
        tracing::warn!(build_id = %build.id, error = %e, "failed to persist build log");
    }

    if !success {
        let _ = tokio::fs::remove_dir_all(&work_dir).await;
        anyhow::bail!("build failed");
    }

    let output: BuildOutput = serde_json::from_slice(
        &tokio::fs::read(work_dir.join(OUTPUT_FILE))
            .await
            .map_err(|e| anyhow::anyhow!("build unit produced no result file: {e}"))?,
    )?;

    if output.kennel_config.services.is_empty() && output.kennel_config.static_sites.is_empty() {
        state
            .store
            .builds()
            .set_status(&build.id, "skipped")
            .await?;
        state
            .store
            .deploy_requests()
            .delete_by_project_branch(&build.project_id, &build.branch)
            .await?;
        let _ = tokio::fs::remove_dir_all(&work_dir).await;
        return Ok(BuildOutcome::Skipped);
    }

    if let Ok(cache_name) = std::env::var("CACHIX_CACHE_NAME") {
        let paths: Vec<&str> = output.store_paths.values().map(String::as_str).collect();
        if let Err(e) = cachix_push(&cache_name, &paths).await {
            tracing::warn!(error = %e, "cachix push failed");
        }
    }

    // Pin build outputs against nix-gc until a deployment inherits the roots
    for (name, store_path) in &output.store_paths {
        let gc_name = format!("{}-{name}", build.id);
        if let Err(e) = crate::deploy::add_gc_root(&gc_name, store_path).await {
            tracing::warn!(build_id = %build.id, error = %e, "failed to pin build gc root");
        }
    }
    if let Some(config_path) = &output.config_store_path {
        let gc_name = format!("{}-config", build.id);
        if let Err(e) = crate::deploy::add_gc_root(&gc_name, config_path).await {
            tracing::warn!(build_id = %build.id, error = %e, "failed to pin kennel-config gc root");
        }
    }

    state
        .store
        .builds()
        .set_result(
            &build.id,
            &serde_json::to_string(&output.store_paths)?,
            &serde_json::to_string(&output.kennel_config)?,
            output.config_store_path.as_deref(),
        )
        .await?;

    let _ = tokio::fs::remove_dir_all(&work_dir).await;
    Ok(BuildOutcome::Built)
}

/// Entry point for the `kennel build-exec <id>` subcommand.
pub async fn build_exec(build_id: &str) -> anyhow::Result<()> {
    let work_root = std::env::var("WORK_DIR")
        .unwrap_or_else(|_| kennel_config::constants::DEFAULT_WORK_DIR.to_string());
    let work_dir = PathBuf::from(work_root).join(build_id);

    // Nix rejects a $HOME it does not own, so create it as the build user
    tokio::fs::create_dir_all(work_dir.join("home")).await?;

    let mut log = String::new();
    let result = build_pipeline(&work_dir, &mut log).await;
    if let Err(ref e) = result {
        log_line(&mut log, &format!("=== build failed: {e:#} ==="));
    }
    let _ = tokio::fs::write(work_dir.join(LOG_FILE), &log).await;

    // Remove the build-user-owned scratch so the daemon can reclaim the work dir
    let _ = tokio::fs::remove_dir_all(work_dir.join("repo")).await;
    let _ = tokio::fs::remove_dir_all(work_dir.join("home")).await;

    let output = result?;
    tokio::fs::write(work_dir.join(OUTPUT_FILE), serde_json::to_vec(&output)?).await?;
    Ok(())
}

async fn build_pipeline(work_dir: &Path, log: &mut String) -> anyhow::Result<BuildOutput> {
    let input: BuildInput =
        serde_json::from_slice(&tokio::fs::read(work_dir.join(INPUT_FILE)).await?)?;

    let repo_path = git_clone(
        log,
        &input.repo_url,
        &input.git_ref,
        &input.commit_sha,
        work_dir,
    )
    .await?;

    let (kennel_config, config_store_path) = eval_kennel_config(log, &repo_path).await?;

    if kennel_config.services.is_empty() && kennel_config.static_sites.is_empty() {
        log_line(log, "no services or static sites defined, skipping deploy");
    }

    let mut store_paths: HashMap<String, String> = HashMap::new();
    for name in kennel_config.services.keys() {
        store_paths.insert(name.clone(), nix_build(log, &repo_path, name).await?);
    }
    for (name, site_config) in &kennel_config.static_sites {
        let attr = site_config.package_attr.as_deref().unwrap_or(name.as_str());
        store_paths.insert(name.clone(), nix_build(log, &repo_path, attr).await?);
    }

    Ok(BuildOutput {
        store_paths,
        kennel_config,
        config_store_path: (!config_store_path.is_empty()).then_some(config_store_path),
    })
}

async fn git_clone(
    log: &mut String,
    repo_url: &str,
    git_ref: &str,
    expected_sha: &str,
    work_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let repo_path = work_dir.join("repo");

    run_cmd(log, "git-init", "git", &["init", "repo"], work_dir).await?;
    run_cmd(
        log,
        "git-remote",
        "git",
        &["remote", "add", "origin", repo_url],
        &repo_path,
    )
    .await?;
    run_cmd(
        log,
        "git-fetch",
        "git",
        &["fetch", "--depth", "1", "origin", "--", git_ref],
        &repo_path,
    )
    .await?;
    run_cmd(
        log,
        "git-checkout",
        "git",
        &["checkout", "FETCH_HEAD"],
        &repo_path,
    )
    .await?;

    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", "HEAD"]).current_dir(&repo_path);

    let phase = run_streamed(log, "git-rev-parse", &mut cmd).await?;
    let head = phase.stdout.trim().to_string();
    anyhow::ensure!(
        head.starts_with(expected_sha) || expected_sha.starts_with(&head),
        "SHA mismatch: expected {expected_sha}, got {head}"
    );

    Ok(repo_path)
}

/// The manifest needs no secrets, and secretspec would otherwise fail resolving the
/// dev profile's .env, which is absent on the build host.
async fn disable_secretspec_for_kennel_config(repo_path: &Path) -> anyhow::Result<()> {
    let local_yaml = repo_path.join("devenv.local.yaml");
    if local_yaml.exists() {
        return Ok(());
    }
    tokio::fs::write(&local_yaml, "secretspec:\n  enable: false\n").await?;
    Ok(())
}

async fn eval_kennel_config(
    log: &mut String,
    repo_path: &Path,
) -> anyhow::Result<(KennelConfig, String)> {
    disable_secretspec_for_kennel_config(repo_path).await?;

    let mut cmd = Command::new("devenv");
    cmd.args(["build", "scottylabs.kennel.config"])
        .current_dir(repo_path);
    let phase = run_streamed(log, "devenv-build", &mut cmd).await?;

    if !phase.status.success() {
        log_line(log, "no devenv kennel config found, using empty config");
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
    let config: KennelConfig = serde_json::from_str(&content)?;
    if !config.is_compatible() {
        anyhow::bail!(
            "kennel.json schema version {} does not match kennel's expected {}. Run `devenv update` in the project and rebuild.",
            config.version,
            kennel_config::constants::KENNEL_CONFIG_SCHEMA_VERSION
        );
    }

    Ok((config, store_path.to_string()))
}

async fn nix_build(log: &mut String, repo_path: &Path, name: &str) -> anyhow::Result<String> {
    let system = format!("{}-linux", std::env::consts::ARCH);
    let flake_ref = format!(".#packages.{system}.{name}");

    let mut cmd = Command::new("nix");
    cmd.args(["build", &flake_ref, "--no-link", "--print-out-paths"])
        .current_dir(repo_path);
    let phase = run_streamed(log, &format!("nix-build:{name}"), &mut cmd).await?;

    anyhow::ensure!(phase.status.success(), "nix build failed for {name}");
    Ok(phase.stdout.trim().to_string())
}

async fn run_cmd(
    log: &mut String,
    phase: &str,
    program: &str,
    args: &[&str],
    cwd: &Path,
) -> anyhow::Result<()> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd);
    let result = run_streamed(log, phase, &mut cmd).await?;
    anyhow::ensure!(
        result.status.success(),
        "{program} {} failed",
        args.join(" ")
    );
    Ok(())
}

async fn cachix_push(cache_name: &str, paths: &[&str]) -> anyhow::Result<()> {
    let mut args = vec!["push", cache_name];
    args.extend(paths);
    let status = Command::new("cachix").args(&args).status().await?;
    anyhow::ensure!(status.success(), "cachix push failed");
    tracing::info!(cache = %cache_name, count = paths.len(), "pushed to cachix");
    Ok(())
}

struct PhaseRun {
    status: std::process::ExitStatus,
    stdout: String,
}

async fn run_streamed(
    log: &mut String,
    phase: &str,
    cmd: &mut Command,
) -> anyhow::Result<PhaseRun> {
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    log_line(log, &format!("=== phase: {phase} ==="));

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
                    log_line(log, &line);
                    captured_stdout.push_str(&line);
                    captured_stdout.push('\n');
                }
                Ok(None) => break,
                Err(e) => return Err(e.into()),
            },
            line = stderr_reader.next_line() => match line {
                Ok(Some(line)) => log_line(log, &line),
                Ok(None) => {}
                Err(e) => return Err(e.into()),
            },
        }
    }

    while let Ok(Some(line)) = stderr_reader.next_line().await {
        log_line(log, &line);
    }

    let status = child.wait().await?;
    Ok(PhaseRun {
        status,
        stdout: captured_stdout,
    })
}
