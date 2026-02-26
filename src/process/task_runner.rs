use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::mpsc;

use crate::config::model::TaskConfig;
use crate::message::{ProcessEvent, TaskStatus};

pub struct TaskRunner {
    config: TaskConfig,
    child: Option<Child>,
    event_tx: mpsc::Sender<ProcessEvent>,
    log_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl TaskRunner {
    pub fn new(config: TaskConfig, event_tx: mpsc::Sender<ProcessEvent>) -> Self {
        Self {
            config,
            child: None,
            event_tx,
            log_tasks: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn task_name(&self) -> &str {
        &self.config.name
    }

    pub async fn start(&mut self) -> anyhow::Result<()> {
        self.send_status(TaskStatus::Starting).await;

        let mut cmd = self.build_command();
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        #[cfg(unix)]
        unsafe {
            #[allow(unused_imports)]
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                    nix::unistd::setsid().map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, e)
                    })?;
                    Ok(())
            });
        }

        match cmd.spawn() {
            Ok(mut child) => {
                if let Some(stdout) = child.stdout.take() {
                    self.spawn_log_reader(stdout, false);
                }
                if let Some(stderr) = child.stderr.take() {
                    self.spawn_log_reader(stderr, true);
                }

                let pid = child.id();
                self.child = Some(child);
                self.send_status(TaskStatus::Running).await;

                tracing::info!(task = %self.config.name, ?pid, "Task started");
                Ok(())
            }
            Err(e) => {
                let err_msg = format!("Failed to start: {}", e);
                self.send_log_line(err_msg, true).await;
                self.send_status(TaskStatus::Failed { exit_code: None }).await;
                Err(e.into())
            }
        }
    }

    pub async fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(ref child) = self.child {
            self.send_status(TaskStatus::Stopping).await;

            let pid = child.id();

            #[cfg(unix)]
            if let Some(pid) = pid {
                let _ = nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(pid as i32),
                    nix::sys::signal::Signal::SIGTERM,
                );
            }

            #[cfg(windows)]
            if let Some(ref mut child) = self.child {
                let _ = child.kill().await;
            }

            match tokio::time::timeout(Duration::from_secs(5), self.wait_for_exit()).await {
                Ok(_) => {}
                Err(_) => {
                    tracing::warn!(task = %self.config.name, "Graceful shutdown timed out, sending SIGKILL");
                    #[cfg(unix)]
                    if let Some(pid) = pid {
                        let _ = nix::sys::signal::killpg(
                            nix::unistd::Pid::from_raw(pid as i32),
                            nix::sys::signal::Signal::SIGKILL,
                        );
                    }
                    #[cfg(not(unix))]
                    if let Some(ref mut child) = self.child {
                        let _ = child.kill().await;
                    }
                }
            }
        }

        self.cleanup_log_tasks().await;
        self.child = None;
        self.send_status(TaskStatus::Stopped).await;
        tracing::info!(task = %self.config.name, "Task stopped");
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn restart(&mut self) -> anyhow::Result<()> {
        self.stop().await?;
        self.start().await
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// Spawn exit monitor; returns a JoinHandle that resolves when the child exits
    pub fn spawn_exit_monitor(&mut self) -> Option<tokio::task::JoinHandle<(String, Option<i32>)>> {
        if let Some(mut child) = self.child.take() {
            let name = self.config.name.clone();
            let event_tx = self.event_tx.clone();

            let handle = tokio::spawn(async move {
                let status = child.wait().await;
                let exit_code = status.ok().and_then(|s| s.code());

                let task_status = match exit_code {
                    Some(0) => TaskStatus::Stopped,
                    code => TaskStatus::Failed { exit_code: code },
                };

                let _ = event_tx
                    .send(ProcessEvent::StatusChanged {
                        task_name: name.clone(),
                        status: task_status,
                    })
                    .await;

                let _ = event_tx
                    .send(ProcessEvent::ProcessExited {
                        task_name: name.clone(),
                        exit_code,
                    })
                    .await;

                (name, exit_code)
            });

            Some(handle)
        } else {
            None
        }
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

        if let Some(ref env_vars) = self.config.env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }

        cmd
    }

    fn spawn_log_reader<R>(&mut self, reader: R, is_stderr: bool)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let event_tx = self.event_tx.clone();
        let task_name = self.config.name.clone();

        let handle = tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
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

    async fn wait_for_exit(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.wait().await;
        }
    }

    async fn cleanup_log_tasks(&mut self) {
        for handle in self.log_tasks.drain(..) {
            handle.abort();
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

impl Drop for TaskRunner {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(ref child) = self.child {
            if let Some(pid) = child.id() {
                let _ = nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(pid as i32),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
        }
        for handle in &self.log_tasks {
            handle.abort();
        }
    }
}
