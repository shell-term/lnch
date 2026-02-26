use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::log::buffer::LogBuffer;

pub fn render_log_view(
    frame: &mut Frame,
    area: Rect,
    task_name: &str,
    log_buffer: &LogBuffer,
    scroll_offset: usize,
) {
    let lines: Vec<Line> = log_buffer
        .lines()
        .iter()
        .map(|log_line| {
            let style = if log_line.is_stderr {
                Style::default().fg(Color::Red)
            } else {
                Style::default()
            };
            Line::styled(log_line.content.clone(), style)
        })
        .collect();

    let total_lines = lines.len();
    let visible_height = area.height.saturating_sub(2) as usize; // subtract borders

    // Auto-scroll: if offset would show beyond content, snap to bottom
    let max_scroll = total_lines.saturating_sub(visible_height);
    let effective_scroll = scroll_offset.min(max_scroll);

    let block = Block::default()
        .title(format!(" Logs: [{}] ", task_name))
        .borders(Borders::ALL);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((effective_scroll as u16, 0));

    frame.render_widget(paragraph, area);
}
