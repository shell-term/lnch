use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::message::TaskStatus;

pub struct TaskListData {
    pub name: String,
    pub status: TaskStatus,
    pub color: Color,
}

fn status_icon(status: &TaskStatus) -> (&str, Color) {
    match status {
        TaskStatus::Running => ("●", Color::Green),
        TaskStatus::Stopped => ("○", Color::DarkGray),
        TaskStatus::Starting => ("◉", Color::Yellow),
        TaskStatus::Stopping => ("◉", Color::Yellow),
        TaskStatus::Failed { .. } => ("✕", Color::Red),
    }
}

pub fn render_task_list(
    frame: &mut Frame,
    area: Rect,
    tasks: &[TaskListData],
    selected_index: usize,
) {
    let items: Vec<ListItem> = tasks
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let (icon, icon_color) = status_icon(&task.status);
            let prefix = if i == selected_index { "> " } else { "  " };
            let line = Line::from(vec![
                Span::raw(prefix),
                Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                Span::styled(task.name.clone(), Style::default().fg(task.color)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::default().title(" Tasks ").borders(Borders::ALL);

    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::DarkGray));

    frame.render_stateful_widget(list, area, &mut list_state);
}
