use crossterm::event::KeyEvent;

/// Runtime status of a task
#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed { exit_code: Option<i32> },
}

/// Commands from App -> ProcessManager
#[derive(Debug)]
#[allow(dead_code)]
pub enum ProcessCommand {
    Start(String),
    Stop(String),
    Restart(String),
    StartAll,
    StopAll,
    Shutdown,
}

/// Events from ProcessManager -> App
#[derive(Debug)]
pub enum ProcessEvent {
    StatusChanged {
        task_name: String,
        status: TaskStatus,
    },
    LogLine {
        task_name: String,
        line: String,
        is_stderr: bool,
    },
    ProcessExited {
        task_name: String,
        exit_code: Option<i32>,
    },
}

/// TUI events
#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Process(ProcessEvent),
}
