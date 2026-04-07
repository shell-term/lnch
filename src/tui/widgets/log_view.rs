use std::cell::{Cell, RefCell};

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::log::buffer::LogBuffer;
use crate::tui::search::SearchState;
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
    search: &SearchState,
) {
    let visible_height = area.height.saturating_sub(2) as usize;
    // inner_width: borders (2) + scrollbar track (1)
    let inner_width = area.width.saturating_sub(3).max(1) as usize;

    // Reuse cached wrapped content if log buffer and width haven't changed.
    let generation = log_buffer.generation();
    let mut cached = wrapped_content_out.borrow_mut().take();
    let is_cache_valid = cached.as_ref().is_some_and(|wc| {
        wc.cache_generation == generation && wc.cache_width == inner_width
    });
    let wrapped = if is_cache_valid {
        let mut wc = cached.take().unwrap();
        wc.max_scroll = wc.visual_lines.len().saturating_sub(visible_height);
        wc
    } else {
        wrap_log_lines(log_buffer.lines(), inner_width, visible_height, generation)
    };
    let max_scroll = wrapped.max_scroll;
    max_scroll_out.set(max_scroll);

    // Empty log hint
    if wrapped.visual_lines.is_empty() {
        let block = Block::default()
            .title(format!(" Logs: [{}] ", task_name))
            .borders(Borders::ALL);
        let hint = Paragraph::new(Line::from(Span::styled(
            "No output yet",
            Style::default().fg(Color::DarkGray),
        )))
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(hint, area);
        *wrapped_content_out.borrow_mut() = Some(wrapped);
        return;
    }

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

    // Collect search match highlight ranges for visible lines.
    let search_highlights = resolve_search_highlights(search, &wrapped, effective_scroll, visible_height);

    // Build visible lines with selection + search highlighting.
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
            let search_ranges: Vec<&SearchHighlight> = search_highlights
                .iter()
                .filter(|h| h.visual_line_index == abs_vl_idx)
                .collect();
            build_line_with_highlight(
                &vl.text, base_style, abs_vl_idx, &sel_info, inner_width, &search_ranges,
            )
        })
        .collect();

    // Build bottom title: search bar or position indicator
    let bottom_title = if search.active {
        let match_info = if search.matches.is_empty() {
            if search.query.is_empty() {
                String::new()
            } else {
                " [0/0] ".to_string()
            }
        } else {
            let idx = search.current_index.map(|i| i + 1).unwrap_or(0);
            format!(" [{}/{}] ", idx, search.matches.len())
        };
        Line::from(vec![
            Span::styled(
                format!(" /{}", search.query),
                Style::default().fg(Color::Yellow).bold(),
            ),
            Span::styled(
                "\u{2588}",
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(" "),
            Span::raw(match_info),
        ])
    } else if search.has_results() {
        let idx = search.current_index.map(|i| i + 1).unwrap_or(0);
        let match_info = format!(" [{}/{}] ", idx, search.matches.len());
        Line::from(vec![
            Span::styled(
                format!(" /{} ", search.query),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(match_info),
            Span::raw(position_text),
        ])
    } else {
        Line::from(position_text).right_aligned()
    };

    let block = Block::default()
        .title(format!(" Logs: [{}] ", task_name))
        .title_bottom(bottom_title)
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

// ---------------------------------------------------------------------------
// Search highlight helpers
// ---------------------------------------------------------------------------

struct SearchHighlight {
    visual_line_index: usize,
    byte_start: usize,
    byte_end: usize,
    is_current: bool,
}

fn resolve_search_highlights(
    search: &SearchState,
    wrapped: &WrappedContent,
    effective_scroll: usize,
    visible_height: usize,
) -> Vec<SearchHighlight> {
    if !search.has_query() || search.matches.is_empty() {
        return Vec::new();
    }

    let end_vl = (effective_scroll + visible_height).min(wrapped.visual_lines.len());
    let mut highlights = Vec::new();

    for (match_idx, m) in search.matches.iter().enumerate() {
        let is_current = search.current_index == Some(match_idx);

        // Find all visual lines that belong to this logical line and overlap the match.
        for (vl_idx, vl) in wrapped.visual_lines.iter().enumerate() {
            if vl_idx < effective_scroll {
                continue;
            }
            if vl_idx >= end_vl {
                break;
            }
            if vl.logical_line_index != m.logical_line_index {
                continue;
            }
            // Check overlap between match [m.byte_start, m.byte_end) and visual line [vl.byte_start, vl.byte_end)
            if m.byte_start >= vl.byte_end || m.byte_end <= vl.byte_start {
                continue;
            }
            let hi_start = m.byte_start.max(vl.byte_start) - vl.byte_start;
            let hi_end = m.byte_end.min(vl.byte_end) - vl.byte_start;
            if hi_start < hi_end {
                highlights.push(SearchHighlight {
                    visual_line_index: vl_idx,
                    byte_start: hi_start,
                    byte_end: hi_end,
                    is_current,
                });
            }
        }
    }

    highlights
}

// ---------------------------------------------------------------------------
// Line building with selection + search highlights
// ---------------------------------------------------------------------------

const SEARCH_MATCH_STYLE: Style = Style::new().fg(Color::Black).bg(Color::Yellow);
const SEARCH_CURRENT_STYLE: Style = Style::new().fg(Color::Black).bg(Color::LightRed);

fn build_line_with_highlight<'a>(
    text: &str,
    base_style: Style,
    abs_vl_idx: usize,
    sel_info: &SelectionInfo,
    _inner_width: usize,
    search_highlights: &[&SearchHighlight],
) -> Line<'a> {
    let sel_highlight_style = base_style.add_modifier(Modifier::REVERSED);

    // Determine selection byte range for this line.
    let sel_range = resolve_selection_range(text, abs_vl_idx, sel_info, _inner_width);

    // If selection is active on this line, selection takes priority over search.
    if let Some((sel_start, sel_end)) = sel_range {
        return split_spans(text, sel_start, sel_end, base_style, sel_highlight_style);
    }

    // Apply search highlights.
    if search_highlights.is_empty() {
        return Line::styled(text.to_string(), base_style);
    }

    build_search_highlighted_line(text, base_style, search_highlights)
}

