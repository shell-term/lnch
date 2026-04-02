use std::collections::VecDeque;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::log::buffer::LogLine;

/// One visual (screen) line produced by wrapping a logical line.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VisualLine {
    /// Index into the original LogBuffer lines.
    pub logical_line_index: usize,
    /// Byte offset (inclusive) into the logical line where this visual line starts.
    pub byte_start: usize,
    /// Byte offset (exclusive) into the logical line where this visual line ends.
    pub byte_end: usize,
    /// The text content of this visual line.
    pub text: String,
    /// Whether the source line was stderr.
    pub is_stderr: bool,
}

/// A position within the wrapped content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPosition {
    pub visual_line_index: usize,
    pub byte_offset: usize,
}

/// Complete pre-wrapped content with visual-line mapping.
pub struct WrappedContent {
    pub visual_lines: Vec<VisualLine>,
    pub max_scroll: usize,
}

// ---------------------------------------------------------------------------
// Wrapping
// ---------------------------------------------------------------------------

/// Wrap all log lines into visual lines at `max_width` display columns.
/// `visible_height` is used only to compute `max_scroll`.
pub fn wrap_log_lines(
    lines: &VecDeque<LogLine>,
    max_width: usize,
    visible_height: usize,
) -> WrappedContent {
    let max_width = max_width.max(1);
    let mut visual_lines = Vec::new();

    for (logical_idx, log_line) in lines.iter().enumerate() {
        wrap_single_line(
            &log_line.content,
            log_line.is_stderr,
            logical_idx,
            max_width,
            &mut visual_lines,
        );
    }

    let total = visual_lines.len();
    let max_scroll = total.saturating_sub(visible_height);

    WrappedContent {
        visual_lines,
        max_scroll,
    }
}

/// Wrap a single logical line into one or more visual lines.
/// Matches ratatui `Wrap { trim: false }` behavior:
///   - break at word boundaries (spaces)
///   - if a word is wider than max_width, break at character level
///   - preserve all whitespace (trim: false)
///   - empty lines produce exactly one visual line
fn wrap_single_line(
    content: &str,
    is_stderr: bool,
    logical_idx: usize,
    max_width: usize,
    out: &mut Vec<VisualLine>,
) {
    if content.is_empty() {
        out.push(VisualLine {
            logical_line_index: logical_idx,
            byte_start: 0,
            byte_end: 0,
            text: String::new(),
            is_stderr,
        });
        return;
    }

    let mut line_start_byte = 0; // byte offset of the current visual line start
    let mut current_text = String::new();
    let mut current_width: usize = 0;

    // Track the last breakable position (after a space).
    let mut last_break_text_len: Option<usize> = None;
    let mut last_break_width: Option<usize> = None;
    let mut last_break_byte: Option<usize> = None; // byte offset in content after the space

    for (byte_idx, grapheme) in content.grapheme_indices(true) {
        let g_width = UnicodeWidthStr::width(grapheme);

        if current_width + g_width > max_width && current_width > 0 {
            // Need to wrap.
            if let (Some(brk_tlen), Some(brk_width), Some(brk_byte)) =
                (last_break_text_len, last_break_width, last_break_byte)
            {
                // Break at the last word boundary.
                let emit_text: String = current_text[..brk_tlen].to_string();
                out.push(VisualLine {
                    logical_line_index: logical_idx,
                    byte_start: line_start_byte,
                    byte_end: brk_byte,
                    text: emit_text,
                    is_stderr,
                });
                // Start new line from the remainder.
                let remainder = current_text[brk_tlen..].to_string();
                current_width = current_width - brk_width;
                line_start_byte = brk_byte;
                current_text = remainder;
                last_break_text_len = None;
                last_break_width = None;
                last_break_byte = None;
            } else {
                // No word boundary found — break at character level.
                let emit_text = current_text.clone();
                out.push(VisualLine {
                    logical_line_index: logical_idx,
                    byte_start: line_start_byte,
                    byte_end: byte_idx,
                    text: emit_text,
                    is_stderr,
                });
                current_text = String::new();
                current_width = 0;
                line_start_byte = byte_idx;
                last_break_text_len = None;
                last_break_width = None;
                last_break_byte = None;
            }
        }

        current_text.push_str(grapheme);
        current_width += g_width;

        // Record break opportunity after a space grapheme.
        if grapheme == " " {
            last_break_text_len = Some(current_text.len());
            last_break_width = Some(current_width);
            last_break_byte = Some(byte_idx + grapheme.len());
        }
    }

    // Emit the remaining content as the last visual line.
    let byte_end = content.len();
    out.push(VisualLine {
        logical_line_index: logical_idx,
        byte_start: line_start_byte,
        byte_end,
        text: current_text,
        is_stderr,
    });
}

