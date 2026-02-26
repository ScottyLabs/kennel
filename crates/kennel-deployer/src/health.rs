use crate::error::Result;
use std::time::Duration;
use tracing::{info, warn};

pub async fn check_health(port: u16, path: &str, timeout_secs: u64) -> Result<()> {
    let url = format!("http://127.0.0.1:{}{}", port, path);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(timeout_secs);
    let backoff_intervals = [1, 2, 4, 8, 15];
    let mut backoff_index = 0;

    loop {
        if start.elapsed() > timeout {
            return Err(crate::DeployerError::HealthCheck(format!(
                "Health check timed out after {}s",
                timeout_secs
            )));
        }

        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                info!("Health check passed for port {}", port);
                return Ok(());
            }
            Ok(response) => {
                warn!(
                    "Health check returned status {} for port {}",
                    response.status(),
                    port
                );
            }
            Err(e) => {
                warn!("Health check failed for port {}: {}", port, e);
            }
        }

        let sleep_duration = backoff_intervals[backoff_index.min(backoff_intervals.len() - 1)];
        tokio::time::sleep(Duration::from_secs(sleep_duration)).await;
        backoff_index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check_timeout() {
        let result = check_health(9999, "/health", 1).await;
        assert!(result.is_err());
    }
}
