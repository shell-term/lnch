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

    let inner_width = area.width.saturating_sub(2) as usize;
    let visible_height = area.height.saturating_sub(2) as usize;

    let total_visual_lines = compute_visual_line_count(&lines, inner_width);
    let max_scroll = total_visual_lines.saturating_sub(visible_height);
    max_scroll_out.set(max_scroll);
    let effective_scroll = scroll_offset.min(max_scroll).min(u16::MAX as usize);

    let block = Block::default()
        .title(format!(" Logs: [{}] ", task_name))
        .borders(Borders::ALL);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((effective_scroll as u16, 0));

    frame.render_widget(paragraph, area);
}

/// Approximate visual line count accounting for line wrapping.
///
/// Uses character-width-based estimation: each logical line occupies
/// `ceil(line_width / inner_width)` visual lines. This is a lower bound
/// relative to word-wrapping (which may insert extra breaks at word
/// boundaries), but is accurate for the vast majority of log output.
fn compute_visual_line_count(lines: &[Line], inner_width: usize) -> usize {
    if inner_width == 0 {
        return lines.len();
    }
    lines
        .iter()
        .map(|line| {
            let w = line.width();
            if w <= inner_width {
                1
            } else {
                (w + inner_width - 1) / inner_width
            }
        })
        .sum()
}
