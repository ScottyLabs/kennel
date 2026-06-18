use std::time::Duration;

pub const DEFAULT_API_HOST: &str = "0.0.0.0";
pub const DEFAULT_API_PORT: u16 = 3000;
pub const DEFAULT_EPHEMERAL_DOMAIN: &str = "scottylabs.net";
pub const CADDY_SERVER_NAME: &str = "srv0";

pub const DEFAULT_MAX_CONCURRENT_BUILDS: usize = 2;
pub const DEFAULT_WORK_DIR: &str = "/var/lib/kennel/builds";
pub const DEFAULT_DB_PATH: &str = "/var/lib/kennel/kennel.db";

pub const SITES_BASE_DIR: &str = "/var/lib/kennel/sites";
pub const GC_ROOTS_DIR: &str = "/nix/var/nix/gcroots/kennel";
pub const LOGS_DIR: &str = "/var/lib/kennel/logs";

pub const BUILD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub const TEARDOWN_GRACE_PERIOD: Duration = Duration::from_secs(30);
pub const RECONCILE_INTERVAL: Duration = Duration::from_secs(30);
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(300);

pub const PORT_RANGE_START: u16 = 10000;
pub const PORT_RANGE_SIZE: u16 = 50000;

pub const DEPLOYMENT_EXPIRY_DAYS: i64 = 7;
pub const LOG_RETENTION_DAYS: i64 = 30;
