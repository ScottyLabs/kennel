use std::time::Duration;

use crate::config::ReadyConfig;

pub async fn run_readiness_probe(
    name: &str,
    config: &ReadyConfig,
    user: Option<&str>,
) -> crate::Result<()> {
    tokio::time::sleep(config.initial_delay).await;

    let deadline = config.timeout.map(|t| tokio::time::Instant::now() + t);
    let mut successes = 0u32;
    let mut failures = 0u32;

    loop {
        if deadline.is_some_and(|d| tokio::time::Instant::now() > d) {
            return Err(crate::SupervisorError::ProbeTimeout(name.to_string()));
        }

        match run_single_probe(config, user).await {
            Ok(()) => {
                failures = 0;
                successes += 1;
                if successes >= config.success_threshold {
                    return Ok(());
                }
            }
            Err(e) => {
                successes = 0;
                failures += 1;
                tracing::debug!(process = name, failures, "readiness probe failed: {e}");
                if failures >= config.failure_threshold {
                    return Err(crate::SupervisorError::ProcessFailed {
                        name: name.to_string(),
                        reason: format!("{failures} consecutive probe failures"),
                    });
                }
            }
        }

        tokio::time::sleep(config.period).await;
    }
}

pub async fn run_single_probe(config: &ReadyConfig, user: Option<&str>) -> anyhow::Result<()> {
    if config.notify {
        // Notify readiness is handled by the notify socket listener,
        // not by polling. Return error so the readiness probe loop
        // delegates to the notify path.
        anyhow::bail!("notify probe handled externally");
    }

    if let Some(http) = &config.http
        && let Some(get) = &http.get
    {
        return probe_http(get, config.probe_timeout).await;
    }

    if let Some(exec) = &config.exec {
        return probe_exec(exec, config.probe_timeout, user).await;
    }

    Ok(())
}

async fn probe_http(config: &crate::config::HttpProbe, timeout: Duration) -> anyhow::Result<()> {
    let url = format!(
        "{}://{}:{}{}",
        config.scheme, config.host, config.port, config.path
    );

    let client = reqwest::Client::builder()
        .connect_timeout(timeout)
        .timeout(timeout)
        .build()?;

    let response = client.get(&url).send().await?;

    if response.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("HTTP probe returned status {}", response.status())
    }
}

async fn probe_exec(command: &str, timeout: Duration, user: Option<&str>) -> anyhow::Result<()> {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(command);

    #[cfg(target_os = "linux")]
    if let Some(username) = user {
        if let Ok(Some(user_info)) = nix::unistd::User::from_name(username) {
            cmd.uid(user_info.uid.as_raw());
            cmd.gid(user_info.gid.as_raw());
        }
    }

    let result = tokio::time::timeout(timeout, cmd.output()).await??;

    if result.status.success() {
        Ok(())
    } else {
        anyhow::bail!("exec probe exited with status {}", result.status)
    }
}

/// Sleeps for the probe period. Used in the supervision task's select! loop.
pub async fn liveness_tick(ready: &Option<ReadyConfig>) {
    if let Some(config) = ready {
        tokio::time::sleep(config.period).await;
    } else {
        std::future::pending::<()>().await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::config::ReadyConfig;

    use super::*;

    fn ready_config_with_exec(cmd: &str) -> ReadyConfig {
        ReadyConfig {
            exec: Some(cmd.to_string()),
            http: None,
            notify: false,
            initial_delay: Duration::ZERO,
            period: Duration::from_millis(100),
            probe_timeout: Duration::from_secs(5),
            timeout: Some(Duration::from_secs(10)),
            success_threshold: 1,
            failure_threshold: 3,
        }
    }

    #[tokio::test]
    async fn exec_probe_success() {
        let result = probe_exec("true", Duration::from_secs(5), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn exec_probe_failure() {
        let result = probe_exec("false", Duration::from_secs(5), None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn exec_probe_timeout() {
        let result = probe_exec("sleep 60", Duration::from_millis(100), None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn readiness_probe_exec_passes() {
        let config = ready_config_with_exec("true");
        let result = run_readiness_probe("test", &config, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn readiness_probe_exec_fails_threshold() {
        let mut config = ready_config_with_exec("false");
        config.failure_threshold = 2;
        config.period = Duration::from_millis(50);

        let result = run_readiness_probe("test", &config, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn readiness_probe_timeout() {
        let mut config = ready_config_with_exec("sleep 60");
        config.timeout = Some(Duration::from_millis(200));
        config.period = Duration::from_millis(50);
        config.probe_timeout = Duration::from_millis(50);

        let result = run_readiness_probe("test", &config, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn readiness_probe_no_probe_configured() {
        let config = ReadyConfig {
            exec: None,
            http: None,
            notify: false,
            initial_delay: Duration::ZERO,
            period: Duration::from_secs(10),
            probe_timeout: Duration::from_secs(4),
            timeout: None,
            success_threshold: 1,
            failure_threshold: 5,
        };

        let result = run_readiness_probe("test", &config, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn readiness_probe_initial_delay() {
        let mut config = ready_config_with_exec("true");
        config.initial_delay = Duration::from_millis(100);

        let start = tokio::time::Instant::now();
        let result = run_readiness_probe("test", &config, None).await;
        assert!(result.is_ok());
        assert!(start.elapsed() >= Duration::from_millis(100));
    }

    #[tokio::test]
    async fn readiness_probe_success_threshold() {
        let mut config = ready_config_with_exec("true");
        config.success_threshold = 3;
        config.period = Duration::from_millis(50);

        let start = tokio::time::Instant::now();
        let result = run_readiness_probe("test", &config, None).await;
        assert!(result.is_ok());
        // Should take at least 2 periods (3 successes with sleep between each)
        assert!(start.elapsed() >= Duration::from_millis(100));
    }

    #[tokio::test]
    async fn single_probe_no_config() {
        let config = ReadyConfig {
            exec: None,
            http: None,
            notify: false,
            initial_delay: Duration::ZERO,
            period: Duration::from_secs(10),
            probe_timeout: Duration::from_secs(4),
            timeout: None,
            success_threshold: 1,
            failure_threshold: 5,
        };

        let result = run_single_probe(&config, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn single_probe_notify_returns_error() {
        let config = ReadyConfig {
            exec: None,
            http: None,
            notify: true,
            initial_delay: Duration::ZERO,
            period: Duration::from_secs(10),
            probe_timeout: Duration::from_secs(4),
            timeout: None,
            success_threshold: 1,
            failure_threshold: 5,
        };

        let result = run_single_probe(&config, None).await;
        assert!(result.is_err());
    }
}