// ---------------------------------------------------------------------------
// Position mapping
// ---------------------------------------------------------------------------

impl WrappedContent {
    /// Convert a screen position (relative to content area, 0-based) to a
    /// `TextPosition`. Returns `None` only if there are no visual lines at all.
    pub fn screen_to_text(
        &self,
        screen_row: usize,
        screen_col: usize,
        scroll_offset: usize,
    ) -> Option<TextPosition> {
        if self.visual_lines.is_empty() {
            return None;
        }
        let vl_idx = (scroll_offset + screen_row).min(self.visual_lines.len() - 1);
        let vl = &self.visual_lines[vl_idx];
        let byte_offset = col_to_byte_offset(&vl.text, screen_col);
        Some(TextPosition {
            visual_line_index: vl_idx,
            byte_offset,
        })
    }

    /// Extract text for a **normal** (line-based) selection.
    /// `start` must come before `end` in reading order.
    pub fn extract_text(&self, start: TextPosition, end: TextPosition) -> String {
        if self.visual_lines.is_empty() {
            return String::new();
        }
        let start_vl = start.visual_line_index.min(self.visual_lines.len() - 1);
        let end_vl = end.visual_line_index.min(self.visual_lines.len() - 1);

        if start_vl == end_vl {
            let vl = &self.visual_lines[start_vl];
            let s = start.byte_offset.min(vl.text.len());
            let e = end.byte_offset.min(vl.text.len());
            return vl.text[s..e].to_string();
        }

        let mut result = String::new();

        for idx in start_vl..=end_vl {
            let vl = &self.visual_lines[idx];

            // Insert separator: newline between different logical lines,
            // nothing between wrapped continuations of the same logical line.
            if idx > start_vl {
                let prev = &self.visual_lines[idx - 1];
                if vl.logical_line_index != prev.logical_line_index {
                    result.push('\n');
                }
            }

            if idx == start_vl {
                let s = start.byte_offset.min(vl.text.len());
                result.push_str(&vl.text[s..]);
            } else if idx == end_vl {
                let e = end.byte_offset.min(vl.text.len());
                result.push_str(&vl.text[..e]);
            } else {
                result.push_str(&vl.text);
            }
        }

        result
    }

    /// Extract text for a **block** (rectangular) selection.
    /// `start_col` and `end_col` are display columns (0-based).
    pub fn extract_block_text(
        &self,
        start_vl: usize,
        end_vl: usize,
        start_col: usize,
        end_col: usize,
    ) -> String {
        if self.visual_lines.is_empty() {
            return String::new();
        }
        let (c_lo, c_hi) = if start_col <= end_col {
            (start_col, end_col)
        } else {
            (end_col, start_col)
        };
        let r_lo = start_vl.min(end_vl).min(self.visual_lines.len() - 1);
        let r_hi = start_vl.max(end_vl).min(self.visual_lines.len() - 1);

        let mut parts: Vec<String> = Vec::new();

        for idx in r_lo..=r_hi {
            let vl = &self.visual_lines[idx];
            let byte_lo = col_to_byte_offset(&vl.text, c_lo);
            let byte_hi = col_to_byte_offset(&vl.text, c_hi);
            parts.push(vl.text[byte_lo..byte_hi].to_string());
        }

        parts.join("\n")
    }
}

