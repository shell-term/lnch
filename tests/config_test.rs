use std::path::Path;

mod helpers;

#[test]
fn test_parse_valid_config() {
    let path = Path::new("tests/fixtures/valid.yaml");
    let config = lnch::config::loader::load_config(path).unwrap();

    assert_eq!(config.name, "test-project");
    assert_eq!(config.tasks.len(), 2);
    assert_eq!(config.tasks[0].name, "task-a");
    assert_eq!(config.tasks[0].command, "echo \"hello from A\"");
    assert_eq!(config.tasks[0].color, Some("green".to_string()));
    assert_eq!(config.tasks[1].name, "task-b");
    assert_eq!(config.tasks[1].depends_on, Some(vec!["task-a".to_string()]));
}

#[test]
fn test_parse_invalid_yaml() {
    let path = Path::new("tests/fixtures/invalid.yaml");
    let result = lnch::config::loader::load_config(path);
    assert!(result.is_err());
}

#[test]
fn test_validate_valid_config() {
    let path = Path::new("tests/fixtures/valid.yaml");
    let config = lnch::config::loader::load_config(path).unwrap();
    let result = lnch::config::validator::validate_config(&config, Path::new("tests/fixtures"));
    assert!(result.is_ok());
}

#[test]
fn test_validate_circular_dep_config() {
    let path = Path::new("tests/fixtures/circular_dep.yaml");
    let config = lnch::config::loader::load_config(path).unwrap();
    let result = lnch::process::dependency::DependencyGraph::from_config(&config);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Circular dependency"),
        "Expected circular dependency error, got: {}",
        err
    );
}
