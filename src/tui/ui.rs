use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use super::app::AppState;
use super::widgets::log_view::render_log_view;
use super::widgets::status_bar::render_status_bar;
use super::widgets::task_list::{render_task_list, TaskListData};

const AUTO_COLOR_CYCLE: &[Color] = &[
    Color::Green,
    Color::Blue,
    Color::Yellow,
    Color::Magenta,
    Color::Cyan,
    Color::Red,
    Color::White,
];

fn color_from_name(name: &str) -> Color {
    match name {
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        _ => Color::White,
    }
}

pub fn render(frame: &mut Frame, state: &AppState) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());

    // Title bar
    let title_style = Style::default()
        .fg(Color::White)
        .bg(Color::Blue)
        .add_modifier(Modifier::BOLD);

    if let Some(update) = &state.update_info {
        let update_text = format!(" v{} available [u] Update ", update.latest_version);
        let update_width = update_text.len() as u16;
        let title_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(update_width)])
            .split(root[0]);

        let title = Paragraph::new(format!(" lnch: {} ", state.project_name)).style(title_style);
        frame.render_widget(title, title_layout[0]);

        let update_label = Paragraph::new(update_text)
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Right);
        frame.render_widget(update_label, title_layout[1]);
    } else {
        let title =
            Paragraph::new(format!(" lnch: {} ", state.project_name)).style(title_style);
        frame.render_widget(title, root[0]);
    }

    // Main content area
    let main_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(root[1]);

    // Task list
    let task_list_data: Vec<TaskListData> = state
        .tasks
        .iter()
        .enumerate()
        .map(|(i, ts)| TaskListData {
            name: ts.config.name.clone(),
            status: ts.status.clone(),
            color: ts
                .config
                .color
                .as_ref()
                .map(|c| color_from_name(c))
                .unwrap_or(AUTO_COLOR_CYCLE[i % AUTO_COLOR_CYCLE.len()]),
        })
        .collect();

    state.last_task_list_area.set(main_area[0]);
    state.last_log_area.set(main_area[1]);

    render_task_list(frame, main_area[0], &task_list_data, state.selected_index);

    // Log view
    if let Some(selected_task) = state.tasks.get(state.selected_index) {
        render_log_view(
            frame,
            main_area[1],
            &selected_task.config.name,
            &selected_task.log_buffer,
            state.log_scroll_offset,
            &state.last_max_scroll,
            &state.selection,
            &state.last_wrapped_content,
            &state.search,
        );
    } else {
        state.last_max_scroll.set(0);
        let block = Block::default().title(" Logs ").borders(Borders::ALL);
        frame.render_widget(block, main_area[1]);
    }

    // Status bar
    render_status_bar(
        frame,
        root[2],
        state.update_info.is_some(),
        state.confirm_quit,
        state.selection.show_copied(),
        state.selection.is_selected(),
    );
}
