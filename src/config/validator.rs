use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::error::LnchError;

use super::model::LnchConfig;

const VALID_COLORS: &[&str] = &[
    "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

/// Validate the loaded config for consistency
pub fn validate_config(config: &LnchConfig, base_dir: &Path) -> Result<(), LnchError> {
    validate_tasks_not_empty(config)?;
    validate_unique_task_names(config)?;
    validate_colors(config)?;
    validate_working_dirs(config, base_dir)?;
    validate_dependency_refs(config)?;
    validate_no_circular_deps(config)?;
    Ok(())
}

fn validate_tasks_not_empty(config: &LnchConfig) -> Result<(), LnchError> {
    if config.tasks.is_empty() {
        return Err(LnchError::ConfigValidation(
            "No tasks defined in config".to_string(),
        ));
    }
    Ok(())
}

fn validate_unique_task_names(config: &LnchConfig) -> Result<(), LnchError> {
    let mut seen = HashSet::new();
    for task in &config.tasks {
        if !seen.insert(&task.name) {
            return Err(LnchError::ConfigValidation(format!(
                "Duplicate task name: '{}'",
                task.name
            )));
        }
    }
    Ok(())
}

fn validate_colors(config: &LnchConfig) -> Result<(), LnchError> {
    for task in &config.tasks {
        if let Some(ref color) = task.color {
            if !VALID_COLORS.contains(&color.as_str()) {
                return Err(LnchError::ConfigValidation(format!(
                    "Invalid color '{}' for task '{}'",
                    color, task.name
                )));
            }
        }
    }
    Ok(())
}

fn validate_working_dirs(config: &LnchConfig, base_dir: &Path) -> Result<(), LnchError> {
    for task in &config.tasks {
        if let Some(ref dir) = task.working_dir {
            let resolved = if dir.is_absolute() {
                dir.clone()
            } else {
                base_dir.join(dir)
            };
            if !resolved.is_dir() {
                return Err(LnchError::ConfigValidation(format!(
                    "Working directory does not exist: '{}'",
                    resolved.display()
                )));
            }
        }
    }
    Ok(())
}

fn validate_dependency_refs(config: &LnchConfig) -> Result<(), LnchError> {
    let task_names: HashSet<&str> = config.tasks.iter().map(|t| t.name.as_str()).collect();

    for task in &config.tasks {
        if let Some(ref deps) = task.depends_on {
            for dep in deps {
                if !task_names.contains(dep.as_str()) {
                    return Err(LnchError::ConfigValidation(format!(
                        "Task '{}' depends on unknown task '{}'",
                        task.name, dep
                    )));
                }
            }
        }
    }
    Ok(())
}

fn validate_no_circular_deps(config: &LnchConfig) -> Result<(), LnchError> {
    let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
    for task in &config.tasks {
        let deps = task
            .depends_on
            .as_ref()
            .map(|d| d.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        edges.insert(task.name.as_str(), deps);
    }

    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Unvisited,
        InStack,
        Done,
    }

    let mut states: HashMap<&str, State> = edges.keys().map(|&k| (k, State::Unvisited)).collect();
    let mut path: Vec<&str> = Vec::new();

    fn dfs<'a>(
        node: &'a str,
        edges: &HashMap<&'a str, Vec<&'a str>>,
        states: &mut HashMap<&'a str, State>,
        path: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        states.insert(node, State::InStack);
        path.push(node);

        if let Some(neighbors) = edges.get(node) {
            for &neighbor in neighbors {
                match states.get(neighbor) {
                    Some(State::InStack) => {
                        let cycle_start = path.iter().position(|&n| n == neighbor).unwrap();
                        let mut cycle: Vec<String> =
                            path[cycle_start..].iter().map(|s| s.to_string()).collect();
                        cycle.push(neighbor.to_string());
                        return Some(cycle);
                    }
                    Some(State::Unvisited) | None => {
                        if let Some(cycle) = dfs(neighbor, edges, states, path) {
                            return Some(cycle);
                        }
                    }
                    Some(State::Done) => {}
                }
            }
        }

        path.pop();
        states.insert(node, State::Done);
        None
    }

    let keys: Vec<&str> = edges.keys().copied().collect();
    for &node in &keys {
        if states[node] == State::Unvisited {
            if let Some(cycle) = dfs(node, &edges, &mut states, &mut path) {
                return Err(LnchError::CircularDependency(cycle.join(" -> ")));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{LnchConfig, TaskConfig};

    fn base_task(name: &str) -> TaskConfig {
        TaskConfig {
            name: name.to_string(),
            command: format!("echo {}", name),
            working_dir: None,
            env: None,
            color: None,
            depends_on: None,
        }
    }

    #[test]
    fn test_validate_empty_tasks() {
        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![],
        };
        let result = validate_config(&config, Path::new("."));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No tasks defined"));
    }

    #[test]
    fn test_validate_duplicate_task_names() {
        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![base_task("foo"), base_task("foo")],
        };
        let result = validate_config(&config, Path::new("."));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Duplicate task name"));
    }

    #[test]
    fn test_validate_unknown_dependency() {
        let mut task = base_task("a");
        task.depends_on = Some(vec!["nonexistent".to_string()]);
        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![task],
        };
        let result = validate_config(&config, Path::new("."));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("depends on unknown task"));
    }

    #[test]
    fn test_validate_circular_dependency() {
        let mut a = base_task("a");
        a.depends_on = Some(vec!["c".to_string()]);
        let mut b = base_task("b");
        b.depends_on = Some(vec!["a".to_string()]);
        let mut c = base_task("c");
        c.depends_on = Some(vec!["b".to_string()]);

        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![a, b, c],
        };
        let result = validate_config(&config, Path::new("."));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Circular dependency"),
            "Expected circular dependency error, got: {}",
            err
        );
    }

    #[test]
    fn test_validate_self_dependency() {
        let mut a = base_task("a");
        a.depends_on = Some(vec!["a".to_string()]);
        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![a],
        };
        let result = validate_config(&config, Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_color() {
        let mut task = base_task("a");
        task.color = Some("rainbow".to_string());
        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![task],
        };
        let result = validate_config(&config, Path::new("."));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid color"));
    }

    #[test]
    fn test_validate_valid_config() {
        let mut b = base_task("b");
        b.depends_on = Some(vec!["a".to_string()]);
        b.color = Some("green".to_string());
        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![base_task("a"), b],
        };
        let result = validate_config(&config, Path::new("."));
        assert!(result.is_ok());
    }
}
