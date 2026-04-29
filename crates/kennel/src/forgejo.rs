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
}

pub fn parse_owner_repo(repo_url: &str) -> Option<(String, String)> {
    let trimmed = repo_url.trim_end_matches('/').trim_end_matches(".git");
    let parts: Vec<&str> = trimmed.rsplitn(3, |c: char| c == '/' || c == ':').collect();
    if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
        Some((parts[1].to_string(), parts[0].to_string()))
    } else {
        None
    }
}

pub fn pr_number_from_branch(branch: &str) -> Option<u64> {
    branch.strip_prefix("pr-")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_https_clone_url() {
        assert_eq!(
            parse_owner_repo("https://codeberg.org/ScottyLabs/kennel.git"),
            Some(("ScottyLabs".into(), "kennel".into()))
        );
    }

    #[test]
    fn parses_https_html_url() {
        assert_eq!(
            parse_owner_repo("https://codeberg.org/ScottyLabs/kennel"),
            Some(("ScottyLabs".into(), "kennel".into()))
        );
    }

    #[test]
    fn parses_ssh_url() {
        assert_eq!(
            parse_owner_repo("ssh://codeberg.org/ScottyLabs/kennel.git"),
            Some(("ScottyLabs".into(), "kennel".into()))
        );
    }

    #[test]
    fn parses_scp_style_ssh() {
        assert_eq!(
            parse_owner_repo("git@codeberg.org:ScottyLabs/kennel.git"),
            Some(("ScottyLabs".into(), "kennel".into()))
        );
    }

    #[test]
    fn extracts_pr_number() {
        assert_eq!(pr_number_from_branch("pr-12"), Some(12));
        assert_eq!(pr_number_from_branch("main"), None);
        assert_eq!(pr_number_from_branch("pr-abc"), None);
    }
}
