use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const API_BASE: &str = "https://api.cloudflare.com/client/v4";
const RECORD_COMMENT: &str = "Custom domain - managed by Kennel";

pub struct CloudflareClient {
    client: Client,
    token: String,
    zones: HashMap<String, String>,
    public_ip: String,
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
    pub fn new(token: String, zones: HashMap<String, String>, public_ip: String) -> Self {
        Self {
            client: Client::new(),
            token,
            zones,
            public_ip,
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

    /// Upsert an A record for the given fqdn pointing at the configured public IP.
    /// Returns Ok(true) if a record was created or updated, Ok(false) if no
    /// configured zone covers the fqdn (caller treats as a non-managed domain).
    pub async fn upsert_a_record(&self, fqdn: &str) -> Result<bool> {
        let Some((zone_name, zone_id)) = self.match_zone(fqdn) else {
            return Ok(false);
        };

        let existing = self.find_record(&zone_id, fqdn).await?;
        let body = UpsertBody {
            kind: "A",
            name: fqdn,
            content: &self.public_ip,
            ttl: 60,
            proxied: false,
            comment: RECORD_COMMENT,
        };

        let resp = match existing {
            Some(record) => {
                if record.kind == "A" && record.content == self.public_ip {
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

        tracing::info!(fqdn = %fqdn, zone = %zone_name, ip = %self.public_ip, http = %status, "dns record upserted");
        Ok(true)
    }

    /// Delete the A record for the given fqdn if one exists in a configured zone.
    pub async fn delete_a_record(&self, fqdn: &str) -> Result<()> {
        let Some((zone_name, zone_id)) = self.match_zone(fqdn) else {
            return Ok(());
        };

        let Some(record) = self.find_record(&zone_id, fqdn).await? else {
            return Ok(());
        };

        if record.kind != "A" {
            tracing::debug!(fqdn = %fqdn, zone = %zone_name, kind = %record.kind, "skipping non-A record");
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
        let url = format!("{API_BASE}/zones/{zone_id}/dns_records?type=A&name={name}");
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
        CloudflareClient::new("t".into(), map, "1.2.3.4".into())
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
