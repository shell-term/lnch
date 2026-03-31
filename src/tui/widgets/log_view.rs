use std::cell::Cell;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};

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

    // Build scroll position indicator for the title
    // Use inner width for line count calculation (borders consume 1 char each side,
    // plus 1 for the scrollbar track).
    let inner_width = area.width.saturating_sub(3).max(1);

    // Build a temporary paragraph to calculate total lines
    let temp_paragraph = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
    let total_visual_lines = temp_paragraph.line_count(inner_width);
    let max_scroll = total_visual_lines.saturating_sub(visible_height);
    max_scroll_out.set(max_scroll);
    let effective_scroll = scroll_offset.min(max_scroll).min(u16::MAX as usize);

    // Scroll position indicator: show current line range / total
    let position_text = if total_visual_lines == 0 {
        String::new()
    } else if max_scroll == 0 {
        // All content fits in the view
        String::new()
    } else {
        let top = effective_scroll + 1;
        let bottom = (effective_scroll + visible_height).min(total_visual_lines);
        format!(" {}-{}/{} ", top, bottom, total_visual_lines)
    };

    let block = Block::default()
        .title(format!(" Logs: [{}] ", task_name))
        .title_bottom(Line::from(position_text).right_aligned())
        .borders(Borders::ALL);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((effective_scroll as u16, 0));

    frame.render_widget(paragraph, area);

    // Render scrollbar
    if max_scroll > 0 {
        let mut scrollbar_state = ScrollbarState::new(max_scroll).position(effective_scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        // Render inside the block border (1-cell inset from the area)
        let scrollbar_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height,
        };
        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}
