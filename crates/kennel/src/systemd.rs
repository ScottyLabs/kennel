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
    pub active_enter_usec: u64,
    pub n_restarts: u32,
}

pub struct SystemdClient {
    conn: Connection,
}

impl SystemdClient {
    pub async fn connect() -> anyhow::Result<Self> {
        let conn = Connection::system().await?;
        Ok(Self { conn })
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
    ) -> anyhow::Result<()> {
        let proxy = self.manager_proxy().await?;

        // StartTransientUnit fails with UnitExists if a prior fragment is still loaded.
        let service_unit = format!("{unit_name}.service");
        let _: Result<zbus::zvariant::OwnedObjectPath, _> = proxy
            .call("StopUnit", &(service_unit.clone(), "replace"))
            .await;
        let _: Result<(), _> = proxy.call("ResetFailedUnit", &(service_unit,)).await;

        let mut properties: Vec<(&str, zbus::zvariant::Value)> = vec![
            ("Description", format!("Kennel: {unit_name}").into()),
            ("Slice", "kennel.slice".into()),
            ("Restart", "on-failure".into()),
            ("RestartUSec", 5_000_000u64.into()),
            ("DynamicUser", true.into()),
            ("CPUAccounting", true.into()),
            ("MemoryAccounting", true.into()),
            ("IOAccounting", true.into()),
            ("TasksAccounting", true.into()),
        ];

        let env_strings: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        if !env_strings.is_empty() {
            properties.push(("Environment", env_strings.into()));
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

        state == "active"
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

        Ok(UnitHealth {
            active: active_state == "active",
            active_state,
            sub_state,
            active_enter_usec,
            n_restarts,
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
