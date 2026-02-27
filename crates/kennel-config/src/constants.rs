use std::time::Duration;

pub const DEFAULT_ROUTER_ADDR: &str = "0.0.0.0:80";
pub const DEFAULT_API_HOST: &str = "0.0.0.0";
pub const DEFAULT_API_PORT: &str = "3000";
pub const DEFAULT_BASE_DOMAIN: &str = "scottylabs.org";

pub const DEFAULT_MAX_CONCURRENT_BUILDS: usize = 2;
pub const DEFAULT_WORK_DIR: &str = "/var/lib/kennel/builds";

pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(300);

pub const PORT_RANGE_START: u16 = 18000;
pub const PORT_RANGE_END: u16 = 19999;

pub const SITES_BASE_DIR: &str = "/var/lib/kennel/sites";
pub const SECRETS_DIR: &str = "/run/kennel/secrets";
pub const LOGS_DIR: &str = "/var/lib/kennel/logs";
pub const SERVICES_BASE_DIR: &str = "/var/lib/kennel/services";
pub const SYSTEMD_UNIT_DIR: &str = "/etc/systemd/system";
pub const ACME_CACHE_DIR: &str = "/var/lib/kennel/acme";
pub const PROJECTS_CONFIG_PATH: &str = "/etc/kennel/projects.json";

pub const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(30);
pub const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_CONSECUTIVE_HEALTH_FAILURES: u32 = 3;

pub const ROUTER_RELOAD_INTERVAL: Duration = Duration::from_secs(60);

pub const CLEANUP_JOB_INTERVAL: Duration = Duration::from_secs(600);
pub const LOG_CLEANUP_INTERVAL: Duration = Duration::from_secs(86400);
pub const LOG_RETENTION_DAYS: i64 = 30;

pub const BUILD_CHANNEL_CAPACITY: usize = 1000;
pub const DEPLOY_CHANNEL_CAPACITY: usize = 100;
pub const TEARDOWN_CHANNEL_CAPACITY: usize = 100;
pub const ROUTER_UPDATE_CHANNEL_CAPACITY: usize = 100;

pub const BLUE_GREEN_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
