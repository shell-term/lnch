use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io;
use std::panic;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyCode, KeyModifiers, MouseButton, MouseEventKind,
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

use super::clipboard;
use super::event::{should_quit, spawn_event_reader};
use super::search::SearchState;
use super::selection::{ScreenPos, SelectionMode, SelectionState};
use super::ui;
use super::widgets::line_wrapper::WrappedContent;

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

/// Transient status bar feedback (e.g., "Reloaded!", "Reload failed: ...").
pub struct StatusFeedback {
    pub message: String,
    pub is_error: bool,
    pub expires_at: Instant,
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
    /// Text selection state machine.
    pub selection: SelectionState,
    /// Cached wrapped content from the last render, used by mouse handler
    /// to map screen coordinates to text positions.
    pub last_wrapped_content: RefCell<Option<WrappedContent>>,
    /// True while the user is dragging the scrollbar thumb.
    pub scrollbar_dragging: bool,
    /// Log search state.
    pub search: SearchState,
    /// Transient status bar feedback message.
    pub status_feedback: Option<StatusFeedback>,
}

pub struct App {
    state: AppState,
    process_cmd_tx: mpsc::Sender<ProcessCommand>,
    process_event_rx: Option<mpsc::Receiver<ProcessEvent>>,
    app_event_rx: Option<mpsc::Receiver<AppEvent>>,
    app_event_tx: mpsc::Sender<AppEvent>,
    update_on_exit: Option<UpdateInfo>,
    config_path: PathBuf,
}

impl App {
    pub fn new(
        config: &LnchConfig,
        config_path: PathBuf,
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
                selection: SelectionState::new(),
                last_wrapped_content: RefCell::new(None),
                scrollbar_dragging: false,
                search: SearchState::new(),
                status_feedback: None,
            },
            process_cmd_tx,
            process_event_rx: Some(process_event_rx),
            app_event_rx: Some(app_event_rx),
            app_event_tx,
            update_on_exit: None,
            config_path,
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

