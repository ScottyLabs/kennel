use serde::Deserialize;

#[derive(Debug, Clone)]
pub enum WebhookEvent {
    Push {
        git_ref: String,
        commit_sha: String,
        author: String,
        deleted: bool,
    },
    PullRequest {
        action: String,
        pr_number: u64,
        commit_sha: String,
        author: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct ForgejoPushEvent {
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub after: String,
    pub pusher: ForgejoPusher,
}

#[derive(Debug, Deserialize)]
pub struct ForgejoPusher {
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct GitHubPushEvent {
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub after: String,
    pub pusher: GitHubPusher,
}

#[derive(Debug, Deserialize)]
pub struct GitHubPusher {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct ForgejoPullRequestEvent {
    pub action: String,
    pub number: u64,
    pub pull_request: ForgejoPullRequest,
    pub sender: ForgejoSender,
}

#[derive(Debug, Deserialize)]
pub struct ForgejoPullRequest {
    pub head: ForgejoHead,
}

#[derive(Debug, Deserialize)]
pub struct ForgejoSender {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct ForgejoHead {
    pub sha: String,
}

#[derive(Debug, Deserialize)]
pub struct GitHubPullRequestEvent {
    pub action: String,
    pub number: u64,
    pub pull_request: GitHubPullRequest,
    pub sender: GitHubSender,
}

#[derive(Debug, Deserialize)]
pub struct GitHubPullRequest {
    pub head: GitHubHead,
}

#[derive(Debug, Deserialize)]
pub struct GitHubHead {
    pub sha: String,
}

#[derive(Debug, Deserialize)]
pub struct GitHubSender {
    pub login: String,
}
