use std::collections::HashSet;
use std::path::Path;

use crate::error::LnchError;

use super::model::LnchConfig;

const VALID_COLORS: &[&str] = &["red", "green", "yellow", "blue", "magenta", "cyan", "white"];

/// Validate the loaded config for consistency
pub fn validate_config(config: &LnchConfig, base_dir: &Path) -> Result<(), LnchError> {
    validate_tasks_not_empty(config)?;
    validate_unique_task_names(config)?;
    validate_colors(config)?;
    validate_working_dirs(config, base_dir)?;
    validate_dependency_refs(config)?;
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
        assert!(result.unwrap_err().to_string().contains("No tasks defined"));
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
