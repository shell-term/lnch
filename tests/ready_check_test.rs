use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use lnch::config::model::{
    ExitCheck, LnchConfig, LogLineCheck, ReadyCheckConfig, TaskConfig, TcpCheck,
};
use lnch::message::{ProcessCommand, ProcessEvent, TaskStatus};
use lnch::process::dependency::DependencyGraph;
use lnch::process::manager::ProcessManager;
use lnch::process::ready::ReadyResult;
use lnch::process::task_runner::TaskRunner;

fn task(name: &str, command: &str) -> TaskConfig {
    TaskConfig {
        name: name.to_string(),
        command: command.to_string(),
        working_dir: None,
        env: None,
        color: None,
        depends_on: None,
        ready_check: None,
    }
}

fn task_with_deps(name: &str, command: &str, deps: Vec<&str>) -> TaskConfig {
    TaskConfig {
        name: name.to_string(),
        command: command.to_string(),
        working_dir: None,
        env: None,
        color: None,
        depends_on: Some(deps.into_iter().map(String::from).collect()),
        ready_check: None,
    }
}

fn ready_check_config(
    tcp: Option<TcpCheck>,
    log_line: Option<LogLineCheck>,
    exit: Option<ExitCheck>,
    timeout: Option<u64>,
    interval: Option<u64>,
) -> ReadyCheckConfig {
    ReadyCheckConfig {
        tcp,
        http: None,
        log_line,
        exit,
        timeout,
        interval,
    }
}

/// Helper: run ProcessManager with StartAll and collect events until timeout.
async fn run_manager_and_collect_events(
    config: LnchConfig,
    collect_duration: Duration,
) -> Vec<ProcessEvent> {
    let dep_graph = DependencyGraph::from_config(&config).unwrap();
    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    let (event_tx, mut event_rx) = mpsc::channel(256);

    let mut manager = ProcessManager::new(&config, dep_graph, cmd_rx, event_tx);
    let manager_handle = tokio::spawn(async move {
        manager.run().await;
    });

    cmd_tx.send(ProcessCommand::StartAll).await.unwrap();

    // Collect events for the specified duration
    let mut events = Vec::new();
    let deadline = tokio::time::Instant::now() + collect_duration;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, event_rx.recv()).await {
            Ok(Some(event)) => events.push(event),
            _ => break,
        }
    }

    // Drain remaining events
    while let Ok(event) = event_rx.try_recv() {
        events.push(event);
    }

    cmd_tx.send(ProcessCommand::Shutdown).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(10), manager_handle).await;

    events
}

/// Extract the order in which tasks transitioned to Running status.
fn running_order(events: &[ProcessEvent]) -> Vec<String> {
    let mut order = Vec::new();
    for event in events {
        if let ProcessEvent::StatusChanged {
            task_name, status, ..
        } = event
        {
            if matches!(status, TaskStatus::Running) {
                order.push(task_name.clone());
            }
        }
    }
    order
}

// ============================================================================
// Smart Default Tests
// ============================================================================

/// One-shot task (exits immediately) as dependency: next group waits for exit.
#[tokio::test]
async fn test_smart_default_oneshot_dependency() {
    // task-a: runs echo (exits quickly with code 0)
    // task-b: depends on task-a
    let a = task("task-a", "echo done");
    let b = task_with_deps("task-b", "echo after-dep", vec!["task-a"]);

    let config = LnchConfig {
        name: "test".to_string(),
        tasks: vec![a, b],
    };

    let events = run_manager_and_collect_events(config, Duration::from_secs(8)).await;
    let order = running_order(&events);

    // Both should have started (task-a first)
    assert!(
        order.contains(&"task-a".to_string()),
        "task-a should have started, order: {:?}",
        order
    );
    assert!(
        order.contains(&"task-b".to_string()),
        "task-b should have started, order: {:?}",
        order
    );

    // Verify task-a started before task-b
    let a_idx = order.iter().position(|n| n == "task-a").unwrap();
    let b_idx = order.iter().position(|n| n == "task-b").unwrap();
    assert!(a_idx < b_idx, "task-a should start before task-b");
}

/// Long-running task as dependency: next group starts after grace period (~2s).
#[tokio::test]
async fn test_smart_default_long_running_dependency() {
    // task-a: long-running (sleep)
    // task-b: depends on task-a, should start after ~2s grace period
    let a = task("task-a", "sleep 300");
    let b = task_with_deps("task-b", "sleep 300", vec!["task-a"]);

    let config = LnchConfig {
        name: "test".to_string(),
        tasks: vec![a, b],
    };

    let events = run_manager_and_collect_events(config, Duration::from_secs(8)).await;
    let order = running_order(&events);

    assert!(
        order.contains(&"task-a".to_string()),
        "task-a should have started"
    );
    assert!(
        order.contains(&"task-b".to_string()),
        "task-b should have started"
    );

    // task-b should have started after task-a's smart default grace period
    let a_idx = order.iter().position(|n| n == "task-a").unwrap();
    let b_idx = order.iter().position(|n| n == "task-b").unwrap();
    assert!(a_idx < b_idx, "task-a should start before task-b");
}

