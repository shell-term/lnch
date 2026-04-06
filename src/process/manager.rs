use std::collections::{HashMap, HashSet};

use tokio::sync::mpsc;

use crate::config::model::LnchConfig;
use crate::message::{ProcessCommand, ProcessEvent};
use crate::process::dependency::DependencyGraph;
use crate::process::task_runner::TaskRunner;

pub struct ProcessManager {
    runners: HashMap<String, TaskRunner>,
    dependency_graph: DependencyGraph,
    cmd_rx: mpsc::Receiver<ProcessCommand>,
    #[allow(dead_code)]
    event_tx: mpsc::Sender<ProcessEvent>,
}

impl ProcessManager {
    pub fn new(
        config: &LnchConfig,
        dependency_graph: DependencyGraph,
        cmd_rx: mpsc::Receiver<ProcessCommand>,
        event_tx: mpsc::Sender<ProcessEvent>,
    ) -> Self {
        let mut runners = HashMap::new();
        for task_config in &config.tasks {
            let runner = TaskRunner::new(task_config.clone(), event_tx.clone());
            runners.insert(task_config.name.clone(), runner);
        }

        Self {
            runners,
            dependency_graph,
            cmd_rx,
            event_tx,
        }
    }

    pub async fn run(&mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            tracing::info!(command = ?cmd, "ProcessManager received command");
            match cmd {
                ProcessCommand::Start(name) => {
                    self.start_task(&name).await;
                }
                ProcessCommand::Stop(name) => {
                    self.stop_task(&name).await;
                }
                ProcessCommand::Restart(name) => {
                    self.restart_task(&name).await;
                }
                ProcessCommand::StartAll => {
                    self.start_all().await;
                }
                ProcessCommand::StopAll => {
                    self.stop_all().await;
                }
                ProcessCommand::Shutdown => {
                    tracing::info!("Shutdown requested, stopping all tasks");
                    self.stop_all().await;
                    break;
                }
                ProcessCommand::Reload(new_config) => {
                    tracing::info!("Config reload requested");
                    self.handle_reload(new_config).await;
                }
            }
        }
    }

    async fn start_task(&mut self, name: &str) {
        if let Some(runner) = self.runners.get_mut(name) {
            if runner.is_running() {
                return;
            }
            if let Err(e) = runner.start().await {
                tracing::error!(task = %name, error = %e, "Failed to start task");
            }
        }
    }

    async fn stop_task(&mut self, name: &str) {
        if let Some(runner) = self.runners.get_mut(name) {
            if let Err(e) = runner.stop().await {
                tracing::error!(task = %name, error = %e, "Failed to stop task");
            }
        }
    }

    async fn restart_task(&mut self, name: &str) {
        self.stop_task(name).await;
        self.start_task(name).await;
    }

    async fn start_all(&mut self) {
        let groups = self.dependency_graph.topological_sort();
        let total_groups = groups.len();

        for (group_idx, group) in groups.into_iter().enumerate() {
            for name in &group {
                self.start_task(name).await;
            }

            // Skip readiness wait for the last group (nothing depends on it)
            if group_idx >= total_groups - 1 {
                break;
            }

            // Wait for all tasks in this group to become ready
            self.wait_for_group_ready(&group).await;
        }
    }

    async fn wait_for_group_ready(&self, group: &[String]) {
        let futures: Vec<_> = group
            .iter()
            .filter_map(|name| {
                self.runners.get(name).map(|runner| {
                    let name = name.clone();
                    let event_tx = self.event_tx.clone();
                    async move {
                        let result = runner.wait_ready().await;
                        match &result {
                            crate::process::ready::ReadyResult::Ready => {
                                tracing::info!(task = %name, "Task is ready");
                            }
                            crate::process::ready::ReadyResult::TimedOut => {
                                tracing::warn!(task = %name, "Readiness check timed out, continuing");
                                let _ = event_tx
                                    .send(ProcessEvent::LogLine {
                                        task_name: name.clone(),
                                        line: "[lnch] Readiness check timed out, continuing..."
                                            .to_string(),
                                        is_stderr: true,
                                    })
                                    .await;
                            }
                            crate::process::ready::ReadyResult::Failed => {
                                tracing::error!(task = %name, "Task failed during readiness check");
                            }
                        }
                        result
                    }
                })
            })
            .collect();

        futures::future::join_all(futures).await;
    }

    async fn handle_reload(&mut self, new_config: LnchConfig) {
        let new_names: HashSet<String> =
            new_config.tasks.iter().map(|t| t.name.clone()).collect();
        let old_names: HashSet<String> = self.runners.keys().cloned().collect();

        let new_config_map: HashMap<&str, &crate::config::model::TaskConfig> = new_config
            .tasks
            .iter()
            .map(|t| (t.name.as_str(), t))
            .collect();

        // 1. Stop and remove runners for deleted tasks
        for name in old_names.difference(&new_names) {
            self.stop_task(name).await;
            self.runners.remove(name.as_str());
            tracing::info!(task = %name, "Removed task runner");
        }

        // 2. Stop and replace runners for changed tasks
        let common: Vec<String> = old_names.intersection(&new_names).cloned().collect();
        for name in &common {
            let new_task_config = new_config_map[name.as_str()];
            if self.runners[name].config_ref() != new_task_config {
                self.stop_task(name).await;
                self.runners.remove(name.as_str());
                let runner = TaskRunner::new(new_task_config.clone(), self.event_tx.clone());
                self.runners.insert(name.clone(), runner);
                tracing::info!(task = %name, "Replaced task runner (config changed)");
            }
        }

        // 3. Add runners for new tasks (stopped state, not started)
        for name in new_names.difference(&old_names) {
            let task_config = new_config_map[name.as_str()];
            let runner = TaskRunner::new(task_config.clone(), self.event_tx.clone());
            self.runners.insert(name.clone(), runner);
            tracing::info!(task = %name, "Added new task runner");
        }

        // 4. Rebuild dependency graph
        match DependencyGraph::from_config(&new_config) {
            Ok(graph) => self.dependency_graph = graph,
            Err(e) => {
                tracing::error!(error = %e, "Failed to rebuild dependency graph during reload");
            }
        }
    }

    async fn stop_all(&mut self) {
        let names: Vec<String> = self.runners.keys().cloned().collect();
        for name in names {
            self.stop_task(&name).await;
        }
    }
}