/// Convert a display column (0-based) to a byte offset within `text`.
/// Accounts for double-width characters via `unicode-width`.
pub fn col_to_byte_offset(text: &str, display_col: usize) -> usize {
    let mut col: usize = 0;
    for (byte_idx, grapheme) in text.grapheme_indices(true) {
        if col >= display_col {
            return byte_idx;
        }
        col += UnicodeWidthStr::width(grapheme);
    }
    // display_col is at or beyond the end of the text.
    text.len()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn make_lines(contents: &[&str]) -> VecDeque<LogLine> {
        contents
            .iter()
            .map(|s| LogLine {
                content: s.to_string(),
                is_stderr: false,
                timestamp: Instant::now(),
            })
            .collect()
    }

    fn make_lines_with_stderr(contents: &[(&str, bool)]) -> VecDeque<LogLine> {
        contents
            .iter()
            .map(|(s, stderr)| LogLine {
                content: s.to_string(),
                is_stderr: *stderr,
                timestamp: Instant::now(),
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // wrap_log_lines tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_no_wrap_short_line() {
        let lines = make_lines(&["hello"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        assert_eq!(wc.visual_lines.len(), 1);
        assert_eq!(wc.visual_lines[0].text, "hello");
        assert_eq!(wc.visual_lines[0].byte_start, 0);
        assert_eq!(wc.visual_lines[0].byte_end, 5);
        assert_eq!(wc.visual_lines[0].logical_line_index, 0);
    }

    #[test]
    fn test_word_wrap_at_space() {
        // "hello world" with width 6 → "hello " (6) and "world" (5)
        let lines = make_lines(&["hello world"]);
        let wc = wrap_log_lines(&lines, 6, 10);
        assert_eq!(wc.visual_lines.len(), 2);
        assert_eq!(wc.visual_lines[0].text, "hello ");
        assert_eq!(wc.visual_lines[0].byte_start, 0);
        assert_eq!(wc.visual_lines[0].byte_end, 6);
        assert_eq!(wc.visual_lines[1].text, "world");
        assert_eq!(wc.visual_lines[1].byte_start, 6);
        assert_eq!(wc.visual_lines[1].byte_end, 11);
    }

    #[test]
    fn test_char_wrap_long_word() {
        // "abcdefgh" with width 3 → "abc", "def", "gh"
        let lines = make_lines(&["abcdefgh"]);
        let wc = wrap_log_lines(&lines, 3, 10);
        assert_eq!(wc.visual_lines.len(), 3);
        assert_eq!(wc.visual_lines[0].text, "abc");
        assert_eq!(wc.visual_lines[1].text, "def");
        assert_eq!(wc.visual_lines[2].text, "gh");
    }

    #[test]
    fn test_empty_line() {
        let lines = make_lines(&[""]);
        let wc = wrap_log_lines(&lines, 20, 10);
        assert_eq!(wc.visual_lines.len(), 1);
        assert_eq!(wc.visual_lines[0].text, "");
        assert_eq!(wc.visual_lines[0].byte_start, 0);
        assert_eq!(wc.visual_lines[0].byte_end, 0);
    }

    #[test]
    fn test_multiple_lines() {
        let lines = make_lines(&["aaa", "bbb"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        assert_eq!(wc.visual_lines.len(), 2);
        assert_eq!(wc.visual_lines[0].logical_line_index, 0);
        assert_eq!(wc.visual_lines[0].text, "aaa");
        assert_eq!(wc.visual_lines[1].logical_line_index, 1);
        assert_eq!(wc.visual_lines[1].text, "bbb");
    }

    #[test]
    fn test_exact_width_line() {
        let lines = make_lines(&["abcde"]);
        let wc = wrap_log_lines(&lines, 5, 10);
        assert_eq!(wc.visual_lines.len(), 1);
        assert_eq!(wc.visual_lines[0].text, "abcde");
    }

    #[test]
    fn test_trailing_space_at_boundary() {
        // "abc " with width 4 → fits in one line (width 4)
        let lines = make_lines(&["abc "]);
        let wc = wrap_log_lines(&lines, 4, 10);
        assert_eq!(wc.visual_lines.len(), 1);
        assert_eq!(wc.visual_lines[0].text, "abc ");
    }

    #[test]
    fn test_cjk_double_width() {
        // Each CJK char is 2 columns. "あいう" = 6 columns. Width 5 → "あい"(4), "う"(2)
        let lines = make_lines(&["あいう"]);
        let wc = wrap_log_lines(&lines, 5, 10);
        assert_eq!(wc.visual_lines.len(), 2);
        assert_eq!(wc.visual_lines[0].text, "あい");
        assert_eq!(wc.visual_lines[1].text, "う");
    }

    #[test]
    fn test_cjk_at_boundary() {
        // Width 3: CJK char (width 2) + another CJK char would be 4 → wrap.
        // "あい" (4 cols) at width 3 → "あ"(2), "い"(2)
        let lines = make_lines(&["あい"]);
        let wc = wrap_log_lines(&lines, 3, 10);
        assert_eq!(wc.visual_lines.len(), 2);
        assert_eq!(wc.visual_lines[0].text, "あ");
        assert_eq!(wc.visual_lines[1].text, "い");
    }

    #[test]
    fn test_mixed_ascii_cjk() {
        // "aあb" = a(1) + あ(2) + b(1) = 4 cols. Width 3 → "aあ" won't fit (3), hmm:
        // a=1, あ=2 → 1+2=3 fits in width 3. Then b=1 → total 4 > 3, wrap.
        // So: "aあ"(3), "b"(1)
        let lines = make_lines(&["aあb"]);
        let wc = wrap_log_lines(&lines, 3, 10);
        assert_eq!(wc.visual_lines.len(), 2);
        assert_eq!(wc.visual_lines[0].text, "aあ");
        assert_eq!(wc.visual_lines[1].text, "b");
    }

    #[test]
    fn test_max_scroll_calculation() {
        let lines = make_lines(&["a", "b", "c", "d", "e"]);
        let wc = wrap_log_lines(&lines, 20, 3);
        // 5 visual lines, visible 3 → max_scroll = 2
        assert_eq!(wc.max_scroll, 2);
    }

    #[test]
    fn test_max_scroll_no_overflow() {
        let lines = make_lines(&["a", "b"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        assert_eq!(wc.max_scroll, 0);
    }

    #[test]
    fn test_stderr_flag_preserved() {
        let lines = make_lines_with_stderr(&[("ok", false), ("err", true)]);
        let wc = wrap_log_lines(&lines, 20, 10);
        assert!(!wc.visual_lines[0].is_stderr);
        assert!(wc.visual_lines[1].is_stderr);
    }

    // -----------------------------------------------------------------------
    // screen_to_text tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_screen_to_text_first_line() {
        let lines = make_lines(&["hello world"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        let pos = wc.screen_to_text(0, 5, 0).unwrap();
        assert_eq!(pos.visual_line_index, 0);
        assert_eq!(pos.byte_offset, 5);
    }

    #[test]
    fn test_screen_to_text_with_scroll() {
        let lines = make_lines(&["aaa", "bbb", "ccc"]);
        let wc = wrap_log_lines(&lines, 20, 2);
        // Scroll offset 1, screen row 0 → visual line 1 ("bbb")
        let pos = wc.screen_to_text(0, 1, 1).unwrap();
        assert_eq!(pos.visual_line_index, 1);
        assert_eq!(pos.byte_offset, 1);
    }

    #[test]
    fn test_screen_to_text_beyond_content() {
        let lines = make_lines(&["abc"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        // Screen row 5 is way beyond content → clamp to last line
        let pos = wc.screen_to_text(5, 0, 0).unwrap();
        assert_eq!(pos.visual_line_index, 0);
    }

    #[test]
    fn test_screen_to_text_col_beyond_line_end() {
        let lines = make_lines(&["hi"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        let pos = wc.screen_to_text(0, 100, 0).unwrap();
        assert_eq!(pos.visual_line_index, 0);
        assert_eq!(pos.byte_offset, 2); // clamped to end of "hi"
    }

    #[test]
    fn test_screen_to_text_cjk_col() {
        // "あいう" — each char is 2 display columns
        let lines = make_lines(&["あいう"]);
        let wc = wrap_log_lines(&lines, 20, 10);

        // col 0 → byte 0 (start of あ)
        let pos = wc.screen_to_text(0, 0, 0).unwrap();
        assert_eq!(pos.byte_offset, 0);

        // col 1 → still byte 0 (middle of あ, which starts at col 0)
        // Actually col_to_byte_offset: col=1, あ width=2, col(0) < 1, advance col to 2
        // Now col(2) >= 1, return byte_idx of い = 3
        // Wait, let me re-check the logic...
        // col=0, grapheme="あ" at byte 0, width=2, col becomes 2
        // col(2) >= display_col(1)? No wait: we check `if col >= display_col` BEFORE advancing.
        // Actually the check is at the start of the loop: if col >= display_col, return byte_idx.
        // For col=1: start with col=0. grapheme "あ" at byte=0. col(0) >= 1? No. col += 2 → 2.
        // Next grapheme "い" at byte=3. col(2) >= 1? Yes. Return 3.
        let pos = wc.screen_to_text(0, 1, 0).unwrap();
        assert_eq!(pos.byte_offset, 3); // byte offset of い

        // col 2 → byte 3 (start of い, which occupies cols 2-3)
        let pos = wc.screen_to_text(0, 2, 0).unwrap();
        assert_eq!(pos.byte_offset, 3);

        // col 4 → byte 6 (start of う)
        let pos = wc.screen_to_text(0, 4, 0).unwrap();
        assert_eq!(pos.byte_offset, 6);
    }

    #[test]
    fn test_screen_to_text_empty() {
        let lines: VecDeque<LogLine> = VecDeque::new();
        let wc = wrap_log_lines(&lines, 20, 10);
        assert!(wc.screen_to_text(0, 0, 0).is_none());
    }

    // -----------------------------------------------------------------------
    // extract_text tests (normal selection)
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_single_line_partial() {
        let lines = make_lines(&["hello world"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        let text = wc.extract_text(
            TextPosition { visual_line_index: 0, byte_offset: 0 },
            TextPosition { visual_line_index: 0, byte_offset: 5 },
        );
        assert_eq!(text, "hello");
    }

    #[test]
    fn test_extract_multi_line() {
        let lines = make_lines(&["aaa", "bbb", "ccc"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        let text = wc.extract_text(
            TextPosition { visual_line_index: 0, byte_offset: 1 },
            TextPosition { visual_line_index: 2, byte_offset: 2 },
        );
        // "aa" + "\n" + "bbb" + "\n" + "cc"
        assert_eq!(text, "aa\nbbb\ncc");
    }

    #[test]
    fn test_extract_across_wrap() {
        // "hello world" wrapped at width 6 → "hello " + "world"
        // These are the SAME logical line, so no \n separator
        let lines = make_lines(&["hello world"]);
        let wc = wrap_log_lines(&lines, 6, 10);
        assert_eq!(wc.visual_lines.len(), 2);
        let text = wc.extract_text(
            TextPosition { visual_line_index: 0, byte_offset: 3 },
            TextPosition { visual_line_index: 1, byte_offset: 3 },
        );
        // "lo " (from vl0[3..]) + "wor" (from vl1[..3]) — no \n since same logical line
        assert_eq!(text, "lo wor");
    }

    #[test]
    fn test_extract_across_logical_lines() {
        let lines = make_lines(&["aaa", "bbb"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        let text = wc.extract_text(
            TextPosition { visual_line_index: 0, byte_offset: 0 },
            TextPosition { visual_line_index: 1, byte_offset: 3 },
        );
        assert_eq!(text, "aaa\nbbb");
    }

    #[test]
    fn test_extract_empty_selection() {
        let lines = make_lines(&["hello"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        let text = wc.extract_text(
            TextPosition { visual_line_index: 0, byte_offset: 2 },
            TextPosition { visual_line_index: 0, byte_offset: 2 },
        );
        assert_eq!(text, "");
    }

    // -----------------------------------------------------------------------
    // extract_block_text tests (rectangular selection)
    // -----------------------------------------------------------------------

    #[test]
    fn test_block_single_row() {
        let lines = make_lines(&["hello world"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        let text = wc.extract_block_text(0, 0, 2, 7);
        assert_eq!(text, "llo w");
    }

    #[test]
    fn test_block_multi_row() {
        let lines = make_lines(&["abcdef", "123456", "xyzxyz"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        let text = wc.extract_block_text(0, 2, 1, 4);
        assert_eq!(text, "bcd\n234\nyzx"); // cols 1..4 of "xyzxyz" = "yzx"
    }

    #[test]
    fn test_block_col_beyond_line() {
        let lines = make_lines(&["ab", "cdefgh"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        // cols 1..5: "ab" only has 2 chars, so we get "b" from first line
        let text = wc.extract_block_text(0, 1, 1, 5);
        assert_eq!(text, "b\ndefg");
    }

    #[test]
    fn test_block_cjk_column() {
        // "あいう" — cols: あ=0-1, い=2-3, う=4-5
        let lines = make_lines(&["あいう"]);
        let wc = wrap_log_lines(&lines, 20, 10);
        // Block select cols 2..4 → should get "い"
        let text = wc.extract_block_text(0, 0, 2, 4);
        assert_eq!(text, "い");
    }

    // -----------------------------------------------------------------------
    // col_to_byte_offset tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_col_to_byte_offset_ascii() {
        assert_eq!(col_to_byte_offset("hello", 0), 0);
        assert_eq!(col_to_byte_offset("hello", 3), 3);
        assert_eq!(col_to_byte_offset("hello", 5), 5); // at end
        assert_eq!(col_to_byte_offset("hello", 100), 5); // beyond end
    }

    #[test]
    fn test_col_to_byte_offset_cjk() {
        // "あいう" — あ at byte 0 (width 2), い at byte 3 (width 2), う at byte 6 (width 2)
        assert_eq!(col_to_byte_offset("あいう", 0), 0);
        assert_eq!(col_to_byte_offset("あいう", 2), 3); // col 2 → start of い
        assert_eq!(col_to_byte_offset("あいう", 4), 6); // col 4 → start of う
        assert_eq!(col_to_byte_offset("あいう", 6), 9); // col 6 → end
    }
}
