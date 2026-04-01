use std::cell::Cell;
use std::io;
use std::panic;
use std::time::Instant;

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyCode, MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use tokio::sync::mpsc;

use crate::config::model::{LnchConfig, TaskConfig};
use crate::log::buffer::{LogBuffer, LogLine};
use crate::message::{AppEvent, ProcessCommand, ProcessEvent, TaskStatus};
use crate::update::checker::UpdateInfo;

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
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
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
    pub confirm_quit: bool,
    pub auto_scroll: bool,
    /// Updated by `render_log_view` each frame via interior mutability,
    /// so scroll logic can compare against the true visual-line max.
    pub last_max_scroll: Cell<usize>,
    pub update_info: Option<UpdateInfo>,
    /// Updated by `render` each frame so mouse handler knows widget areas.
    pub last_task_list_area: Cell<Rect>,
    pub last_log_area: Cell<Rect>,
}

pub struct App {
    state: AppState,
    process_cmd_tx: mpsc::Sender<ProcessCommand>,
    process_event_rx: Option<mpsc::Receiver<ProcessEvent>>,
    app_event_rx: Option<mpsc::Receiver<AppEvent>>,
    app_event_tx: mpsc::Sender<AppEvent>,
    update_on_exit: Option<UpdateInfo>,
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
                confirm_quit: false,
                auto_scroll: true,
                last_max_scroll: Cell::new(0),
                update_info: None,
                last_task_list_area: Cell::new(Rect::default()),
                last_log_area: Cell::new(Rect::default()),
            },
            process_cmd_tx,
            process_event_rx: Some(process_event_rx),
            app_event_rx: Some(app_event_rx),
            app_event_tx,
            update_on_exit: None,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut terminal = TerminalGuard::new(setup_terminal()?);

        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
            prev_hook(info);
        }));

        let result = self.event_loop(&mut terminal).await;

        // Drop the guard first (restores terminal), then propagate any error
        drop(terminal);

        // Execute update if the user requested it
        if let Some(info) = self.update_on_exit.take() {
            crate::update::checker::execute_update(&info);
        }

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

        // Spawn background update checker
        let update_tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            if let Some(info) = crate::update::checker::check_for_update().await {
                let _ = update_tx.send(AppEvent::UpdateAvailable(info)).await;
            }
        });

        let mut app_event_rx = self.app_event_rx.take().unwrap();

        loop {
            terminal.draw(|frame| ui::render(frame, &self.state))?;

            // Wait for at least one event
            match app_event_rx.recv().await {
                Some(event) => self.handle_app_event(event).await,
                None => {
                    tracing::warn!("App event channel closed, exiting");
                    break;
                }
            }

            // Drain all pending events before redrawing
            while let Ok(event) = app_event_rx.try_recv() {
                self.handle_app_event(event).await;
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

    async fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Key(key) => {
                tracing::debug!(key = ?key, "Key event received");
                self.handle_key(key).await;
            }
            AppEvent::Mouse(mouse) => {
                self.handle_mouse(mouse);
            }
            AppEvent::Tick => {}
            AppEvent::Process(event) => {
                self.handle_process_event(event);
            }
            AppEvent::UpdateAvailable(info) => {
                self.state.update_info = Some(info);
            }
        }
    }

    async fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if key.kind != crossterm::event::KeyEventKind::Press {
            return;
        }

        // Handle quit confirmation mode
        if self.state.confirm_quit {
            match key.code {
                KeyCode::Char('y') => {
                    self.state.should_quit = true;
                }
                _ => {
                    self.state.confirm_quit = false;
                }
            }
            return;
        }

        if should_quit(&key) {
            if self.has_running_tasks() {
                self.state.confirm_quit = true;
            } else {
                self.state.should_quit = true;
            }
            return;
        }

        match key.code {
            KeyCode::Up => self.select_previous_task(),
            KeyCode::Down => self.select_next_task(),

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
                let _ = self
                    .process_cmd_tx
                    .send(ProcessCommand::Restart(name))
                    .await;
            }

            KeyCode::Char('k') => self.scroll_log_up_by(10),
            KeyCode::Char('j') => self.scroll_log_down_by(10),
            KeyCode::Home => {
                self.state.log_scroll_offset = 0;
                self.state.auto_scroll = false;
            }
            KeyCode::End => {
                self.state.auto_scroll = true;
                self.snap_scroll_to_bottom();
            }

            KeyCode::Char('c') => self.clear_selected_log(),

            KeyCode::Char('u') => {
                if let Some(info) = self.state.update_info.clone() {
                    self.update_on_exit = Some(info);
                    self.state.should_quit = true;
                }
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

    fn scroll_log_up_by(&mut self, lines: usize) {
        self.state.auto_scroll = false;
        // Normalize sentinel (usize::MAX) to the actual max before subtracting,
        // otherwise the arithmetic has no visible effect.
        let current = self
            .state
            .log_scroll_offset
            .min(self.state.last_max_scroll.get());
        self.state.log_scroll_offset = current.saturating_sub(lines);
    }

    fn scroll_log_down_by(&mut self, lines: usize) {
        // Same normalization for consistency.
        let current = self
            .state
            .log_scroll_offset
            .min(self.state.last_max_scroll.get());
        self.state.log_scroll_offset = current.saturating_add(lines);
        if self.state.log_scroll_offset >= self.state.last_max_scroll.get() {
            self.state.auto_scroll = true;
            self.snap_scroll_to_bottom();
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

    fn clear_selected_log(&mut self) {
        if let Some(task) = self.state.tasks.get_mut(self.state.selected_index) {
            task.log_buffer.clear();
            self.state.log_scroll_offset = 0;
            self.state.auto_scroll = true;
        }
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

    fn has_running_tasks(&self) -> bool {
        self.state
            .tasks
            .iter()
            .any(|t| matches!(t.status, TaskStatus::Running | TaskStatus::Starting))
    }

    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        if self.state.confirm_quit {
            return;
        }

        let task_list_area = self.state.last_task_list_area.get();
        let log_area = self.state.last_log_area.get();
        let col = mouse.column;
        let row = mouse.row;

        let in_task_list = col >= task_list_area.x
            && col < task_list_area.x + task_list_area.width
            && row >= task_list_area.y
            && row < task_list_area.y + task_list_area.height;

        let in_log_area = col >= log_area.x
            && col < log_area.x + log_area.width
            && row >= log_area.y
            && row < log_area.y + log_area.height;

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if in_log_area || in_task_list {
                    self.scroll_log_up_by(3);
                }
            }
            MouseEventKind::ScrollDown => {
                if in_log_area || in_task_list {
                    self.scroll_log_down_by(3);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if in_task_list {
                    // +1 to skip the border top row
                    let clicked_index = (row - task_list_area.y).saturating_sub(1) as usize;
                    self.select_task(clicked_index);
                }
            }
            _ => {}
        }
    }

    fn select_task(&mut self, index: usize) {
        if index < self.state.tasks.len() && index != self.state.selected_index {
            self.state.selected_index = index;
            self.reset_scroll();
        }
    }

    fn find_task_mut(&mut self, name: &str) -> Option<&mut TaskState> {
        self.state.tasks.iter_mut().find(|t| t.config.name == name)
    }
}

fn setup_terminal() -> anyhow::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}
