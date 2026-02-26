use std::io;
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
            },
            process_cmd_tx,
            process_event_rx: Some(process_event_rx),
            app_event_rx: Some(app_event_rx),
            app_event_tx,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut terminal = setup_terminal()?;

        // Spawn keyboard/tick event reader
        let _event_reader = spawn_event_reader(self.app_event_tx.clone());

        // Forward process events to app event channel
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

        // Main event loop
        loop {
            terminal.draw(|frame| ui::render(frame, &self.state))?;

            match app_event_rx.recv().await {
                Some(AppEvent::Key(key)) => {
                    self.handle_key(key).await;
                }
                Some(AppEvent::Tick) => {
                    // periodic refresh is handled by the draw call
                }
                Some(AppEvent::Process(event)) => {
                    self.handle_process_event(event);
                }
                None => break,
            }

            if self.state.should_quit {
                let _ = self.process_cmd_tx.send(ProcessCommand::Shutdown).await;
                // Give some time for shutdown
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                break;
            }
        }

        restore_terminal(&mut terminal)?;
        Ok(())
    }

    async fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
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
        self.state.log_scroll_offset += 10;
        // If we've scrolled to the bottom, re-enable auto_scroll
        if let Some(task) = self.state.tasks.get(self.state.selected_index) {
            if self.state.log_scroll_offset >= task.log_buffer.len().saturating_sub(1) {
                self.state.auto_scroll = true;
            }
        }
    }

    fn snap_scroll_to_bottom(&mut self) {
        if let Some(task) = self.state.tasks.get(self.state.selected_index) {
            self.state.log_scroll_offset = task.log_buffer.len();
        }
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

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
