use serde::{Deserialize, Serialize};

/// `GET /projects/{project}/branches/{branch}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Branch {
    pub project: String,
    pub branch: String,
    /// None until the branch's first push
    pub request: Option<Request>,
    pub deployments: Vec<Deployment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub commit: String,
    pub status: RequestStatus,
    /// Set when the deploy failed, e.g. `healthcheck`
    pub failure_stage: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    /// Building, or not yet deployed
    Pending,
    Deployed,
    Failed,
    /// Previews are off for the project
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deployment {
    pub id: String,
    pub name: String,
    /// `service` or `site`
    pub kind: String,
    pub commit: String,
    pub url: String,
    pub custom_url: Option<String>,
    /// None for sites
    pub health: Option<Health>,
}

/// `GET /deployments/{id}/health`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Health {
    pub active: bool,
    pub active_state: String,
    pub sub_state: String,
    pub result: String,
    pub active_enter_usec: u64,
    pub n_restarts: u32,
    /// None while the unit is down
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_healthy: Option<bool>,
}
