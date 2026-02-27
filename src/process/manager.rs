use std::collections::HashMap;

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
        for group in groups {
            for name in &group {
                self.start_task(name).await;
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
