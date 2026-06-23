use reqwest::Client;
use std::collections::HashSet;

pub struct CaddyClient {
    client: Client,
    admin_url: String,
}

impl CaddyClient {
    pub fn new(admin_url: String) -> Self {
        Self {
            client: Client::new(),
            admin_url,
        }
    }

    pub async fn add_static_route(
        &self,
        route_id: &str,
        domain: &str,
        store_path: &str,
        spa: bool,
    ) -> anyhow::Result<()> {
        let config = if spa {
            serde_json::json!({
                "@id": route_id,
                "match": [{"host": [domain]}],
                "handle": [{
                    "handler": "subroute",
                    "routes": [{
                        "handle": [{
                            "handler": "rewrite",
                            "uri": "{http.matchers.file.relative}"
                        }],
                        "match": [{
                            "file": {
                                "root": store_path,
                                "try_files": ["{http.request.uri.path}", "/index.html"]
                            }
                        }]
                    }, {
                        "handle": [{
                            "handler": "file_server",
                            "root": store_path
                        }]
                    }]
                }]
            })
        } else {
            serde_json::json!({
                "@id": route_id,
                "match": [{"host": [domain]}],
                "handle": [{
                    "handler": "file_server",
                    "root": store_path
                }]
            })
        };

        self.add_route(&config).await
    }

    pub async fn add_proxy_route(
        &self,
        route_id: &str,
        domain: &str,
        port: u16,
    ) -> anyhow::Result<()> {
        let config = serde_json::json!({
            "@id": route_id,
            "match": [{"host": [domain]}],
            "handle": [{
                "handler": "reverse_proxy",
                "upstreams": [{"dial": format!("localhost:{port}")}]
            }]
        });

        self.add_route(&config).await
    }

    pub async fn remove_route(&self, route_id: &str) -> anyhow::Result<()> {
        let url = format!("{}/id/{}", self.admin_url, route_id);
        let resp = self.client.delete(&url).send().await?;

        if resp.status().as_u16() == 404 {
            return Ok(());
        }

        anyhow::ensure!(
            resp.status().is_success(),
            "caddy route remove failed: {}",
            resp.text().await?
        );
        Ok(())
    }

    /// Returns the set of `@id` values for all routes on the kennel server
    pub async fn list_route_ids(&self) -> anyhow::Result<HashSet<String>> {
        let url = format!(
            "{}/config/apps/http/servers/{}/routes",
            self.admin_url,
            kennel_config::constants::CADDY_SERVER_NAME
        );
        let resp = self.client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Ok(HashSet::new());
        }
        let routes: Vec<serde_json::Value> = resp.json().await?;
        Ok(routes
            .iter()
            .filter_map(|r| r["@id"].as_str().map(String::from))
            .collect())
    }

    // Upsert a route by its @id
    async fn add_route(&self, config: &serde_json::Value) -> anyhow::Result<()> {
        if let Some(id) = config["@id"].as_str() {
            let url = format!("{}/id/{}", self.admin_url, id);
            let resp = self.client.patch(&url).json(config).send().await?;
            if resp.status().is_success() {
                return Ok(());
            }

            anyhow::ensure!(
                resp.status().as_u16() == 404,
                "caddy route patch failed: {}",
                resp.text().await?
            );
            // Route doesn't exist yet, fall through to POST
        }

        let url = format!(
            "{}/config/apps/http/servers/{}/routes",
            self.admin_url,
            kennel_config::constants::CADDY_SERVER_NAME
        );
        let resp = self.client.post(&url).json(config).send().await?;

        anyhow::ensure!(
            resp.status().is_success(),
            "caddy route add failed: {}",
            resp.text().await?
        );
        Ok(())
    }
}
