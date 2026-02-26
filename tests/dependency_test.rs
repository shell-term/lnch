use std::path::Path;

#[test]
fn test_dependency_graph_from_valid_config() {
    let path = Path::new("tests/fixtures/valid.yaml");
    let config = lnch::config::loader::load_config(path).unwrap();
    let graph = lnch::process::dependency::DependencyGraph::from_config(&config);
    assert!(graph.is_ok());

    let graph = graph.unwrap();
    let groups = graph.topological_sort();
    assert_eq!(groups.len(), 2);
    assert!(groups[0].contains(&"task-a".to_string()));
    assert!(groups[1].contains(&"task-b".to_string()));
}

#[test]
fn test_dependency_graph_from_circular_config() {
    let path = Path::new("tests/fixtures/circular_dep.yaml");
    let config = lnch::config::loader::load_config(path).unwrap();
    let result = lnch::process::dependency::DependencyGraph::from_config(&config);
    assert!(result.is_err());
}
