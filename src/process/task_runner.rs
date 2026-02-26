use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::mpsc;

use crate::config::model::TaskConfig;
use crate::message::{ProcessEvent, TaskStatus};

pub struct TaskRunner {
    config: TaskConfig,
    pid: Option<u32>,
    exit_monitor: Option<tokio::task::JoinHandle<(String, Option<i32>)>>,
    event_tx: mpsc::Sender<ProcessEvent>,
    log_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl TaskRunner {
    pub fn new(config: TaskConfig, event_tx: mpsc::Sender<ProcessEvent>) -> Self {
        Self {
            config,
            pid: None,
            exit_monitor: None,
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
        cmd.kill_on_drop(true);

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

                self.pid = child.id();
                self.send_status(TaskStatus::Running).await;
                tracing::info!(task = %self.config.name, pid = ?self.pid, "Task started");

                self.spawn_exit_monitor(child);

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

    #[allow(unused_variables)]
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        let pid = match self.pid {
            Some(pid) => pid,
            None => return Ok(()),
        };

        self.send_status(TaskStatus::Stopping).await;

        if self.is_running() {
            #[cfg(unix)]
            {
                let _ = nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(pid as i32),
                    nix::sys::signal::Signal::SIGTERM,
                );
            }

            #[cfg(windows)]
            {
                if let Some(handle) = self.exit_monitor.take() {
                    handle.abort();
                }
            }

            let exited = match self.exit_monitor.as_mut() {
                Some(handle) => {
                    tokio::time::timeout(Duration::from_secs(5), handle)
                        .await
                        .is_ok()
                }
                None => true,
            };

            if !exited {
                tracing::warn!(task = %self.config.name, "Graceful shutdown timed out, force killing");
                #[cfg(unix)]
                {
                    let _ = nix::sys::signal::killpg(
                        nix::unistd::Pid::from_raw(pid as i32),
                        nix::sys::signal::Signal::SIGKILL,
                    );
                }
                if let Some(handle) = self.exit_monitor.take() {
                    handle.abort();
                }
            }
        }

        self.exit_monitor = None;
        self.pid = None;
        self.cleanup_log_tasks().await;
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
        self.exit_monitor
            .as_ref()
            .map_or(false, |h| !h.is_finished())
    }

    fn spawn_exit_monitor(&mut self, mut child: Child) {
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

        self.exit_monitor = Some(handle);
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
        if let Some(pid) = self.pid {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        if let Some(handle) = self.exit_monitor.take() {
            handle.abort();
        }
        for handle in &self.log_tasks {
            handle.abort();
        }
    }
}
