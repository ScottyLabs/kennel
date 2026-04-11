use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::ResourceLimits;

const CGROUP_BASE: &str = "/sys/fs/cgroup/kennel";

pub struct CgroupWrapper {
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct CgroupStats {
    pub memory_current: u64,
    pub memory_peak: u64,
    pub cpu_usage_usec: u64,
    pub pids_current: u64,
}

impl CgroupWrapper {
    /// Create a cgroup subtree and write resource limits. The cgroup is
    /// created at /sys/fs/cgroup/kennel/<name>/.
    #[cfg(target_os = "linux")]
    pub fn create(name: &str, limits: &ResourceLimits) -> crate::Result<Self> {
        let path = PathBuf::from(CGROUP_BASE).join(name);
        std::fs::create_dir_all(&path)?;

        if let Some(max) = limits.memory_max {
            write_limit(&path, "memory.max", &max.to_string())?;
        }
        if let Some(high) = limits.memory_high {
            write_limit(&path, "memory.high", &high.to_string())?;
        }
        if let Some(cpu) = limits.cpu_max {
            let max_usec = (cpu * 100_000.0) as u64;
            write_limit(&path, "cpu.max", &format!("{max_usec} 100000"))?;
        }
        if let Some(weight) = limits.cpu_weight {
            write_limit(&path, "cpu.weight", &weight.to_string())?;
        }
        if let Some(tasks) = limits.tasks_max {
            write_limit(&path, "pids.max", &tasks.to_string())?;
        }

        Ok(Self { path })
    }

    /// No-op on non-Linux platforms.
    #[cfg(not(target_os = "linux"))]
    pub fn create(name: &str, _limits: &ResourceLimits) -> crate::Result<Self> {
        tracing::debug!(
            process = name,
            "cgroup isolation not available on this platform"
        );
        Ok(Self {
            path: PathBuf::from(CGROUP_BASE).join(name),
        })
    }

    /// Read live resource usage from cgroup control files.
    #[cfg(target_os = "linux")]
    pub fn stats(&self) -> crate::Result<CgroupStats> {
        let memory_current = read_u64(&self.path.join("memory.current"))?;
        let memory_peak = read_u64(&self.path.join("memory.peak"))?;
        let pids_current = read_u64(&self.path.join("pids.current"))?;
        let cpu_stat = std::fs::read_to_string(self.path.join("cpu.stat"))?;
        let cpu_usage_usec = parse_cpu_stat(&cpu_stat, "usage_usec").unwrap_or(0);

        Ok(CgroupStats {
            memory_current,
            memory_peak,
            cpu_usage_usec,
            pids_current,
        })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn stats(&self) -> crate::Result<CgroupStats> {
        Ok(CgroupStats {
            memory_current: 0,
            memory_peak: 0,
            cpu_usage_usec: 0,
            pids_current: 0,
        })
    }

    /// Remove the cgroup directory. The cgroup must be empty (no
    /// processes) before removal.
    #[cfg(target_os = "linux")]
    pub fn remove(&self) -> crate::Result<()> {
        if self.path.exists() {
            std::fs::remove_dir(&self.path)?;
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn remove(&self) -> crate::Result<()> {
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for CgroupWrapper {
    fn drop(&mut self) {
        if let Err(e) = self.remove() {
            tracing::warn!(
                path = %self.path.display(),
                error = %e,
                "failed to remove cgroup on drop"
            );
        }
    }
}

#[cfg(target_os = "linux")]
fn write_limit(cgroup_path: &Path, file: &str, value: &str) -> crate::Result<()> {
    let path = cgroup_path.join(file);
    std::fs::write(&path, value).map_err(|e| {
        crate::SupervisorError::Other(anyhow::anyhow!("failed to write {}: {e}", path.display()))
    })
}

#[cfg(target_os = "linux")]
fn read_u64(path: &Path) -> crate::Result<u64> {
    let content = std::fs::read_to_string(path)?;
    content.trim().parse().map_err(|e| {
        crate::SupervisorError::Other(anyhow::anyhow!("parse {}: {e}", path.display()))
    })
}

#[cfg(target_os = "linux")]
fn parse_cpu_stat(content: &str, key: &str) -> Option<u64> {
    for line in content.lines() {
        if let Some(value) = line.strip_prefix(key) {
            return value.trim().parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgroup_stats_serializable() {
        let stats = CgroupStats {
            memory_current: 1024,
            memory_peak: 2048,
            cpu_usage_usec: 500000,
            pids_current: 3,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"memory_current\":1024"));
    }

    #[test]
    fn create_on_non_linux() {
        let limits = ResourceLimits {
            memory_max: Some(512 * 1024 * 1024),
            memory_high: None,
            cpu_max: Some(1.0),
            cpu_weight: None,
            tasks_max: Some(128),
        };

        // On macOS this is a no-op that succeeds.
        let wrapper = CgroupWrapper::create("test-process", &limits).unwrap();
        assert!(wrapper.path().ends_with("test-process"));
    }

    #[test]
    fn stats_on_non_linux() {
        let limits = ResourceLimits {
            memory_max: None,
            memory_high: None,
            cpu_max: None,
            cpu_weight: None,
            tasks_max: None,
        };

        let wrapper = CgroupWrapper::create("test-stats", &limits).unwrap();
        let stats = wrapper.stats().unwrap();
        assert_eq!(stats.memory_current, 0);
        assert_eq!(stats.pids_current, 0);
    }
}