/// Extract the selection byte range for a given visual line, if any.
fn resolve_selection_range(
    text: &str,
    abs_vl_idx: usize,
    sel_info: &SelectionInfo,
    _inner_width: usize,
) -> Option<(usize, usize)> {
    match sel_info {
        SelectionInfo::None => None,
        SelectionInfo::Normal {
            start_vl,
            start_byte,
            end_vl,
            end_byte,
        } => {
            if abs_vl_idx < *start_vl || abs_vl_idx > *end_vl {
                return None;
            }
            let (sel_start, sel_end) = if abs_vl_idx == *start_vl && abs_vl_idx == *end_vl {
                ((*start_byte).min(text.len()), (*end_byte).min(text.len()))
            } else if abs_vl_idx == *start_vl {
                ((*start_byte).min(text.len()), text.len())
            } else if abs_vl_idx == *end_vl {
                (0, (*end_byte).min(text.len()))
            } else {
                (0, text.len())
            };
            if sel_start == sel_end {
                None
            } else {
                Some((sel_start, sel_end))
            }
        }
        SelectionInfo::Block {
            start_vl,
            end_vl,
            col_lo,
            col_hi,
        } => {
            if abs_vl_idx < *start_vl || abs_vl_idx > *end_vl {
                return None;
            }
            let byte_lo = col_to_byte_offset(text, *col_lo);
            let byte_hi = col_to_byte_offset(text, *col_hi);
            if byte_lo == byte_hi {
                None
            } else {
                Some((byte_lo, byte_hi))
            }
        }
    }
}

/// Build a Line with multiple search highlights applied.
fn build_search_highlighted_line<'a>(
    text: &str,
    base_style: Style,
    highlights: &[&SearchHighlight],
) -> Line<'a> {
    // Merge overlapping highlights and sort by start position.
    let mut ranges: Vec<(usize, usize, bool)> = highlights
        .iter()
        .map(|h| (h.byte_start.min(text.len()), h.byte_end.min(text.len()), h.is_current))
        .filter(|(s, e, _)| s < e)
        .collect();
    ranges.sort_by_key(|r| r.0);

    let mut spans = Vec::new();
    let mut pos = 0;

    for (start, end, is_current) in &ranges {
        if *start > pos {
            spans.push(Span::styled(text[pos..*start].to_string(), base_style));
        }
        let style = if *is_current {
            SEARCH_CURRENT_STYLE
        } else {
            SEARCH_MATCH_STYLE
        };
        spans.push(Span::styled(text[*start..*end].to_string(), style));
        pos = *end;
    }

    if pos < text.len() {
        spans.push(Span::styled(text[pos..].to_string(), base_style));
    }

    Line::from(spans)
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
