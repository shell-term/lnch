use std::cell::{Cell, RefCell};

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::log::buffer::LogBuffer;
use crate::tui::selection::{SelectionMode, SelectionState};
use crate::tui::widgets::line_wrapper::{col_to_byte_offset, wrap_log_lines, WrappedContent};

pub fn render_log_view(
    frame: &mut Frame,
    area: Rect,
    task_name: &str,
    log_buffer: &LogBuffer,
    scroll_offset: usize,
    max_scroll_out: &Cell<usize>,
    selection: &SelectionState,
    wrapped_content_out: &RefCell<Option<WrappedContent>>,
) {
    let visible_height = area.height.saturating_sub(2) as usize;
    // inner_width: borders (2) + scrollbar track (1)
    let inner_width = area.width.saturating_sub(3).max(1) as usize;

    // Pre-compute visual lines.
    let wrapped = wrap_log_lines(log_buffer.lines(), inner_width, visible_height);
    let max_scroll = wrapped.max_scroll;
    max_scroll_out.set(max_scroll);

    let effective_scroll = scroll_offset.min(max_scroll);

    // Scroll position indicator
    let total_visual_lines = wrapped.visual_lines.len();
    let position_text = if total_visual_lines == 0 || max_scroll == 0 {
        String::new()
    } else {
        let top = effective_scroll + 1;
        let bottom = (effective_scroll + visible_height).min(total_visual_lines);
        format!(" {}-{}/{} ", top, bottom, total_visual_lines)
    };

    // Resolve selection range for highlighting.
    let sel_info = resolve_selection(selection, &wrapped, area, effective_scroll);

    // Build visible lines with selection highlighting.
    let end = (effective_scroll + visible_height).min(total_visual_lines);
    let visible_visual_lines = &wrapped.visual_lines[effective_scroll..end];

    let lines: Vec<Line> = visible_visual_lines
        .iter()
        .enumerate()
        .map(|(screen_row, vl)| {
            let base_style = if vl.is_stderr {
                Style::default().fg(Color::Red)
            } else {
                Style::default()
            };
            let abs_vl_idx = effective_scroll + screen_row;
            build_line_with_highlight(&vl.text, base_style, abs_vl_idx, &sel_info, inner_width)
        })
        .collect();

    let block = Block::default()
        .title(format!(" Logs: [{}] ", task_name))
        .title_bottom(Line::from(position_text).right_aligned())
        .borders(Borders::ALL);

    // No .wrap() and no .scroll() — lines are already pre-wrapped and sliced.
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);

    // Render scrollbar
    if max_scroll > 0 {
        let mut scrollbar_state = ScrollbarState::new(max_scroll).position(effective_scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }

    // Cache wrapped content for mouse handler.
    *wrapped_content_out.borrow_mut() = Some(wrapped);
}

// ---------------------------------------------------------------------------
// Selection highlight helpers
// ---------------------------------------------------------------------------

/// Resolved selection info for rendering.
enum SelectionInfo {
    None,
    Normal {
        start_vl: usize,
        start_byte: usize,
        end_vl: usize,
        end_byte: usize,
    },
    Block {
        start_vl: usize,
        end_vl: usize,
        col_lo: usize,
        col_hi: usize,
    },
}

fn resolve_selection(
    selection: &SelectionState,
    wrapped: &WrappedContent,
    area: Rect,
    effective_scroll: usize,
) -> SelectionInfo {
    let (start, end, mode) = match selection.normalized_range() {
        Some(v) => v,
        None => return SelectionInfo::None,
    };

    // Convert absolute screen coords to content-area-relative coords.
    let content_x = area.x + 1; // left border
    let content_y = area.y + 1; // top border

    let start_row = start.row.saturating_sub(content_y) as usize;
    let start_col = start.col.saturating_sub(content_x) as usize;
    let end_row = end.row.saturating_sub(content_y) as usize;
    let end_col = end.col.saturating_sub(content_x) as usize;

    match mode {
        SelectionMode::Normal => {
            let sp = wrapped.screen_to_text(start_row, start_col, effective_scroll);
            let ep = wrapped.screen_to_text(end_row, end_col, effective_scroll);
            match (sp, ep) {
                (Some(s), Some(e)) => SelectionInfo::Normal {
                    start_vl: s.visual_line_index,
                    start_byte: s.byte_offset,
                    end_vl: e.visual_line_index,
                    end_byte: e.byte_offset,
                },
                _ => SelectionInfo::None,
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
                    SelectionInfo::Block {
                        start_vl: r_lo,
                        end_vl: r_hi,
                        col_lo: c_lo,
                        col_hi: c_hi,
                    }
                }
                _ => SelectionInfo::None,
            }
        }
    }
}

fn build_line_with_highlight<'a>(
    text: &str,
    base_style: Style,
    abs_vl_idx: usize,
    sel_info: &SelectionInfo,
    _inner_width: usize,
) -> Line<'a> {
    let highlight_style = base_style.add_modifier(Modifier::REVERSED);

    match sel_info {
        SelectionInfo::None => Line::styled(text.to_string(), base_style),

        SelectionInfo::Normal {
            start_vl,
            start_byte,
            end_vl,
            end_byte,
        } => {
            if abs_vl_idx < *start_vl || abs_vl_idx > *end_vl {
                return Line::styled(text.to_string(), base_style);
            }

            let (sel_start, sel_end) = if abs_vl_idx == *start_vl && abs_vl_idx == *end_vl {
                (
                    (*start_byte).min(text.len()),
                    (*end_byte).min(text.len()),
                )
            } else if abs_vl_idx == *start_vl {
                ((*start_byte).min(text.len()), text.len())
            } else if abs_vl_idx == *end_vl {
                (0, (*end_byte).min(text.len()))
            } else {
                // Middle line — fully selected
                (0, text.len())
            };

            if sel_start == sel_end {
                return Line::styled(text.to_string(), base_style);
            }

            split_spans(text, sel_start, sel_end, base_style, highlight_style)
        }

        SelectionInfo::Block {
            start_vl,
            end_vl,
            col_lo,
            col_hi,
        } => {
            if abs_vl_idx < *start_vl || abs_vl_idx > *end_vl {
                return Line::styled(text.to_string(), base_style);
            }

            let byte_lo = col_to_byte_offset(text, *col_lo);
            let byte_hi = col_to_byte_offset(text, *col_hi);

            if byte_lo == byte_hi {
                return Line::styled(text.to_string(), base_style);
            }

            split_spans(text, byte_lo, byte_hi, base_style, highlight_style)
        }
    }
}

fn split_spans<'a>(
    text: &str,
    sel_start: usize,
    sel_end: usize,
    base_style: Style,
    highlight_style: Style,
) -> Line<'a> {
    let mut spans = Vec::with_capacity(3);
    if sel_start > 0 {
        spans.push(Span::styled(text[..sel_start].to_string(), base_style));
    }
    spans.push(Span::styled(
        text[sel_start..sel_end].to_string(),
        highlight_style,
    ));
    if sel_end < text.len() {
        spans.push(Span::styled(text[sel_end..].to_string(), base_style));
    }
    Line::from(spans)
}
