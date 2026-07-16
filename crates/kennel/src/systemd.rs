use futures_util::stream::StreamExt;
use std::collections::HashMap;
use zbus::Connection;
use zbus::proxy::Proxy;

// org.freedesktop.systemd1 ListUnits row:
// (name, description, load, active, sub, followed, unit_path, job_id, job_type, job_path)
type UnitListEntry = (
    String,
    String,
    String,
    String,
    String,
    String,
    zbus::zvariant::OwnedObjectPath,
    u32,
    String,
    zbus::zvariant::OwnedObjectPath,
);

#[derive(Debug, Clone, serde::Serialize)]
pub struct UnitHealth {
    pub active: bool,
    pub active_state: String,
    pub sub_state: String,
    pub result: String,
    pub active_enter_usec: u64,
    pub n_restarts: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_healthy: Option<bool>,
}

pub struct SystemdClient {
    conn: Connection,
}

impl SystemdClient {
    pub async fn connect() -> anyhow::Result<Self> {
        let conn = Connection::system().await?;
        let client = Self { conn };
        // Subscribe to systemd job and unit signals
        let _: Result<(), zbus::Error> = client.manager_proxy().await?.call("Subscribe", &()).await;
        Ok(client)
    }

    async fn manager_proxy(&self) -> anyhow::Result<Proxy<'_>> {
        Ok(Proxy::new(
            &self.conn,
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
        )
        .await?)
    }

    pub async fn start_transient_unit(
        &self,
        unit_name: &str,
        exec_start: &str,
        env: &HashMap<String, String>,
        user: &str,
        working_dir: Option<&str>,
    ) -> anyhow::Result<()> {
        let proxy = self.manager_proxy().await?;

        // Stop any prior unit and wait for the stop job to finish
        // StartTransientUnit with mode fail errors while one is still loaded
        let service_unit = format!("{unit_name}.service");
        let mut job_removed = proxy.receive_signal("JobRemoved").await?;
        let stop: zbus::Result<zbus::zvariant::OwnedObjectPath> = proxy
            .call("StopUnit", &(service_unit.as_str(), "replace"))
            .await;
        if let Ok(job) = stop {
            let _ = tokio::time::timeout(kennel_config::constants::UNIT_STOP_TIMEOUT, async {
                while let Some(signal) = job_removed.next().await {
                    if let Ok((_, removed, _, _)) =
                        signal
                            .body()
                            .deserialize::<(u32, zbus::zvariant::OwnedObjectPath, String, String)>()
                        && removed == job
                    {
                        break;
                    }
                }
            })
            .await;
        }
        let _: Result<(), zbus::Error> = proxy
            .call("ResetFailedUnit", &(service_unit.as_str(),))
            .await;

        let mut properties: Vec<(&str, zbus::zvariant::Value)> = vec![
            ("Description", format!("Kennel: {unit_name}").into()),
            ("Slice", "kennel.slice".into()),
            ("Restart", "on-failure".into()),
            ("RestartUSec", 5_000_000u64.into()),
            (
                "StartLimitIntervalUSec",
                (kennel_config::constants::UNIT_START_LIMIT_INTERVAL.as_micros() as u64).into(),
            ),
            (
                "StartLimitBurst",
                kennel_config::constants::UNIT_START_LIMIT_BURST.into(),
            ),
            ("DynamicUser", true.into()),
            ("User", user.into()),
            ("CPUAccounting", true.into()),
            ("MemoryAccounting", true.into()),
            ("IOAccounting", true.into()),
            ("TasksAccounting", true.into()),
            ("NoNewPrivileges", true.into()),
            ("ProtectSystem", "strict".into()),
            ("ProtectHome", "yes".into()),
            ("PrivateTmp", true.into()),
        ];

        let env_strings: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        if !env_strings.is_empty() {
            properties.push(("Environment", env_strings.into()));
        }

        if let Some(group) = valkey_supplementary_group(env) {
            properties.push(("SupplementaryGroups", vec![group].into()));
        }

        if let Some(dir) = working_dir {
            properties.push(("WorkingDirectory", dir.into()));
        }

        let exec_start_value: Vec<(String, Vec<String>, bool)> =
            vec![(exec_start.to_string(), vec![exec_start.to_string()], false)];
        properties.push(("ExecStart", exec_start_value.into()));

        let _: zbus::zvariant::OwnedObjectPath = proxy
            .call(
                "StartTransientUnit",
                &(
                    format!("{unit_name}.service"),
                    "fail",
                    properties,
                    Vec::<(String, Vec<(String, zbus::zvariant::Value)>)>::new(),
                ),
            )
            .await?;

        tracing::info!(unit = %unit_name, "started transient unit");
        Ok(())
    }

    pub async fn run_build_unit(
        &self,
        unit_name: &str,
        argv: &[String],
        working_dir: &str,
        env: &HashMap<String, String>,
        group: &str,
        timeout: std::time::Duration,
    ) -> anyhow::Result<bool> {
        let proxy = self.manager_proxy().await?;
        let service_unit = format!("{unit_name}.service");

        let mut job_removed = proxy.receive_signal("JobRemoved").await?;
        let _: zbus::Result<zbus::zvariant::OwnedObjectPath> = proxy
            .call("StopUnit", &(service_unit.as_str(), "replace"))
            .await;
        let _: Result<(), zbus::Error> = proxy
            .call("ResetFailedUnit", &(service_unit.as_str(),))
            .await;

        let env_strings: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let exec_start: Vec<(String, Vec<String>, bool)> =
            vec![(argv[0].clone(), argv.to_vec(), false)];

        let properties: Vec<(&str, zbus::zvariant::Value)> = vec![
            ("Description", format!("Kennel build: {unit_name}").into()),
            ("Slice", "kennel.slice".into()),
            ("Type", "oneshot".into()),
            ("DynamicUser", true.into()),
            ("SupplementaryGroups", vec![group.to_string()].into()),
            ("WorkingDirectory", working_dir.into()),
            ("Environment", env_strings.into()),
            ("NoNewPrivileges", true.into()),
            ("ProtectSystem", "strict".into()),
            ("ProtectHome", "yes".into()),
            ("PrivateTmp", true.into()),
            ("ReadWritePaths", vec![working_dir.to_string()].into()),
            ("ExecStart", exec_start.into()),
        ];

        let start_job: zbus::zvariant::OwnedObjectPath = proxy
            .call(
                "StartTransientUnit",
                &(
                    service_unit.as_str(),
                    "replace",
                    properties,
                    Vec::<(String, Vec<(String, zbus::zvariant::Value)>)>::new(),
                ),
            )
            .await?;

        tracing::info!(unit = %unit_name, "started build unit");

        let waited = tokio::time::timeout(timeout, async {
            while let Some(signal) = job_removed.next().await {
                if let Ok((_, removed, _, result)) =
                    signal
                        .body()
                        .deserialize::<(u32, zbus::zvariant::OwnedObjectPath, String, String)>()
                    && removed == start_job
                {
                    return result;
                }
            }
            "failed".to_string()
        })
        .await;

        match waited {
            Ok(result) => Ok(result == "done"),
            Err(_) => {
                let _: zbus::Result<zbus::zvariant::OwnedObjectPath> = proxy
                    .call("StopUnit", &(service_unit.as_str(), "replace"))
                    .await;
                anyhow::bail!("build unit {unit_name} timed out")
            }
        }
    }

    pub async fn stop_unit(&self, unit_name: &str) -> anyhow::Result<()> {
        let proxy = self.manager_proxy().await?;

        let _: zbus::zvariant::OwnedObjectPath = proxy
            .call("StopUnit", &(format!("{unit_name}.service"), "fail"))
            .await?;

        tracing::info!(unit = %unit_name, "stopped unit");
        Ok(())
    }

    pub async fn is_active(&self, unit_name: &str) -> bool {
        let Ok(proxy) = self.manager_proxy().await else {
            return false;
        };

        let result: Result<zbus::zvariant::OwnedObjectPath, _> = proxy
            .call("GetUnit", &(format!("{unit_name}.service"),))
            .await;

        let Ok(unit_path) = result else {
            return false;
        };

        let Ok(unit_proxy) = Proxy::new(
            &self.conn,
            "org.freedesktop.systemd1",
            unit_path,
            "org.freedesktop.systemd1.Unit",
        )
        .await
        else {
            return false;
        };

        let state: String = unit_proxy
            .get_property("ActiveState")
            .await
            .unwrap_or_default();

        matches!(state.as_str(), "active" | "activating" | "reloading")
    }

    pub async fn get_health(&self, unit_name: &str) -> anyhow::Result<UnitHealth> {
        let proxy = self.manager_proxy().await?;
        let unit_path: zbus::zvariant::OwnedObjectPath = proxy
            .call("GetUnit", &(format!("{unit_name}.service"),))
            .await?;

        let unit_proxy = Proxy::new(
            &self.conn,
            "org.freedesktop.systemd1",
            unit_path,
            "org.freedesktop.systemd1.Unit",
        )
        .await?;

        let active_state: String = unit_proxy
            .get_property("ActiveState")
            .await
            .unwrap_or_default();
        let sub_state: String = unit_proxy
            .get_property("SubState")
            .await
            .unwrap_or_default();
        let active_enter_usec: u64 = unit_proxy
            .get_property("ActiveEnterTimestamp")
            .await
            .unwrap_or(0);
        let n_restarts: u32 = unit_proxy.get_property("NRestarts").await.unwrap_or(0);
        let result: String = unit_proxy.get_property("Result").await.unwrap_or_default();

        Ok(UnitHealth {
            active: active_state == "active",
            active_state,
            sub_state,
            result,
            active_enter_usec,
            n_restarts,
            app_healthy: None,
        })
    }

    pub async fn list_kennel_units(&self) -> anyhow::Result<Vec<String>> {
        let proxy = self.manager_proxy().await?;

        let units: Vec<UnitListEntry> = proxy
            .call(
                "ListUnitsByPatterns",
                &(Vec::<String>::new(), vec!["kennel-*.service"]),
            )
            .await?;

        Ok(units
            .into_iter()
            .map(|(name, ..)| name.trim_end_matches(".service").to_string())
            .collect())
    }
}

fn valkey_supplementary_group(env: &HashMap<String, String>) -> Option<String> {
    env.get("VALKEY_URL")
        .and_then(|url| unix_socket_group_from_valkey_url(url))
}

fn unix_socket_group_from_valkey_url(url: &str) -> Option<String> {
    let path = url.strip_prefix("redis+unix://")?;
    let path = path.split('?').next()?;
    std::path::Path::new(path)
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_group_from_valkey_url() {
        let env = HashMap::from([(
            "VALKEY_URL".into(),
            "redis+unix:///run/redis-kennel/redis.sock?db=8".into(),
        )]);
        assert_eq!(
            valkey_supplementary_group(&env).as_deref(),
            Some("redis-kennel")
        );
    }

    #[test]
    fn ignores_deployments_without_valkey() {
        let env = HashMap::from([("PORT".into(), "3000".into())]);
        assert_eq!(valkey_supplementary_group(&env), None);
    }
}
