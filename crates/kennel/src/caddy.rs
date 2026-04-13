use reqwest::Client;

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

    async fn add_route(&self, config: &serde_json::Value) -> anyhow::Result<()> {
        let url = format!(
            "{}/config/apps/http/servers/kennel/routes",
            self.admin_url
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
