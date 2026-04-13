use std::collections::HashMap;
use zbus::Connection;
use zbus::proxy::Proxy;

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
        user: Option<&str>,
        env: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        let proxy = self.manager_proxy().await?;

        let mut properties: Vec<(&str, zbus::zvariant::Value)> = vec![
            ("Description", format!("Kennel: {unit_name}").into()),
            ("Slice", "kennel.slice".into()),
            ("Restart", "on-failure".into()),
            ("RestartSec", 5u32.into()),
        ];

        let env_strings: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        if !env_strings.is_empty() {
            properties.push(("Environment", env_strings.into()));
        }

        if let Some(u) = user {
            properties.push(("User", u.into()));
        }

        let exec_start_value: Vec<(String, Vec<String>, bool)> =
            vec![(exec_start.to_string(), vec![exec_start.to_string()], false)];
        properties.push(("ExecStart", exec_start_value.into()));

        let _: () = proxy
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

    pub async fn list_kennel_units(&self) -> anyhow::Result<Vec<String>> {
        let proxy = self.manager_proxy().await?;

        let units: Vec<(
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
        )> = proxy
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