// ============================================================================
// Explicit Ready Check Tests
// ============================================================================

/// Exit ready check: explicitly wait for process to exit with code 0.
#[tokio::test]
async fn test_exit_ready_check() {
    let mut a = task("task-a", "echo done");
    a.ready_check = Some(ready_check_config(
        None,
        None,
        Some(ExitCheck {}),
        Some(10),
        None,
    ));
    let b = task_with_deps("task-b", "echo after", vec!["task-a"]);

    let config = LnchConfig {
        name: "test".to_string(),
        tasks: vec![a, b],
    };

    let events = run_manager_and_collect_events(config, Duration::from_secs(8)).await;
    let order = running_order(&events);

    assert!(order.contains(&"task-a".to_string()));
    assert!(order.contains(&"task-b".to_string()));

    let a_idx = order.iter().position(|n| n == "task-a").unwrap();
    let b_idx = order.iter().position(|n| n == "task-b").unwrap();
    assert!(a_idx < b_idx);
}

/// Log line ready check: wait for a specific pattern in stdout.
#[tokio::test]
async fn test_log_line_ready_check() {
    // task-a: prints "server ready" after a delay
    let cmd_a = if cfg!(target_os = "windows") {
        r#"cmd /C "echo starting && ping -n 2 127.0.0.1 >nul && echo server ready""#
    } else {
        "echo starting && sleep 1 && echo 'server ready'"
    };

    let mut a = task("task-a", cmd_a);
    a.ready_check = Some(ready_check_config(
        None,
        Some(LogLineCheck {
            pattern: "server ready".to_string(),
        }),
        None,
        Some(10),
        None,
    ));
    let b = task_with_deps("task-b", "echo after", vec!["task-a"]);

    let config = LnchConfig {
        name: "test".to_string(),
        tasks: vec![a, b],
    };

    let events = run_manager_and_collect_events(config, Duration::from_secs(10)).await;
    let order = running_order(&events);

    assert!(
        order.contains(&"task-a".to_string()),
        "task-a should have started"
    );
    assert!(
        order.contains(&"task-b".to_string()),
        "task-b should have started"
    );

    // Verify ordering
    let a_idx = order.iter().position(|n| n == "task-a").unwrap();
    let b_idx = order.iter().position(|n| n == "task-b").unwrap();
    assert!(a_idx < b_idx);

    // Verify that "server ready" log line was captured
    let has_ready_line = events.iter().any(|e| {
        if let ProcessEvent::LogLine { line, .. } = e {
            line.contains("server ready")
        } else {
            false
        }
    });
    assert!(has_ready_line, "Should have captured 'server ready' log line");
}

/// TCP ready check: wait for a port to be listening.
#[tokio::test]
async fn test_tcp_ready_check() {
    // task-a: starts a simple TCP listener using Python
    let cmd_a = if cfg!(target_os = "windows") {
        "python -c \"import socket,time; s=socket.socket(); s.bind(('127.0.0.1',18234)); s.listen(1); print('listening'); time.sleep(30)\""
    } else {
        "python3 -c \"import socket,time; s=socket.socket(); s.bind(('127.0.0.1',18234)); s.listen(1); print('listening'); time.sleep(30)\""
    };

    let mut a = task("task-a", cmd_a);
    a.ready_check = Some(ready_check_config(
        Some(TcpCheck { port: 18234 }),
        None,
        None,
        Some(15),
        Some(200),
    ));
    let b = task_with_deps("task-b", "echo connected", vec!["task-a"]);

    let config = LnchConfig {
        name: "test".to_string(),
        tasks: vec![a, b],
    };

    let events = run_manager_and_collect_events(config, Duration::from_secs(15)).await;
    let order = running_order(&events);

    assert!(
        order.contains(&"task-a".to_string()),
        "task-a should have started, events: {:?}",
        order
    );
    assert!(
        order.contains(&"task-b".to_string()),
        "task-b should have started, events: {:?}",
        order
    );

    let a_idx = order.iter().position(|n| n == "task-a").unwrap();
    let b_idx = order.iter().position(|n| n == "task-b").unwrap();
    assert!(a_idx < b_idx);
}

// ============================================================================
// Timeout Test
// ============================================================================

