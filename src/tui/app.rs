use std::cell::Cell;
use std::io;
use std::panic;
use std::time::Instant;

use crossterm::event::KeyCode;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use tokio::sync::mpsc;

use crate::config::model::{LnchConfig, TaskConfig};
use crate::log::buffer::{LogBuffer, LogLine};
use crate::message::{AppEvent, ProcessCommand, ProcessEvent, TaskStatus};

use super::event::{should_quit, spawn_event_reader};
use super::ui;

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn new(terminal: Terminal<CrosstermBackend<io::Stdout>>) -> Self {
        Self { terminal }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

impl std::ops::Deref for TerminalGuard {
    type Target = Terminal<CrosstermBackend<io::Stdout>>;
    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl std::ops::DerefMut for TerminalGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

pub struct TaskState {
    pub config: TaskConfig,
    pub status: TaskStatus,
    pub log_buffer: LogBuffer,
}

pub struct AppState {
    pub project_name: String,
    pub tasks: Vec<TaskState>,
    pub selected_index: usize,
    pub log_scroll_offset: usize,
    pub should_quit: bool,
    pub auto_scroll: bool,
    /// Updated by `render_log_view` each frame via interior mutability,
    /// so scroll logic can compare against the true visual-line max.
    pub last_max_scroll: Cell<usize>,
}

pub struct App {
    state: AppState,
    process_cmd_tx: mpsc::Sender<ProcessCommand>,
    process_event_rx: Option<mpsc::Receiver<ProcessEvent>>,
    app_event_rx: Option<mpsc::Receiver<AppEvent>>,
    app_event_tx: mpsc::Sender<AppEvent>,
}

impl App {
    pub fn new(
        config: &LnchConfig,
        process_cmd_tx: mpsc::Sender<ProcessCommand>,
        process_event_rx: mpsc::Receiver<ProcessEvent>,
    ) -> Self {
        let tasks: Vec<TaskState> = config
            .tasks
            .iter()
            .map(|tc| TaskState {
                config: tc.clone(),
                status: TaskStatus::Stopped,
                log_buffer: LogBuffer::with_default_capacity(),
            })
            .collect();

        let (app_event_tx, app_event_rx) = mpsc::channel(256);

        Self {
            state: AppState {
                project_name: config.name.clone(),
                tasks,
                selected_index: 0,
                log_scroll_offset: 0,
                should_quit: false,
                auto_scroll: true,
                last_max_scroll: Cell::new(0),
            },
            process_cmd_tx,
            process_event_rx: Some(process_event_rx),
            app_event_rx: Some(app_event_rx),
            app_event_tx,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut terminal = TerminalGuard::new(setup_terminal()?);

        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            prev_hook(info);
        }));

        let result = self.event_loop(&mut terminal).await;

        // Drop the guard first (restores terminal), then propagate any error
        drop(terminal);
        result
    }

    async fn event_loop(&mut self, terminal: &mut TerminalGuard) -> anyhow::Result<()> {
        let _event_reader = spawn_event_reader(self.app_event_tx.clone());

        let process_event_tx = self.app_event_tx.clone();
        let mut process_event_rx = self.process_event_rx.take().unwrap();
        tokio::spawn(async move {
            while let Some(event) = process_event_rx.recv().await {
                if process_event_tx
                    .send(AppEvent::Process(event))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut app_event_rx = self.app_event_rx.take().unwrap();

        loop {
            terminal.draw(|frame| ui::render(frame, &self.state))?;

            match app_event_rx.recv().await {
                Some(AppEvent::Key(key)) => {
                    tracing::debug!(key = ?key, "Key event received");
                    self.handle_key(key).await;
                }
                Some(AppEvent::Tick) => {}
                Some(AppEvent::Process(event)) => {
                    self.handle_process_event(event);
                }
                None => {
                    tracing::warn!("App event channel closed, exiting");
                    break;
                }
            }

            if self.state.should_quit {
                tracing::info!("App quit triggered, sending Shutdown");
                let _ = self.process_cmd_tx.send(ProcessCommand::Shutdown).await;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                break;
            }
        }

        Ok(())
    }

    async fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if key.kind != crossterm::event::KeyEventKind::Press {
            return;
        }

        if should_quit(&key) {
            self.state.should_quit = true;
            return;
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.select_previous_task(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next_task(),

            KeyCode::Char('a') => {
                let _ = self.process_cmd_tx.send(ProcessCommand::StartAll).await;
            }
            KeyCode::Char('s') => {
                let name = self.selected_task_name();
                let cmd = if self.is_selected_task_running() {
                    ProcessCommand::Stop(name)
                } else {
                    ProcessCommand::Start(name)
                };
                let _ = self.process_cmd_tx.send(cmd).await;
            }
            KeyCode::Char('r') => {
                let name = self.selected_task_name();
                let _ = self.process_cmd_tx.send(ProcessCommand::Restart(name)).await;
            }

            KeyCode::PageUp => self.scroll_log_up(),
            KeyCode::PageDown => self.scroll_log_down(),
            KeyCode::Home => {
                self.state.log_scroll_offset = 0;
                self.state.auto_scroll = false;
            }
            KeyCode::End => {
                self.state.auto_scroll = true;
                self.snap_scroll_to_bottom();
            }

            _ => {}
        }
    }

    fn handle_process_event(&mut self, event: ProcessEvent) {
        match event {
            ProcessEvent::StatusChanged { task_name, status } => {
                if let Some(task) = self.find_task_mut(&task_name) {
                    task.status = status;
                }
            }
            ProcessEvent::LogLine {
                task_name,
                line,
                is_stderr,
            } => {
                if let Some(task) = self.find_task_mut(&task_name) {
                    task.log_buffer.push(LogLine {
                        content: line,
                        is_stderr,
                        timestamp: Instant::now(),
                    });
                }
                // Auto-scroll if following the selected task
                if self.state.auto_scroll && self.selected_task_name() == task_name {
                    self.snap_scroll_to_bottom();
                }
            }
            ProcessEvent::ProcessExited {
                task_name,
                exit_code,
            } => {
                if let Some(task) = self.find_task_mut(&task_name) {
                    match exit_code {
                        Some(0) => task.status = TaskStatus::Stopped,
                        code => task.status = TaskStatus::Failed { exit_code: code },
                    }
                }
            }
        }
    }

    fn select_previous_task(&mut self) {
        if self.state.selected_index > 0 {
            self.state.selected_index -= 1;
            self.reset_scroll();
        }
    }

    fn select_next_task(&mut self) {
        if self.state.selected_index + 1 < self.state.tasks.len() {
            self.state.selected_index += 1;
            self.reset_scroll();
        }
    }

    fn scroll_log_up(&mut self) {
        self.state.auto_scroll = false;
        self.state.log_scroll_offset = self.state.log_scroll_offset.saturating_sub(10);
    }

    fn scroll_log_down(&mut self) {
        self.state.log_scroll_offset = self.state.log_scroll_offset.saturating_add(10);
        if self.state.log_scroll_offset >= self.state.last_max_scroll.get() {
            self.state.auto_scroll = true;
        }
    }

    fn snap_scroll_to_bottom(&mut self) {
        // Use usize::MAX as a sentinel; render_log_view clamps it to the
        // actual visual-line max_scroll each frame.
        self.state.log_scroll_offset = usize::MAX;
    }

    fn reset_scroll(&mut self) {
        self.state.auto_scroll = true;
        self.snap_scroll_to_bottom();
    }

    fn selected_task_name(&self) -> String {
        self.state
            .tasks
            .get(self.state.selected_index)
            .map(|t| t.config.name.clone())
            .unwrap_or_default()
    }

    fn is_selected_task_running(&self) -> bool {
        self.state
            .tasks
            .get(self.state.selected_index)
            .map(|t| matches!(t.status, TaskStatus::Running | TaskStatus::Starting))
            .unwrap_or(false)
    }

    fn find_task_mut(&mut self, name: &str) -> Option<&mut TaskState> {
        self.state.tasks.iter_mut().find(|t| t.config.name == name)
    }
}

fn setup_terminal() -> anyhow::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}
