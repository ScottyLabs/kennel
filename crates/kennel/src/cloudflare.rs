use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const API_BASE: &str = "https://api.cloudflare.com/client/v4";
const MANAGED_SUFFIX: &str = "managed by Kennel";

fn record_comment(project_name: &str) -> String {
    format!("{project_name} - {MANAGED_SUFFIX}")
}

pub struct CloudflareClient {
    client: Client,
    token: String,
    zones: HashMap<String, String>,
    tunnel_target: String,
}

#[derive(Deserialize)]
struct ListResponse {
    result: Vec<Record>,
}

#[derive(Deserialize, Debug)]
struct Record {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    content: String,
    comment: Option<String>,
}

#[derive(Serialize)]
struct UpsertBody<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    name: &'a str,
    content: &'a str,
    ttl: u32,
    proxied: bool,
    comment: &'a str,
}

impl CloudflareClient {
    pub fn new(token: String, zones: HashMap<String, String>, tunnel_id: String) -> Self {
        Self {
            client: Client::new(),
            token,
            zones,
            tunnel_target: format!("{tunnel_id}.cfargotunnel.com"),
        }
    }

    /// Find the most specific zone that the given fqdn lives in.
    /// Returns the (zone_name, zone_id) pair, or None if no zone matches.
    fn match_zone(&self, fqdn: &str) -> Option<(String, String)> {
        let lower = fqdn.to_ascii_lowercase();
        self.zones
            .iter()
            .filter(|(zone, _)| {
                let z = zone.as_str();
                lower == z || lower.ends_with(&format!(".{z}"))
            })
            .max_by_key(|(zone, _)| zone.len())
            .map(|(z, id)| (z.clone(), id.clone()))
    }

    /// Upsert a proxied CNAME for the given fqdn pointing at the host's Cloudflare Tunnel.
    /// Returns Ok(true) if a record was created or updated, Ok(false) if no
    /// configured zone covers the fqdn (the domain cannot be served).
    pub async fn upsert_record(&self, project_name: &str, fqdn: &str) -> Result<bool> {
        let Some((zone_name, zone_id)) = self.match_zone(fqdn) else {
            return Ok(false);
        };

        let existing = self.find_record(&zone_id, fqdn).await?;
        let comment = record_comment(project_name);
        let body = UpsertBody {
            kind: "CNAME",
            name: fqdn,
            content: &self.tunnel_target,
            ttl: 1,
            proxied: true,
            comment: &comment,
        };

        let resp = match existing {
            Some(record) => {
                if record.kind == "CNAME" && record.content == self.tunnel_target {
                    tracing::debug!(fqdn = %fqdn, zone = %zone_name, "dns record already current");
                    return Ok(true);
                }
                let url = format!("{API_BASE}/zones/{zone_id}/dns_records/{}", record.id);
                self.client.put(&url)
            }
            None => {
                let url = format!("{API_BASE}/zones/{zone_id}/dns_records");
                self.client.post(&url)
            }
        };

        let response = resp.bearer_auth(&self.token).json(&body).send().await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("cloudflare {status} when upserting {fqdn}, response: {text}");
        }

        tracing::info!(fqdn = %fqdn, zone = %zone_name, target = %self.tunnel_target, http = %status, "dns record upserted");
        Ok(true)
    }

    /// Delete the kennel-managed record for the given fqdn if one exists in a configured zone.
    pub async fn delete_record(&self, fqdn: &str) -> Result<()> {
        let Some((zone_name, zone_id)) = self.match_zone(fqdn) else {
            return Ok(());
        };

        let Some(record) = self.find_record(&zone_id, fqdn).await? else {
            return Ok(());
        };

        let managed = record
            .comment
            .as_deref()
            .is_some_and(|c| c.ends_with(MANAGED_SUFFIX));
        if !managed {
            tracing::debug!(fqdn = %fqdn, zone = %zone_name, kind = %record.kind, "skipping record kennel does not manage");
            return Ok(());
        }

        let url = format!("{API_BASE}/zones/{zone_id}/dns_records/{}", record.id);
        let response = self
            .client
            .delete(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("cloudflare {status} when deleting {fqdn}, response: {text}");
        }

        tracing::info!(fqdn = %fqdn, zone = %zone_name, "dns record deleted");
        Ok(())
    }

    async fn find_record(&self, zone_id: &str, fqdn: &str) -> Result<Option<Record>> {
        let name = urlencoding::encode(fqdn);
        let url = format!("{API_BASE}/zones/{zone_id}/dns_records?name={name}");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?
            .error_for_status()?
            .json::<ListResponse>()
            .await?;
        Ok(resp.result.into_iter().next())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(zones: &[(&str, &str)]) -> CloudflareClient {
        let map = zones
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        CloudflareClient::new("t".into(), map, "tid".into())
    }

    #[test]
    fn picks_longest_matching_zone() {
        let c = mk(&[("scottylabs.org", "z1"), ("apps.scottylabs.org", "z2")]);
        let (zone, _) = c.match_zone("foo.apps.scottylabs.org").unwrap();
        assert_eq!(zone, "apps.scottylabs.org");
    }

    #[test]
    fn rejects_substring_collision() {
        let c = mk(&[("cmu.quest", "z1")]);
        assert!(c.match_zone("notcmu.quest").is_none());
        assert!(c.match_zone("cmu.quest").is_some());
    }

    #[test]
    fn case_insensitive() {
        let c = mk(&[("cmu.quest", "z1")]);
        assert!(c.match_zone("App.Cmu.Quest").is_some());
    }
}
