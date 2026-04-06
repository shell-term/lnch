use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, watch, Notify};

use crate::config::model::TaskConfig;
use crate::message::{ProcessEvent, TaskStatus};
use crate::process::ready::{CheckType, ReadyResult};

pub struct TaskRunner {
    config: TaskConfig,
    pid: Option<u32>,
    exit_monitor: Option<tokio::task::JoinHandle<(String, Option<i32>)>>,
    event_tx: mpsc::Sender<ProcessEvent>,
    log_tasks: Vec<tokio::task::JoinHandle<()>>,
    #[cfg(windows)]
    pty_input: Option<std::fs::File>,
    ready_notify: Arc<Notify>,
    ready_flag: Arc<AtomicBool>,
    exit_code_tx: watch::Sender<Option<Option<i32>>>,
    exit_code_rx: watch::Receiver<Option<Option<i32>>>,
}

impl TaskRunner {
    pub fn new(config: TaskConfig, event_tx: mpsc::Sender<ProcessEvent>) -> Self {
        let (exit_code_tx, exit_code_rx) = watch::channel(None);
        Self {
            config,
            pid: None,
            exit_monitor: None,
            event_tx,
            log_tasks: Vec::new(),
            #[cfg(windows)]
            pty_input: None,
            ready_notify: Arc::new(Notify::new()),
            ready_flag: Arc::new(AtomicBool::new(false)),
            exit_code_tx,
            exit_code_rx,
        }
    }

    #[allow(dead_code)]
    pub fn task_name(&self) -> &str {
        &self.config.name
    }

    // ------------------------------------------------------------------
    // Start
    // ------------------------------------------------------------------

