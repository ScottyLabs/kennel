use crate::error::{Result, WebhookError};
use crate::events::*;
use axum::http::HeaderMap;

const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

pub fn parse_webhook_event(headers: &HeaderMap, body: &[u8]) -> Result<WebhookEvent> {
    // Determine platform from event header
    let event_type = if let Some(forgejo_event) = headers.get("X-Forgejo-Event") {
        forgejo_event
            .to_str()
            .map_err(|_| WebhookError::InvalidPayload("Invalid event header".to_string()))?
    } else if let Some(github_event) = headers.get("X-GitHub-Event") {
        github_event
            .to_str()
            .map_err(|_| WebhookError::InvalidPayload("Invalid event header".to_string()))?
    } else {
        return Err(WebhookError::MissingHeader(
            "X-Forgejo-Event or X-GitHub-Event",
        ));
    };

    match event_type {
        "push" => parse_push_event(headers, body),
        "pull_request" => parse_pull_request_event(headers, body),
        _ => Err(WebhookError::InvalidPayload(format!(
            "Unsupported event type: {}",
            event_type
        ))),
    }
}

fn parse_push_event(headers: &HeaderMap, body: &[u8]) -> Result<WebhookEvent> {
    let is_forgejo = headers.contains_key("X-Forgejo-Event");

    if is_forgejo {
        let event: ForgejoPushEvent = serde_json::from_slice(body)?;

        let git_ref = event
            .git_ref
            .strip_prefix("refs/heads/")
            .unwrap_or(&event.git_ref)
            .to_string();

        let deleted = event.after == ZERO_SHA;

        Ok(WebhookEvent::Push {
            git_ref,
            commit_sha: event.after,
            author: event.pusher.username,
            deleted,
        })
    } else {
        let event: GitHubPushEvent = serde_json::from_slice(body)?;

        let git_ref = event
            .git_ref
            .strip_prefix("refs/heads/")
            .unwrap_or(&event.git_ref)
            .to_string();

        let deleted = event.after == ZERO_SHA;

        Ok(WebhookEvent::Push {
            git_ref,
            commit_sha: event.after,
            author: event.pusher.name,
            deleted,
        })
    }
}

fn parse_pull_request_event(headers: &HeaderMap, body: &[u8]) -> Result<WebhookEvent> {
    let is_forgejo = headers.contains_key("X-Forgejo-Event");

    if is_forgejo {
        let event: ForgejoPullRequestEvent = serde_json::from_slice(body)?;

        Ok(WebhookEvent::PullRequest {
            action: event.action,
            pr_number: event.number,
            commit_sha: event.pull_request.head.sha,
            author: event.sender.login,
        })
    } else {
        let event: GitHubPullRequestEvent = serde_json::from_slice(body)?;

        Ok(WebhookEvent::PullRequest {
            action: event.action,
            pr_number: event.number,
            commit_sha: event.pull_request.head.sha,
            author: event.sender.login,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_forgejo_push_event() {
        let body = r#"{
            "ref": "refs/heads/main",
            "before": "abc123",
            "after": "def456",
            "pusher": {
                "username": "alice"
            }
        }"#;

        let mut headers = HeaderMap::new();
        headers.insert("X-Forgejo-Event", "push".parse().unwrap());

        let event = parse_webhook_event(&headers, body.as_bytes()).unwrap();

        match event {
            WebhookEvent::Push {
                git_ref,
                commit_sha,
                author,
                deleted,
            } => {
                assert_eq!(git_ref, "main");
                assert_eq!(commit_sha, "def456");
                assert_eq!(author, "alice");
                assert!(!deleted);
            }
            _ => panic!("Expected Push event"),
        }
    }

    #[test]
    fn test_parse_github_push_event() {
        let body = r#"{
            "ref": "refs/heads/feature-branch",
            "before": "abc123",
            "after": "def456",
            "pusher": {
                "name": "bob"
            }
        }"#;

        let mut headers = HeaderMap::new();
        headers.insert("X-GitHub-Event", "push".parse().unwrap());

        let event = parse_webhook_event(&headers, body.as_bytes()).unwrap();

        match event {
            WebhookEvent::Push {
                git_ref,
                commit_sha,
                author,
                deleted,
            } => {
                assert_eq!(git_ref, "feature-branch");
                assert_eq!(commit_sha, "def456");
                assert_eq!(author, "bob");
                assert!(!deleted);
            }
            _ => panic!("Expected Push event"),
        }
    }

    #[test]
    fn test_parse_branch_deletion() {
        let body = r#"{
            "ref": "refs/heads/old-branch",
            "before": "abc123",
            "after": "0000000000000000000000000000000000000000",
            "pusher": {
                "username": "alice"
            }
        }"#;

        let mut headers = HeaderMap::new();
        headers.insert("X-Forgejo-Event", "push".parse().unwrap());

        let event = parse_webhook_event(&headers, body.as_bytes()).unwrap();

        match event {
            WebhookEvent::Push { deleted, .. } => {
                assert!(deleted);
            }
            _ => panic!("Expected Push event"),
        }
    }

    #[test]
    fn test_parse_forgejo_pr_event() {
        let body = r#"{
            "action": "opened",
            "number": 42,
            "pull_request": {
                "head": {
                    "sha": "abc123",
                    "ref": "feature-branch"
                }
            }
        }"#;

        let mut headers = HeaderMap::new();
        headers.insert("X-Forgejo-Event", "pull_request".parse().unwrap());

        let event = parse_webhook_event(&headers, body.as_bytes()).unwrap();

        match event {
            WebhookEvent::PullRequest {
                action,
                pr_number,
                commit_sha,
                ..
            } => {
                assert_eq!(action, "opened");
                assert_eq!(pr_number, 42);
                assert_eq!(commit_sha, "abc123");
            }
            _ => panic!("Expected PullRequest event"),
        }
    }

    #[test]
    fn test_parse_github_pr_event() {
        let body = r#"{
            "action": "synchronize",
            "number": 99,
            "pull_request": {
                "head": {
                    "sha": "xyz789"
                }
            },
            "sender": {
                "login": "charlie"
            }
        }"#;

        let mut headers = HeaderMap::new();
        headers.insert("X-GitHub-Event", "pull_request".parse().unwrap());

        let event = parse_webhook_event(&headers, body.as_bytes()).unwrap();

        match event {
            WebhookEvent::PullRequest {
                action,
                pr_number,
                commit_sha,
                author,
            } => {
                assert_eq!(action, "synchronize");
                assert_eq!(pr_number, 99);
                assert_eq!(commit_sha, "xyz789");
                assert_eq!(author, "charlie");
            }
            _ => panic!("Expected PullRequest event"),
        }
    }
}