/// Timeout: readiness check times out and continues anyway.
#[tokio::test]
async fn test_ready_check_timeout_continues() {
    // task-a: TCP check on a port that nothing listens on, with short timeout
    let mut a = task("task-a", "sleep 300");
    a.ready_check = Some(ready_check_config(
        Some(TcpCheck { port: 19876 }),
        None,
        None,
        Some(2), // 2 second timeout
        Some(200),
    ));
    let b = task_with_deps("task-b", "echo after-timeout", vec!["task-a"]);

    let config = LnchConfig {
        name: "test".to_string(),
        tasks: vec![a, b],
    };

    let events = run_manager_and_collect_events(config, Duration::from_secs(10)).await;
    let order = running_order(&events);

    // task-b should still start despite timeout
    assert!(
        order.contains(&"task-b".to_string()),
        "task-b should start even after timeout, order: {:?}",
        order
    );

    // Verify timeout log message was sent
    let has_timeout_msg = events.iter().any(|e| {
        if let ProcessEvent::LogLine { line, .. } = e {
            line.contains("timed out")
        } else {
            false
        }
    });
    assert!(
        has_timeout_msg,
        "Should have a timeout message in the logs"
    );
}

// ============================================================================
// No Dependencies: All tasks start simultaneously
// ============================================================================

#[tokio::test]
async fn test_no_deps_all_start_immediately() {
    let config = LnchConfig {
        name: "test".to_string(),
        tasks: vec![
            task("task-a", "sleep 300"),
            task("task-b", "sleep 300"),
            task("task-c", "sleep 300"),
        ],
    };

    let events = run_manager_and_collect_events(config, Duration::from_secs(5)).await;
    let order = running_order(&events);

    assert_eq!(
        order.len(),
        3,
        "All 3 tasks should be running, got: {:?}",
        order
    );
}

// ============================================================================
// Multi-level dependency chain
// ============================================================================

#[tokio::test]
async fn test_multi_level_dependency_chain() {
    // a -> b -> c (linear chain, each with exit ready check)
    let mut a = task("task-a", "echo step-1-done");
    a.ready_check = Some(ready_check_config(
        None,
        None,
        Some(ExitCheck {}),
        Some(10),
        None,
    ));
    let mut b = task_with_deps("task-b", "echo step-2-done", vec!["task-a"]);
    b.ready_check = Some(ready_check_config(
        None,
        None,
        Some(ExitCheck {}),
        Some(10),
        None,
    ));
    let c = task_with_deps("task-c", "echo step-3-done", vec!["task-b"]);

    let config = LnchConfig {
        name: "test".to_string(),
        tasks: vec![a, b, c],
    };

    let events = run_manager_and_collect_events(config, Duration::from_secs(10)).await;
    let order = running_order(&events);

    assert!(order.contains(&"task-a".to_string()));
    assert!(order.contains(&"task-b".to_string()));
    assert!(order.contains(&"task-c".to_string()));

    // Verify strict ordering: a before b before c
    let a_idx = order.iter().position(|n| n == "task-a").unwrap();
    let b_idx = order.iter().position(|n| n == "task-b").unwrap();
    let c_idx = order.iter().position(|n| n == "task-c").unwrap();
    assert!(a_idx < b_idx, "task-a should start before task-b");
    assert!(b_idx < c_idx, "task-b should start before task-c");
}

// ============================================================================
// TaskRunner wait_ready unit tests
// ============================================================================

#[tokio::test]
async fn test_task_runner_wait_ready_smart_default_long_running() {
    let (event_tx, _event_rx) = mpsc::channel(256);
    let config = task("test", "sleep 300");
    let mut runner = TaskRunner::new(config, event_tx);

    runner.start().await.unwrap();

    let start = Instant::now();
    let result = runner.wait_ready().await;
    let elapsed = start.elapsed();

    assert_eq!(result, ReadyResult::Ready);
    // Smart default grace period is 2 seconds
    assert!(
        elapsed >= Duration::from_millis(1500),
        "Should wait at least ~2s grace period, took: {:?}",
        elapsed
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "Should not wait too long, took: {:?}",
        elapsed
    );

    runner.stop().await.unwrap();
}

#[tokio::test]
async fn test_task_runner_wait_ready_exit_check() {
    let (event_tx, _event_rx) = mpsc::channel(256);
    let mut config = task("test", "echo hello");
    config.ready_check = Some(ready_check_config(
        None,
        None,
        Some(ExitCheck {}),
        Some(10),
        None,
    ));
    let mut runner = TaskRunner::new(config, event_tx);

    runner.start().await.unwrap();
    let result = runner.wait_ready().await;

    assert_eq!(result, ReadyResult::Ready);
}
