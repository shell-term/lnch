use std::cell::Cell;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::log::buffer::LogBuffer;

pub fn render_log_view(
    frame: &mut Frame,
    area: Rect,
    task_name: &str,
    log_buffer: &LogBuffer,
    scroll_offset: usize,
    max_scroll_out: &Cell<usize>,
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

    let visible_height = area.height.saturating_sub(2) as usize;

    let block = Block::default()
        .title(format!(" Logs: [{}] ", task_name))
        .borders(Borders::ALL);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    // Use inner width: Block borders consume 1 char each side, so text wraps to area.width - 2.
    // Using area.width would underestimate line count and prevent scrolling to the bottom.
    let inner_width = area.width.saturating_sub(2).max(1);
    let total_visual_lines = paragraph.line_count(inner_width);
    let max_scroll = total_visual_lines.saturating_sub(visible_height);
    max_scroll_out.set(max_scroll);
    let effective_scroll = scroll_offset.min(max_scroll).min(u16::MAX as usize);

    let paragraph = paragraph.scroll((effective_scroll as u16, 0));

    frame.render_widget(paragraph, area);
}
