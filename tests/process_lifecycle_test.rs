use std::time::Duration;
use tokio::sync::mpsc;

use lnch::config::model::TaskConfig;
use lnch::message::{ProcessEvent, TaskStatus};
use lnch::process::task_runner::TaskRunner;

fn long_running_config(name: &str, command: &str) -> TaskConfig {
    TaskConfig {
        name: name.to_string(),
        command: command.to_string(),
        working_dir: None,
        env: None,
        color: None,
        depends_on: None,
    }
}

#[tokio::test]
async fn test_process_stays_alive_for_duration() {
    let (event_tx, mut event_rx) = mpsc::channel(256);
    let config = long_running_config("long-sleep", "sleep 300");
    let mut runner = TaskRunner::new(config, event_tx);

    runner.start().await.unwrap();
    assert!(runner.is_running(), "Process should be running after start");

    // Wait 3 seconds and check periodically
    for i in 0..6 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            runner.is_running(),
            "Process should still be running after {}ms",
            (i + 1) * 500
        );
    }

    // Check that no unexpected exit events were received
    while let Ok(event) = event_rx.try_recv() {
        if let ProcessEvent::ProcessExited { exit_code, .. } = &event {
            panic!("Process exited unexpectedly with code: {:?}", exit_code);
        }
    }

    runner.stop().await.unwrap();
    assert!(!runner.is_running(), "Process should be stopped after stop");
}

#[tokio::test]
async fn test_process_manager_keeps_processes_alive() {
    use lnch::config::model::LnchConfig;
    use lnch::message::ProcessCommand;
    use lnch::process::dependency::DependencyGraph;
    use lnch::process::manager::ProcessManager;

    let config = LnchConfig {
        name: "test".to_string(),
        tasks: vec![
            long_running_config("task-a", "sleep 300"),
            long_running_config("task-b", "sleep 300"),
        ],
    };

    let dep_graph = DependencyGraph::from_config(&config).unwrap();
    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    let (event_tx, mut event_rx) = mpsc::channel(256);

    let mut manager = ProcessManager::new(&config, dep_graph, cmd_rx, event_tx);
    let manager_handle = tokio::spawn(async move {
        manager.run().await;
    });

    // Start all tasks
    cmd_tx.send(ProcessCommand::StartAll).await.unwrap();

    // Wait for tasks to start
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Collect status events
    let mut running_tasks = std::collections::HashSet::new();
    while let Ok(event) = event_rx.try_recv() {
        if let ProcessEvent::StatusChanged {
            task_name, status, ..
        } = &event
        {
            if matches!(status, TaskStatus::Running) {
                running_tasks.insert(task_name.clone());
            }
        }
    }
    assert!(
        running_tasks.contains("task-a"),
        "task-a should have started"
    );
    assert!(
        running_tasks.contains("task-b"),
        "task-b should have started"
    );

    // Wait 3 more seconds
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Check no unexpected exits occurred
    let mut unexpected_exits = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        if let ProcessEvent::ProcessExited {
            task_name,
            exit_code,
        } = event
        {
            unexpected_exits.push((task_name, exit_code));
        }
    }
    assert!(
        unexpected_exits.is_empty(),
        "No tasks should have exited unexpectedly, but got: {:?}",
        unexpected_exits
    );

    // Shutdown
    cmd_tx.send(ProcessCommand::Shutdown).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(10), manager_handle).await;
}
