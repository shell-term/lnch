use tokio::sync::mpsc;

use lnch::config::model::TaskConfig;
use lnch::message::ProcessEvent;
use lnch::process::task_runner::TaskRunner;

fn simple_task_config(name: &str, command: &str) -> TaskConfig {
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

#[tokio::test]
async fn test_start_and_stop_simple_process() {
    let (event_tx, mut event_rx) = mpsc::channel(64);
    let config = simple_task_config("test-sleep", "sleep 60");
    let mut runner = TaskRunner::new(config, event_tx);

    runner.start().await.unwrap();

    // Collect status events
    let mut got_starting = false;
    let mut got_running = false;
    while let Ok(event) = event_rx.try_recv() {
        if let ProcessEvent::StatusChanged { status, .. } = event {
            match status {
                lnch::message::TaskStatus::Starting => got_starting = true,
                lnch::message::TaskStatus::Running => got_running = true,
                _ => {}
            }
        }
    }
    assert!(got_starting, "Should have received Starting status");
    assert!(got_running, "Should have received Running status");
    assert!(runner.is_running());

    runner.stop().await.unwrap();
    assert!(!runner.is_running());
}

#[tokio::test]
async fn test_log_capture() {
    let (event_tx, mut event_rx) = mpsc::channel(256);
    let config = simple_task_config("test-echo", "echo hello");
    let mut runner = TaskRunner::new(config, event_tx);

    runner.start().await.unwrap();

    // Poll events with a timeout, looking for the log line or process exit.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut found_hello = false;
    let mut process_exited = false;
    let mut received: Vec<String> = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, event_rx.recv()).await {
            Ok(Some(ProcessEvent::LogLine { line, .. })) => {
                received.push(format!("LogLine: {:?}", line));
                if line.contains("hello") {
                    found_hello = true;
                    break;
                }
            }
            Ok(Some(ProcessEvent::ProcessExited { exit_code, .. })) => {
                received.push(format!("ProcessExited({:?})", exit_code));
                process_exited = true;
                // Process done; drain any remaining buffered events.
                while let Ok(event) = event_rx.try_recv() {
                    if let ProcessEvent::LogLine { line, .. } = event {
                        received.push(format!("LogLine(drain): {:?}", line));
                        if line.contains("hello") {
                            found_hello = true;
                        }
                    }
                }
                break;
            }
            Ok(Some(event)) => {
                received.push(format!("Other: {:?}", event));
            }
            Ok(None) | Err(_) => break,
        }
    }
    if !found_hello {
        eprintln!("process_exited={process_exited}, events={received:#?}");
    }
    assert!(found_hello, "Should have captured 'hello' in logs");
}
