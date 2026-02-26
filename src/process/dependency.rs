use std::collections::{HashMap, VecDeque};

use crate::config::model::LnchConfig;
use crate::error::LnchError;

#[derive(Debug)]
pub struct DependencyGraph {
    /// task_name -> list of tasks it depends on
    edges: HashMap<String, Vec<String>>,
    /// all known task names
    all_tasks: Vec<String>,
}

impl DependencyGraph {
    pub fn from_config(config: &LnchConfig) -> Result<Self, LnchError> {
        let mut edges = HashMap::new();
        let mut all_tasks = Vec::new();

        for task in &config.tasks {
            all_tasks.push(task.name.clone());
            let deps = task.depends_on.clone().unwrap_or_default();
            edges.insert(task.name.clone(), deps);
        }

        let graph = Self { edges, all_tasks };
        if let Some(cycle) = graph.detect_cycle() {
            return Err(LnchError::CircularDependency(cycle.join(" -> ")));
        }
        Ok(graph)
    }

    /// Kahn's algorithm: returns groups of tasks that can be started in parallel.
    /// Each group's dependencies are satisfied by all previous groups.
    pub fn topological_sort(&self) -> Vec<Vec<String>> {
        // Build in-degree map and reverse adjacency list
        // edges: task -> [deps it depends ON]
        // For Kahn's, we need: for each task, count how many depend on it (in-degree)
        // Actually, edges[task] = [things task depends on]
        // So the "graph" direction is: dep -> task (dep must come before task)
        // in_degree[task] = number of dependencies task has
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

        for name in &self.all_tasks {
            in_degree.insert(name.as_str(), 0);
            dependents.entry(name.as_str()).or_default();
        }

        for (task, deps) in &self.edges {
            in_degree.insert(task.as_str(), deps.len());
            for dep in deps {
                dependents.entry(dep.as_str()).or_default().push(task.as_str());
            }
        }

        let mut result = Vec::new();
        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&name, _)| name)
            .collect();

        while !queue.is_empty() {
            let group: Vec<String> = queue.drain(..).map(|s| s.to_string()).collect();
            let mut next_queue = Vec::new();

            for task in &group {
                if let Some(deps) = dependents.get(task.as_str()) {
                    for &dependent in deps {
                        if let Some(deg) = in_degree.get_mut(dependent) {
                            *deg -= 1;
                            if *deg == 0 {
                                next_queue.push(dependent);
                            }
                        }
                    }
                }
            }

            result.push(group);
            queue.extend(next_queue);
        }

        result
    }

    /// DFS-based cycle detection
    fn detect_cycle(&self) -> Option<Vec<String>> {
        #[derive(Clone, Copy, PartialEq)]
        enum State {
            Unvisited,
            InStack,
            Done,
        }

        let mut states: HashMap<&str, State> = self
            .all_tasks
            .iter()
            .map(|t| (t.as_str(), State::Unvisited))
            .collect();
        let mut path: Vec<&str> = Vec::new();

        fn dfs<'a>(
            node: &'a str,
            edges: &'a HashMap<String, Vec<String>>,
            states: &mut HashMap<&'a str, State>,
            path: &mut Vec<&'a str>,
        ) -> Option<Vec<String>> {
            states.insert(node, State::InStack);
            path.push(node);

            if let Some(deps) = edges.get(node) {
                for dep in deps {
                    match states.get(dep.as_str()) {
                        Some(State::InStack) => {
                            let start = path.iter().position(|&n| n == dep.as_str()).unwrap();
                            let mut cycle: Vec<String> =
                                path[start..].iter().map(|s| s.to_string()).collect();
                            cycle.push(dep.clone());
                            return Some(cycle);
                        }
                        Some(State::Unvisited) | None => {
                            if let Some(cycle) = dfs(dep.as_str(), edges, states, path) {
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

        for task in &self.all_tasks {
            if states[task.as_str()] == State::Unvisited {
                if let Some(cycle) = dfs(task.as_str(), &self.edges, &mut states, &mut path) {
                    return Some(cycle);
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::TaskConfig;

    fn task(name: &str, deps: Option<Vec<&str>>) -> TaskConfig {
        TaskConfig {
            name: name.to_string(),
            command: format!("echo {}", name),
            working_dir: None,
            env: None,
            color: None,
            depends_on: deps.map(|d| d.into_iter().map(String::from).collect()),
        }
    }

    #[test]
    fn test_topological_sort_no_deps() {
        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![task("a", None), task("b", None), task("c", None)],
        };
        let graph = DependencyGraph::from_config(&config).unwrap();
        let groups = graph.topological_sort();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 3);
    }

    #[test]
    fn test_topological_sort_linear() {
        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![
                task("a", None),
                task("b", Some(vec!["a"])),
                task("c", Some(vec!["b"])),
            ],
        };
        let graph = DependencyGraph::from_config(&config).unwrap();
        let groups = graph.topological_sort();
        assert_eq!(groups.len(), 3);
        assert!(groups[0].contains(&"a".to_string()));
        assert!(groups[1].contains(&"b".to_string()));
        assert!(groups[2].contains(&"c".to_string()));
    }

    #[test]
    fn test_topological_sort_diamond() {
        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![
                task("a", None),
                task("b", Some(vec!["a"])),
                task("c", Some(vec!["a"])),
                task("d", Some(vec!["b", "c"])),
            ],
        };
        let graph = DependencyGraph::from_config(&config).unwrap();
        let groups = graph.topological_sort();
        assert_eq!(groups.len(), 3);
        assert!(groups[0].contains(&"a".to_string()));
        assert!(groups[1].contains(&"b".to_string()));
        assert!(groups[1].contains(&"c".to_string()));
        assert!(groups[2].contains(&"d".to_string()));
    }

    #[test]
    fn test_detect_cycle() {
        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![
                task("a", Some(vec!["c"])),
                task("b", Some(vec!["a"])),
                task("c", Some(vec!["b"])),
            ],
        };
        let result = DependencyGraph::from_config(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Circular dependency"));
    }

    #[test]
    fn test_detect_self_dependency() {
        let config = LnchConfig {
            name: "test".to_string(),
            tasks: vec![task("a", Some(vec!["a"]))],
        };
        let result = DependencyGraph::from_config(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Circular dependency"));
    }
}