    pub async fn start(&mut self) -> anyhow::Result<()> {
        // Reset readiness state for fresh start / restart
        self.ready_flag.store(false, Ordering::Release);
        let _ = self.exit_code_tx.send(None);

        self.send_status(TaskStatus::Starting).await;

        #[cfg(windows)]
        {
            match self.start_with_pty().await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!(
                        task = %self.config.name,
                        error = %e,
                        "ConPTY unavailable, falling back to pipe mode"
                    );
                }
            }
        }

        self.start_with_pipes().await
    }

    /// Pipe-based process startup (all platforms; fallback on Windows).
    async fn start_with_pipes(&mut self) -> anyhow::Result<()> {
        let mut cmd = self.build_command();
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        #[cfg(unix)]
        unsafe {
            #[allow(unused_imports)]
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                nix::unistd::setsid()
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                Ok(())
            });
        }

        #[cfg(windows)]
        {
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
            cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }

        match cmd.spawn() {
            Ok(mut child) => {
                if let Some(stdout) = child.stdout.take() {
                    self.spawn_log_reader(stdout, false);
                }
                if let Some(stderr) = child.stderr.take() {
                    self.spawn_log_reader(stderr, true);
                }

                self.pid = child.id();
                self.send_status(TaskStatus::Running).await;
                tracing::info!(task = %self.config.name, pid = ?self.pid, "Task started (pipe mode)");

                self.spawn_exit_monitor(child);

                Ok(())
            }
            Err(e) => {
                let err_msg = format!("Failed to start: {}", e);
                self.send_log_line(err_msg, true).await;
                self.send_status(TaskStatus::Failed { exit_code: None })
                    .await;
                Err(e.into())
            }
        }
    }

    /// ConPTY-based process startup (Windows only).
    ///
    /// Gives the child a virtual console so grandchild processes (e.g. Python
    /// multiprocessing workers) receive valid console handles regardless of
    /// handle inheritance settings.
    #[cfg(windows)]
    async fn start_with_pty(&mut self) -> anyhow::Result<()> {
        use crate::process::pty::PtyProcess;

        let mut pty = PtyProcess::spawn(
            &self.config.command,
            self.config.working_dir.as_deref(),
            self.config.env.as_ref(),
        )?;

        let pid = pty.pid();
        self.pid = Some(pid);

        if let Some(output) = pty.take_output() {
            self.spawn_pty_log_reader(output);
        }

        self.pty_input = Some(pty.write_input_handle()?);

        self.send_status(TaskStatus::Running).await;
        tracing::info!(task = %self.config.name, pid = pid, "Task started (ConPTY mode)");

        self.spawn_pty_exit_monitor(pty);

        Ok(())
    }

    // ------------------------------------------------------------------
    // Stop
    // ------------------------------------------------------------------

    pub async fn stop(&mut self) -> anyhow::Result<()> {
        let pid = match self.pid {
            Some(pid) => pid,
            None => return Ok(()),
        };

        self.send_status(TaskStatus::Stopping).await;

        if self.is_running() {
            self.graceful_terminate(pid);

            let exited = match self.exit_monitor.as_mut() {
                Some(handle) => tokio::time::timeout(Duration::from_secs(5), handle)
                    .await
                    .is_ok(),
                None => true,
            };

            if !exited {
                tracing::warn!(task = %self.config.name, "Graceful shutdown timed out, force killing");
                Self::force_kill(pid);
                if let Some(handle) = self.exit_monitor.take() {
                    handle.abort();
                }
            }
        }

        self.exit_monitor = None;
        self.pid = None;
        #[cfg(windows)]
        {
            self.pty_input = None;
        }
        self.cleanup_log_tasks().await;
        self.send_status(TaskStatus::Stopped).await;
        tracing::info!(task = %self.config.name, "Task stopped");
        Ok(())
    }

    fn graceful_terminate(&mut self, pid: u32) {
        #[cfg(windows)]
        {
            if let Some(ref mut input) = self.pty_input {
                use std::io::Write;
                let _ = input.write_all(b"\x03");
                return;
            }

            use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
            let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) };
            if sent == 0 {
                Self::run_taskkill(&["/T", "/PID", &pid.to_string()]);
            }
        }
        #[cfg(unix)]
        {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGTERM,
            );
        }
    }

    fn force_kill(pid: u32) {
        #[cfg(unix)]
        {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        #[cfg(windows)]
        {
            Self::run_taskkill(&["/F", "/T", "/PID", &pid.to_string()]);
        }
    }

    #[cfg(windows)]
    fn run_taskkill(args: &[&str]) {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new("taskkill")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    // ------------------------------------------------------------------
    // Restart / status
    // ------------------------------------------------------------------

    #[allow(dead_code)]
    pub async fn restart(&mut self) -> anyhow::Result<()> {
        self.stop().await?;
        self.start().await
    }

    pub fn is_running(&self) -> bool {
        self.exit_monitor.as_ref().is_some_and(|h| !h.is_finished())
    }

    pub fn config_ref(&self) -> &TaskConfig {
        &self.config
    }

    // ------------------------------------------------------------------
    // Exit monitor (pipe mode)
    // ------------------------------------------------------------------

    fn spawn_exit_monitor(&mut self, mut child: Child) {
        let name = self.config.name.clone();
        let event_tx = self.event_tx.clone();
        let exit_code_tx = self.exit_code_tx.clone();

        let handle = tokio::spawn(async move {
            let status = child.wait().await;

            let exit_code = match &status {
                Ok(s) => {
                    tracing::info!(task = %name, status = ?s, "Process exited");
                    s.code()
                }
                Err(e) => {
                    tracing::error!(task = %name, error = %e, "Process wait failed");
                    let _ = event_tx
                        .send(ProcessEvent::LogLine {
                            task_name: name.clone(),
                            line: format!("Process error: {}", e),
                            is_stderr: true,
                        })
                        .await;
                    None
                }
            };

            let _ = exit_code_tx.send(Some(exit_code));
            send_exit_events(&event_tx, &name, exit_code).await;
            (name, exit_code)
        });

        self.exit_monitor = Some(handle);
    }

    // ------------------------------------------------------------------
    // Exit monitor (ConPTY mode)
    // ------------------------------------------------------------------

    #[cfg(windows)]
    fn spawn_pty_exit_monitor(&mut self, pty: crate::process::pty::PtyProcess) {
        let name = self.config.name.clone();

        let handle = tokio::task::spawn_blocking(move || {
            let exit_code = pty.wait();
            // `pty` is dropped here → ClosePseudoConsole → output pipe EOF
            (name, exit_code)
        });

        let event_tx_for_exit = self.event_tx.clone();
        let task_name_for_exit = self.config.name.clone();
        let exit_code_tx = self.exit_code_tx.clone();

        let wrapped = tokio::spawn(async move {
            let result = handle.await;
            let (name, exit_code) = match result {
                Ok(pair) => pair,
                Err(e) => {
                    tracing::error!(task = %task_name_for_exit, error = %e, "PTY exit monitor panicked");
                    (task_name_for_exit, None)
                }
            };
            let _ = exit_code_tx.send(Some(exit_code));
            send_exit_events(&event_tx_for_exit, &name, exit_code).await;
            (name, exit_code)
        });

        self.exit_monitor = Some(wrapped);
    }

    // ------------------------------------------------------------------
    // Log readers
    // ------------------------------------------------------------------

    fn spawn_log_reader<R>(&mut self, reader: R, is_stderr: bool)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let event_tx = self.event_tx.clone();
        let task_name = self.config.name.clone();
        let ready_notify = self.ready_notify.clone();
        let ready_flag = self.ready_flag.clone();
        let pattern = self.log_line_pattern();

        let handle = tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(ref pat) = pattern {
                    if line.contains(pat.as_str()) {
                        ready_flag.store(true, Ordering::Release);
                        ready_notify.notify_one();
                    }
                }
                let _ = event_tx
                    .send(ProcessEvent::LogLine {
                        task_name: task_name.clone(),
                        line,
                        is_stderr,
                    })
                    .await;
            }
        });

        self.log_tasks.push(handle);
    }

    /// Read lines from the ConPTY output pipe in a blocking thread.
    /// ANSI escape codes are stripped since the TUI renders its own styles.
    #[cfg(windows)]
    fn spawn_pty_log_reader(&mut self, output: std::fs::File) {
        let event_tx = self.event_tx.clone();
        let task_name = self.config.name.clone();
        let ready_notify = self.ready_notify.clone();
        let ready_flag = self.ready_flag.clone();
        let pattern = self.log_line_pattern();

        let handle = tokio::task::spawn_blocking(move || {
            use std::io::{BufRead, BufReader};

            let reader = BufReader::new(output);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };
                let cleaned = strip_ansi(&line);
                if cleaned.is_empty() {
                    continue;
                }
                if let Some(ref pat) = pattern {
                    if cleaned.contains(pat.as_str()) {
                        ready_flag.store(true, Ordering::Release);
                        ready_notify.notify_one();
                    }
                }
                // blocking_send is fine inside spawn_blocking
                let _ = event_tx.blocking_send(ProcessEvent::LogLine {
                    task_name: task_name.clone(),
                    line: cleaned,
                    is_stderr: false,
                });
            }
        });

        self.log_tasks.push(handle);
    }

    // ------------------------------------------------------------------
    // Cleanup & helpers
    // ------------------------------------------------------------------

    async fn cleanup_log_tasks(&mut self) {
        let handles: Vec<_> = self.log_tasks.drain(..).collect();
        let _ = tokio::time::timeout(
            Duration::from_millis(500),
            futures::future::join_all(handles),
        )
        .await;
    }

    fn build_command(&self) -> tokio::process::Command {
        let (shell, flag) = if cfg!(target_os = "windows") {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };

        let mut cmd = tokio::process::Command::new(shell);
        cmd.arg(flag).arg(&self.config.command);

        if let Some(ref dir) = self.config.working_dir {
            cmd.current_dir(dir);
        }

        cmd.env("PYTHONUNBUFFERED", "1");

        if let Some(ref env_vars) = self.config.env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }

        cmd
    }

    /// Extract the log_line pattern from ready_check config, if any.
    fn log_line_pattern(&self) -> Option<String> {
        self.config
            .ready_check
            .as_ref()
            .and_then(|rc| rc.log_line.as_ref())
            .map(|ll| ll.pattern.clone())
    }

    /// Determine the effective check type for this task.
    fn effective_check_type(&self) -> CheckType {
        match self.config.ready_check {
            Some(ref rc) => {
                if let Some(ref tcp) = rc.tcp {
                    CheckType::Tcp(tcp.port)
                } else if let Some(ref http) = rc.http {
                    CheckType::Http {
                        url: http.url.clone(),
                        status: http.status,
                    }
                } else if rc.log_line.is_some() {
                    CheckType::LogLine
                } else {
                    CheckType::Exit
                }
            }
            None => CheckType::SmartDefault,
        }
    }

    fn effective_timeout(&self) -> Duration {
        let secs = self
            .config
            .ready_check
            .as_ref()
            .and_then(|rc| rc.timeout)
            .unwrap_or(30);
        Duration::from_secs(secs)
    }

    fn effective_interval(&self) -> Duration {
        let ms = self
            .config
            .ready_check
            .as_ref()
            .and_then(|rc| rc.interval)
            .unwrap_or(500);
        Duration::from_millis(ms)
    }

    /// Wait for this task to become ready according to its ready_check config
    /// or smart defaults.
    pub async fn wait_ready(&self) -> ReadyResult {
        let check_type = self.effective_check_type();
        let timeout = self.effective_timeout();
        let interval = self.effective_interval();

        match check_type {
            CheckType::SmartDefault => {
                crate::process::ready::wait_smart_default(
                    self.exit_code_rx.clone(),
                )
                .await
            }
            CheckType::Exit => {
                crate::process::ready::wait_exit(self.exit_code_rx.clone(), timeout).await
            }
            CheckType::Tcp(port) => {
                crate::process::ready::wait_tcp(port, timeout, interval).await
            }
            CheckType::Http { url, status } => {
                crate::process::ready::wait_http(&url, status, timeout, interval).await
            }
            CheckType::LogLine => {
                crate::process::ready::wait_log_line(
                    self.ready_flag.clone(),
                    self.ready_notify.clone(),
                    timeout,
                )
                .await
            }
        }
    }

    async fn send_status(&self, status: TaskStatus) {
        let _ = self
            .event_tx
            .send(ProcessEvent::StatusChanged {
                task_name: self.config.name.clone(),
                status,
            })
            .await;
    }

    async fn send_log_line(&self, line: String, is_stderr: bool) {
        let _ = self
            .event_tx
            .send(ProcessEvent::LogLine {
                task_name: self.config.name.clone(),
                line,
                is_stderr,
            })
            .await;
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

async fn send_exit_events(
    event_tx: &mpsc::Sender<ProcessEvent>,
    name: &str,
    exit_code: Option<i32>,
) {
    if exit_code != Some(0) {
        let msg = match exit_code {
            Some(code) => format!("Process exited with code {} {}", code, exit_code_hint(code)),
            None => "Process terminated (unknown exit code)".to_string(),
        };
        let _ = event_tx
            .send(ProcessEvent::LogLine {
                task_name: name.to_owned(),
                line: msg,
                is_stderr: true,
            })
            .await;
    }

    let task_status = match exit_code {
        Some(0) => TaskStatus::Stopped,
        code => TaskStatus::Failed { exit_code: code },
    };

    let _ = event_tx
        .send(ProcessEvent::StatusChanged {
            task_name: name.to_owned(),
            status: task_status,
        })
        .await;

    let _ = event_tx
        .send(ProcessEvent::ProcessExited {
            task_name: name.to_owned(),
            exit_code,
        })
        .await;
}

fn exit_code_hint(code: i32) -> &'static str {
    match code {
        9009 => "(command not found — check that the program is installed and in PATH)",
        3 => "(path not found)",
        5 => "(access denied)",
        130 => "(interrupted by Ctrl+C / SIGINT)",
        137 => "(killed / SIGKILL)",
        143 => "(terminated / SIGTERM)",
        _ => "",
    }
}

/// Strip ANSI escape sequences from ConPTY output.
#[cfg(windows)]
fn strip_ansi(input: &str) -> String {
    let bytes = strip_ansi_escapes::strip(input);
    String::from_utf8_lossy(&bytes).into_owned()
}

impl Drop for TaskRunner {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            Self::force_kill(pid);
        }
        if let Some(handle) = self.exit_monitor.take() {
            handle.abort();
        }
        for handle in &self.log_tasks {
            handle.abort();
        }
    }
}
