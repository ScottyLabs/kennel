use std::collections::{HashMap, HashSet};

use crate::config::ProcessConfig;
use crate::error::SupervisorError;

/// Sort process configs into batches by dependency order. Processes in the
/// same batch have no dependencies on each other and can start concurrently.
/// Each batch must complete (all readiness probes pass) before the next
/// batch starts.
pub fn topological_sort(configs: &[ProcessConfig]) -> crate::Result<Vec<Vec<&ProcessConfig>>> {
    let name_to_config: HashMap<&str, &ProcessConfig> =
        configs.iter().map(|c| (c.name.as_str(), c)).collect();

    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for config in configs {
        in_degree.entry(config.name.as_str()).or_insert(0);
        for dep in &config.after {
            if name_to_config.contains_key(dep.as_str()) {
                *in_degree.entry(config.name.as_str()).or_insert(0) += 1;
                dependents
                    .entry(dep.as_str())
                    .or_default()
                    .push(&config.name);
            }
        }
    }

    let mut batches = Vec::new();
    let mut queue: HashSet<&str> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(&name, _)| name)
        .collect();

    while !queue.is_empty() {
        let batch: Vec<&ProcessConfig> = queue.iter().map(|name| name_to_config[name]).collect();
        batches.push(batch);

        let mut next_queue = HashSet::new();
        for name in &queue {
            if let Some(deps) = dependents.get(name) {
                for dep in deps {
                    let degree = in_degree.get_mut(dep).unwrap();
                    *degree -= 1;
                    if *degree == 0 {
                        next_queue.insert(*dep);
                    }
                }
            }
        }
        queue = next_queue;
    }

    let total_sorted: usize = batches.iter().map(|b| b.len()).sum();
    if total_sorted != configs.len() {
        return Err(SupervisorError::DependencyCycle);
    }

    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(name: &str, after: &[&str]) -> ProcessConfig {
        ProcessConfig {
            name: name.into(),
            exec: "true".into(),
            args: vec![],
            cwd: None,
            env: HashMap::new(),
            after: after.iter().map(|s| s.to_string()).collect(),
            listen: vec![],
            ports: HashMap::new(),
            ready: None,
            restart: Default::default(),
            watch: Default::default(),
            watchdog: None,
            resources: None,
            user: None,
            capabilities: vec![],
        }
    }

    #[test]
    fn no_dependencies() {
        let configs = vec![cfg("a", &[]), cfg("b", &[]), cfg("c", &[])];
        let batches = topological_sort(&configs).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 3);
    }

    #[test]
    fn linear_chain() {
        let configs = vec![cfg("a", &[]), cfg("b", &["a"]), cfg("c", &["b"])];
        let batches = topological_sort(&configs).unwrap();
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0][0].name, "a");
        assert_eq!(batches[1][0].name, "b");
        assert_eq!(batches[2][0].name, "c");
    }

    #[test]
    fn diamond_dependency() {
        let configs = vec![
            cfg("a", &[]),
            cfg("b", &["a"]),
            cfg("c", &["a"]),
            cfg("d", &["b", "c"]),
        ];
        let batches = topological_sort(&configs).unwrap();
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0][0].name, "a");
        assert_eq!(batches[1].len(), 2);
        let second_names: HashSet<&str> = batches[1].iter().map(|c| c.name.as_str()).collect();
        assert!(second_names.contains("b"));
        assert!(second_names.contains("c"));
        assert_eq!(batches[2][0].name, "d");
    }

    #[test]
    fn cycle_detected() {
        let configs = vec![cfg("a", &["b"]), cfg("b", &["a"])];
        let result = topological_sort(&configs);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SupervisorError::DependencyCycle
        ));
    }

    #[test]
    fn external_dependency_ignored() {
        let configs = vec![cfg("a", &["external-service"]), cfg("b", &["a"])];
        let batches = topological_sort(&configs).unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0][0].name, "a");
        assert_eq!(batches[1][0].name, "b");
    }

    #[test]
    fn empty_configs() {
        let configs: Vec<ProcessConfig> = vec![];
        let batches = topological_sort(&configs).unwrap();
        assert!(batches.is_empty());
    }

    #[test]
    fn single_process() {
        let configs = vec![cfg("solo", &[])];
        let batches = topological_sort(&configs).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0][0].name, "solo");
    }
}
