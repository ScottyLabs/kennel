use std::time::Duration;

pub const DEFAULT_ROUTER_ADDR: &str = "0.0.0.0:80";
pub const DEFAULT_API_HOST: &str = "0.0.0.0";
pub const DEFAULT_API_PORT: &str = "3000";
pub const DEFAULT_BASE_DOMAIN: &str = "scottylabs.org";

pub const DEFAULT_MAX_CONCURRENT_BUILDS: usize = 2;
pub const DEFAULT_WORK_DIR: &str = "/var/lib/kennel/builds";

pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(300);

pub const SITES_BASE_DIR: &str = "/var/lib/kennel/sites";
pub const SECRETS_DIR: &str = "/run/kennel/secrets";
pub const LOGS_DIR: &str = "/var/lib/kennel/logs";
pub const SERVICES_BASE_DIR: &str = "/var/lib/kennel/services";
pub const ACME_CACHE_DIR: &str = "/var/lib/kennel/acme";
pub const PROJECTS_CONFIG_PATH: &str = "/etc/kennel/projects.json";

pub const ROUTER_RELOAD_INTERVAL: Duration = Duration::from_secs(60);

pub const CLEANUP_JOB_INTERVAL: Duration = Duration::from_secs(600);
pub const LOG_CLEANUP_INTERVAL: Duration = Duration::from_secs(86400);
pub const LOG_RETENTION_DAYS: i64 = 30;
pub const DEPLOYMENT_EXPIRY_DAYS: i64 = 7;

pub const SUPERVISOR_EVENT_CAPACITY: usize = 100;

pub const BUILD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub const TEARDOWN_GRACE_PERIOD: Duration = Duration::from_secs(30);
pub const BLUE_GREEN_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