        // Spawn periodic update checker (immediate + every 1 hour)
        let update_tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            loop {
                if let Some(info) = crate::update::checker::check_for_update().await {
                    let _ = update_tx.send(AppEvent::UpdateAvailable(info)).await;
                }
                tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
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
            AppEvent::Tick => {
                self.state.selection.tick();
                if let Some(ref fb) = self.state.status_feedback {
                    if Instant::now() >= fb.expires_at {
                        self.state.status_feedback = None;
                    }
                }
            }
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

        // Handle search input mode — captures all keys except Esc/Enter
        if self.state.search.active {
            match key.code {
                KeyCode::Esc => {
                    self.state.search.cancel();
                }
                KeyCode::Enter => {
                    self.state.search.confirm();
                    self.jump_to_current_match();
                }
                KeyCode::Backspace => {
                    self.state.search.query.pop();
                    self.refresh_search();
                }
                KeyCode::Char(c) => {
                    self.state.search.query.push(c);
                    self.refresh_search();
                }
                _ => {}
            }
            return;
        }

        // Ctrl+C with active selection → copy instead of quit
        if key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.state.selection.is_selected()
        {
            self.copy_selection();
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

            // Search
            KeyCode::Char('/') => {
                self.state.search.activate();
            }
            KeyCode::Char('n') => {
                self.state.search.next_match();
                self.jump_to_current_match();
            }
            KeyCode::Char('N') => {
                self.state.search.prev_match();
                self.jump_to_current_match();
            }
            KeyCode::Esc => {
                if self.state.search.has_query() {
                    self.state.search.clear_highlights();
                }
            }

            // Config reload
            KeyCode::Char('R') => {
                self.reload_config().await;
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
            self.state.selection.clear();
            self.state.selected_index -= 1;
            self.reset_scroll();
            self.refresh_search();
        }
    }

    fn select_next_task(&mut self) {
        if self.state.selected_index + 1 < self.state.tasks.len() {
            self.state.selection.clear();
            self.state.selected_index += 1;
            self.reset_scroll();
            self.refresh_search();
        }
    }

    fn scroll_log_up_by(&mut self, lines: usize) {
        self.state.selection.clear();
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
        self.state.selection.clear();
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
        self.state.selection.clear();
        if let Some(task) = self.state.tasks.get_mut(self.state.selected_index) {
            task.log_buffer.clear();
            self.state.log_scroll_offset = 0;
            self.state.auto_scroll = true;
        }
        self.refresh_search();
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

        // Scrollbar: rightmost column of log area, between top/bottom borders
        let on_scrollbar = self.state.last_max_scroll.get() > 0
            && log_area.width > 0
            && col == log_area.x + log_area.width - 1
            && row > log_area.y
            && row < log_area.y + log_area.height.saturating_sub(1);

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
                if on_scrollbar {
                    self.state.scrollbar_dragging = true;
                    self.scroll_to_mouse_y(row);
                } else if in_log_area {
                    let mode = if mouse.modifiers.contains(KeyModifiers::ALT) {
                        SelectionMode::Block
                    } else {
                        SelectionMode::Normal
                    };
                    self.state.selection =
                        SelectionState::start_selecting(ScreenPos { col, row }, mode);
                } else if in_task_list {
                    // +1 to skip the border top row
                    let clicked_index = (row - task_list_area.y).saturating_sub(1) as usize;
                    self.select_task(clicked_index);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.state.scrollbar_dragging {
                    self.scroll_to_mouse_y(row);
                } else if let SelectionState::Selecting {
                    ref mut current, ..
                } = self.state.selection
                {
                    *current = ScreenPos { col, row };
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.state.scrollbar_dragging {
                    self.state.scrollbar_dragging = false;
                } else if let Some((anchor, current, _)) = self.state.selection.selecting_range() {
                    if anchor == current {
                        // Single click (no drag) — clear selection
                        self.state.selection.clear();
                    } else {
                        // Finalize selection, keep highlight visible
                        self.state.selection.finish_selecting();
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if self.state.selection.is_selected() {
                    self.copy_selection();
                }
            }
            _ => {}
        }
    }

    fn scroll_to_mouse_y(&mut self, row: u16) {
        let log_area = self.state.last_log_area.get();
        let max_scroll = self.state.last_max_scroll.get();
        if max_scroll == 0 {
            return;
        }

        // Track area: inner rows between top and bottom borders
        let track_start = log_area.y + 1;
        let track_end = log_area.y + log_area.height.saturating_sub(2);
        if track_start >= track_end {
            return;
        }

        let clamped_row = row.clamp(track_start, track_end);
        let track_height = (track_end - track_start) as f64;
        let ratio = (clamped_row - track_start) as f64 / track_height;
        let new_offset = (ratio * max_scroll as f64).round() as usize;

        self.state.selection.clear();
        self.state.log_scroll_offset = new_offset.min(max_scroll);
        self.state.auto_scroll = self.state.log_scroll_offset >= max_scroll;
    }

    fn copy_selection(&mut self) {
        let (anchor, current, mode) = match self.state.selection.selecting_range() {
            Some(v) => v,
            None => return,
        };

        let log_area = self.state.last_log_area.get();
        let content_x = log_area.x + 1; // left border
        let content_y = log_area.y + 1; // top border

        let wrapped_ref = self.state.last_wrapped_content.borrow();
        let wrapped = match wrapped_ref.as_ref() {
            Some(w) => w,
            None => return,
        };

        // Normalize anchor/current to reading order
        let (start, end) = if anchor.row < current.row
            || (anchor.row == current.row && anchor.col <= current.col)
        {
            (anchor, current)
        } else {
            (current, anchor)
        };

        let start_row = start.row.saturating_sub(content_y) as usize;
        let start_col = start.col.saturating_sub(content_x) as usize;
        let end_row = end.row.saturating_sub(content_y) as usize;
        let end_col = end.col.saturating_sub(content_x) as usize;

        let effective_scroll = self
            .state
            .log_scroll_offset
            .min(self.state.last_max_scroll.get());

        let text = match mode {
            SelectionMode::Normal => {
                let sp = wrapped.screen_to_text(start_row, start_col, effective_scroll);
                let ep = wrapped.screen_to_text(end_row, end_col, effective_scroll);
                match (sp, ep) {
                    (Some(s), Some(e)) => wrapped.extract_text(s, e),
                    _ => return,
                }
            }
            SelectionMode::Block => {
                let s_vl = wrapped.screen_to_text(start_row, 0, effective_scroll);
                let e_vl = wrapped.screen_to_text(end_row, 0, effective_scroll);
                match (s_vl, e_vl) {
                    (Some(s), Some(e)) => {
                        let (r_lo, r_hi) = if s.visual_line_index <= e.visual_line_index {
                            (s.visual_line_index, e.visual_line_index)
                        } else {
                            (e.visual_line_index, s.visual_line_index)
                        };
                        let (c_lo, c_hi) = if start_col <= end_col {
                            (start_col, end_col)
                        } else {
                            (end_col, start_col)
                        };
                        wrapped.extract_block_text(r_lo, r_hi, c_lo, c_hi)
                    }
                    _ => return,
                }
            }
        };

        drop(wrapped_ref);

        if !text.is_empty() {
            let _ = clipboard::copy_to_clipboard(&text);
            self.state.selection = SelectionState::copied();
        } else {
            self.state.selection.clear();
        }
    }

    /// Recompute search matches against the current task's log buffer.
    fn refresh_search(&mut self) {
        if !self.state.search.has_query() {
            return;
        }
        if let Some(task) = self.state.tasks.get(self.state.selected_index) {
            self.state.search.update_matches(task.log_buffer.lines());
        }
    }

    /// Scroll the log view so the current search match is visible.
    fn jump_to_current_match(&mut self) {
        let current_match = match self.state.search.current_match() {
            Some(m) => m.clone(),
            None => return,
        };

        // Find the visual line for this match by looking at the cached wrapped content.
        let wrapped_ref = self.state.last_wrapped_content.borrow();
        let wrapped = match wrapped_ref.as_ref() {
            Some(w) => w,
            None => return,
        };

        // Find the first visual line that contains this match.
        let target_vl = wrapped
            .visual_lines
            .iter()
            .position(|vl| {
                vl.logical_line_index == current_match.logical_line_index
                    && vl.byte_start <= current_match.byte_start
                    && vl.byte_end > current_match.byte_start
            })
            .unwrap_or(0);

        let log_area = self.state.last_log_area.get();
        let visible_height = log_area.height.saturating_sub(2) as usize;
        let max_scroll = wrapped.max_scroll;

        drop(wrapped_ref);

        // Center the match on screen if it's outside the visible area.
        let effective_scroll = self.state.log_scroll_offset.min(max_scroll);
        if target_vl < effective_scroll || target_vl >= effective_scroll + visible_height {
            let new_offset = target_vl.saturating_sub(visible_height / 2);
            self.state.log_scroll_offset = new_offset.min(max_scroll);
        }
        self.state.auto_scroll = false;
    }

    async fn reload_config(&mut self) {
        use crate::config::loader::{config_base_dir, load_config, resolve_working_dirs};
        use crate::config::validator::validate_config;
        use crate::process::dependency::DependencyGraph;

        // 1. Load config from disk
        let mut new_config = match load_config(&self.config_path) {
            Ok(c) => c,
            Err(e) => {
                self.state.status_feedback = Some(StatusFeedback {
                    message: format!("Reload failed: {}", e),
                    is_error: true,
                    expires_at: Instant::now() + Duration::from_secs(5),
                });
                return;
            }
        };

        // 2. Validate
        let base_dir = config_base_dir(&self.config_path);
        if let Err(e) = validate_config(&new_config, &base_dir) {
            self.state.status_feedback = Some(StatusFeedback {
                message: format!("Reload failed: {}", e),
                is_error: true,
                expires_at: Instant::now() + Duration::from_secs(5),
            });
            return;
        }

        // 3. Resolve working dirs
        resolve_working_dirs(&mut new_config, &base_dir);

        // 4. Validate dependency graph
        if let Err(e) = DependencyGraph::from_config(&new_config) {
            self.state.status_feedback = Some(StatusFeedback {
                message: format!("Reload failed: {}", e),
                is_error: true,
                expires_at: Instant::now() + Duration::from_secs(5),
            });
            return;
        }

        // 5. Compute diff for feedback
        let old_names: std::collections::HashSet<&str> = self
            .state
            .tasks
            .iter()
            .map(|t| t.config.name.as_str())
            .collect();
        let new_names: std::collections::HashSet<&str> =
            new_config.tasks.iter().map(|t| t.name.as_str()).collect();
        let added = new_names.difference(&old_names).count();
        let removed = old_names.difference(&new_names).count();
        let changed = new_config
            .tasks
            .iter()
            .filter(|nt| {
                self.state
                    .tasks
                    .iter()
                    .any(|ot| ot.config.name == nt.name && ot.config != **nt)
            })
            .count();

        if added == 0 && removed == 0 && changed == 0 {
            self.state.status_feedback = Some(StatusFeedback {
                message: "No config changes detected".to_string(),
                is_error: false,
                expires_at: Instant::now() + Duration::from_secs(3),
            });
            return;
        }

        // 6. Apply to App state
        self.apply_config_to_app_state(&new_config);

        // 7. Send to ProcessManager
        let _ = self
            .process_cmd_tx
            .send(ProcessCommand::Reload(new_config))
            .await;

        // 8. Show success feedback
        let mut parts = Vec::new();
        if added > 0 {
            parts.push(format!("{} added", added));
        }
        if removed > 0 {
            parts.push(format!("{} removed", removed));
        }
        if changed > 0 {
            parts.push(format!("{} changed", changed));
        }
        self.state.status_feedback = Some(StatusFeedback {
            message: format!("Reloaded: {}", parts.join(", ")),
            is_error: false,
            expires_at: Instant::now() + Duration::from_secs(3),
        });
    }

    fn apply_config_to_app_state(&mut self, new_config: &LnchConfig) {
        // Drain old tasks into a map for O(1) lookup, preserving LogBuffer (not Clone).
        let mut old_tasks: HashMap<String, TaskState> = self
            .state
            .tasks
            .drain(..)
            .map(|t| (t.config.name.clone(), t))
            .collect();

        let mut new_tasks = Vec::new();
        for new_task_config in &new_config.tasks {
            if let Some(old_task) = old_tasks.remove(&new_task_config.name) {
                if old_task.config == *new_task_config {
                    // Unchanged: preserve status + logs
                    new_tasks.push(old_task);
                } else {
                    // Changed: fresh state, ProcessManager will handle restart
                    new_tasks.push(TaskState {
                        config: new_task_config.clone(),
                        status: TaskStatus::Stopped,
                        log_buffer: LogBuffer::with_default_capacity(),
                    });
                }
            } else {
                // Added: new task in stopped state
                new_tasks.push(TaskState {
                    config: new_task_config.clone(),
                    status: TaskStatus::Stopped,
                    log_buffer: LogBuffer::with_default_capacity(),
                });
            }
        }
        // Remaining old_tasks entries are removed — dropped here.

        self.state.tasks = new_tasks;
        self.state.project_name = new_config.name.clone();

        // Fix selected_index if out of bounds
        if self.state.selected_index >= self.state.tasks.len() {
            self.state.selected_index = self.state.tasks.len().saturating_sub(1);
        }

        self.state.selection.clear();
        self.reset_scroll();
        self.refresh_search();
    }

    fn select_task(&mut self, index: usize) {
        if index < self.state.tasks.len() && index != self.state.selected_index {
            self.state.selection.clear();
            self.state.selected_index = index;
            *self.state.last_wrapped_content.borrow_mut() = None;
            self.reset_scroll();
            self.refresh_search();
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
