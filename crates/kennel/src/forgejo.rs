use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const COMMENT_MARKER: &str = "<!-- kennel-deployment -->";

pub struct ForgejoClient {
    client: Client,
    api_base: String,
    token: String,
}

#[derive(Deserialize)]
struct Comment {
    id: i64,
    body: String,
}

#[derive(Serialize)]
struct CommentBody<'a> {
    body: &'a str,
}

#[derive(Serialize)]
pub struct CommitStatus<'a> {
    pub state: &'a str,
    pub description: &'a str,
    pub context: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<&'a str>,
}

impl ForgejoClient {
    pub fn new(api_base: String, token: String) -> Self {
        Self {
            client: Client::new(),
            api_base: api_base.trim_end_matches('/').to_string(),
            token,
        }
    }

    pub async fn upsert_pr_comment(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
        body: &str,
    ) -> Result<()> {
        let full_body = format!("{COMMENT_MARKER}\n{body}");
        let list_url = format!(
            "{}/repos/{owner}/{repo}/issues/{pr_number}/comments",
            self.api_base
        );

        let resp = self
            .client
            .get(&list_url)
            .header("Authorization", format!("token {}", self.token))
            .send()
            .await?;
        anyhow::ensure!(
            resp.status().is_success(),
            "list comments failed: {}",
            resp.text().await?
        );
        let comments: Vec<Comment> = resp.json().await?;

        if let Some(existing) = comments.iter().find(|c| c.body.contains(COMMENT_MARKER)) {
            let edit_url = format!(
                "{}/repos/{owner}/{repo}/issues/comments/{}",
                self.api_base, existing.id
            );
            let resp = self
                .client
                .patch(&edit_url)
                .header("Authorization", format!("token {}", self.token))
                .json(&CommentBody { body: &full_body })
                .send()
                .await?;
            anyhow::ensure!(
                resp.status().is_success(),
                "patch comment failed: {}",
                resp.text().await?
            );
        } else {
            let resp = self
                .client
                .post(&list_url)
                .header("Authorization", format!("token {}", self.token))
                .json(&CommentBody { body: &full_body })
                .send()
                .await?;
            anyhow::ensure!(
                resp.status().is_success(),
                "create comment failed: {}",
                resp.text().await?
            );
        }
        Ok(())
    }

    pub async fn create_commit_status(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
        status: CommitStatus<'_>,
    ) -> Result<()> {
        let url = format!("{}/repos/{owner}/{repo}/statuses/{sha}", self.api_base);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("token {}", self.token))
            .json(&status)
            .send()
            .await?;
        if !resp.status().is_success() {
            let code = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("commit status API returned {code}: {text}");
        }
        Ok(())
    }
}

pub fn pr_number_from_branch(branch: &str) -> Option<u64> {
    branch.strip_prefix("pr-")?.parse().ok()
}
