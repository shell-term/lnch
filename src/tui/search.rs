use std::collections::VecDeque;

use crate::log::buffer::LogLine;

/// A single match within a logical log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Index into the LogBuffer lines.
    pub logical_line_index: usize,
    /// Byte offset (inclusive) within the logical line.
    pub byte_start: usize,
    /// Byte offset (exclusive) within the logical line.
    pub byte_end: usize,
}

/// Log search state.
pub struct SearchState {
    /// Current search query.
    pub query: String,
    /// Whether the search input bar is active (typing mode).
    pub active: bool,
    /// All matches for the current query against the current task's log.
    pub matches: Vec<SearchMatch>,
    /// Index into `matches` for the currently focused match.
    pub current_index: Option<usize>,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            active: false,
            matches: Vec::new(),
            current_index: None,
        }
    }

    /// Start search input mode.
    pub fn activate(&mut self) {
        self.active = true;
        self.query.clear();
        self.matches.clear();
        self.current_index = None;
    }

    /// Cancel search and clear everything.
    pub fn cancel(&mut self) {
        self.active = false;
        self.query.clear();
        self.matches.clear();
        self.current_index = None;
    }

    /// Clear highlights but keep query for potential re-search.
    pub fn clear_highlights(&mut self) {
        self.matches.clear();
        self.current_index = None;
        self.query.clear();
    }

    /// Confirm search (Enter pressed). Exit input mode, focus first match.
    pub fn confirm(&mut self) {
        self.active = false;
        if !self.matches.is_empty() {
            self.current_index = Some(0);
        }
    }

    /// Whether there are search results to display.
    pub fn has_results(&self) -> bool {
        !self.query.is_empty() && !self.matches.is_empty()
    }

    /// Whether the search query is non-empty (even with no matches).
    pub fn has_query(&self) -> bool {
        !self.query.is_empty()
    }

    /// Move to the next match (wraps around).
    pub fn next_match(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.current_index = Some(match self.current_index {
            Some(i) => (i + 1) % self.matches.len(),
            None => 0,
        });
    }

    /// Move to the previous match (wraps around).
    pub fn prev_match(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.current_index = Some(match self.current_index {
            Some(0) => self.matches.len() - 1,
            Some(i) => i - 1,
            None => self.matches.len() - 1,
        });
    }

    /// Recompute matches against the given log lines.
    pub fn update_matches(&mut self, lines: &VecDeque<LogLine>) {
        self.matches = find_matches(&self.query, lines);
        // Keep current_index valid.
        if self.matches.is_empty() {
            self.current_index = None;
        } else if let Some(idx) = self.current_index {
            if idx >= self.matches.len() {
                self.current_index = Some(self.matches.len() - 1);
            }
        }
    }

    /// Get the currently focused match.
    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.current_index.and_then(|i| self.matches.get(i))
    }
}

/// Determine if search should be case-sensitive (smart case).
/// All-lowercase query → case-insensitive; otherwise case-sensitive.
fn is_case_sensitive(query: &str) -> bool {
    query.chars().any(|c| c.is_uppercase())
}

/// Find all matches of `query` in the log lines.
pub fn find_matches(query: &str, lines: &VecDeque<LogLine>) -> Vec<SearchMatch> {
    if query.is_empty() {
        return Vec::new();
    }

    let case_sensitive = is_case_sensitive(query);
    let query_lower = if case_sensitive {
        String::new()
    } else {
        query.to_lowercase()
    };
    let search_query = if case_sensitive { query } else { &query_lower };

    let mut matches = Vec::new();

    for (line_idx, log_line) in lines.iter().enumerate() {
        let content = &log_line.content;
        let haystack = if case_sensitive {
            content.to_string()
        } else {
            content.to_lowercase()
        };

        let mut start = 0;
        while let Some(pos) = haystack[start..].find(search_query) {
            let byte_start = start + pos;
            let byte_end = byte_start + search_query.len();
            matches.push(SearchMatch {
                logical_line_index: line_idx,
                byte_start,
                byte_end,
            });
            start = byte_end;
        }
    }

    matches
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn make_lines(texts: &[&str]) -> VecDeque<LogLine> {
        texts
            .iter()
            .map(|t| LogLine {
                content: t.to_string(),
                is_stderr: false,
                timestamp: Instant::now(),
            })
            .collect()
    }

    #[test]
    fn test_find_matches_basic() {
        let lines = make_lines(&["hello world", "goodbye world", "hello again"]);
        let matches = find_matches("hello", &lines);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].logical_line_index, 0);
        assert_eq!(matches[0].byte_start, 0);
        assert_eq!(matches[0].byte_end, 5);
        assert_eq!(matches[1].logical_line_index, 2);
    }

    #[test]
    fn test_find_matches_case_insensitive() {
        let lines = make_lines(&["Hello World", "HELLO", "hello"]);
        // All-lowercase query → case-insensitive
        let matches = find_matches("hello", &lines);
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_find_matches_case_sensitive() {
        let lines = make_lines(&["Hello World", "HELLO", "hello"]);
        // Query with uppercase → case-sensitive
        let matches = find_matches("Hello", &lines);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].logical_line_index, 0);
    }

    #[test]
    fn test_find_matches_multiple_per_line() {
        let lines = make_lines(&["abcabc", "abc"]);
        let matches = find_matches("abc", &lines);
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].byte_start, 0);
        assert_eq!(matches[0].byte_end, 3);
        assert_eq!(matches[1].byte_start, 3);
        assert_eq!(matches[1].byte_end, 6);
        assert_eq!(matches[2].logical_line_index, 1);
    }

    #[test]
    fn test_find_matches_empty_query() {
        let lines = make_lines(&["hello"]);
        let matches = find_matches("", &lines);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_next_match_cycles() {
        let lines = make_lines(&["aaa", "bbb", "aaa"]);
        let mut state = SearchState::new();
        state.query = "aaa".to_string();
        state.update_matches(&lines);
        assert_eq!(state.matches.len(), 2);

        state.next_match();
        assert_eq!(state.current_index, Some(0));
        state.next_match();
        assert_eq!(state.current_index, Some(1));
        state.next_match();
        assert_eq!(state.current_index, Some(0)); // wraps
    }

    #[test]
    fn test_prev_match_cycles() {
        let lines = make_lines(&["aaa", "bbb", "aaa"]);
        let mut state = SearchState::new();
        state.query = "aaa".to_string();
        state.update_matches(&lines);

        state.prev_match();
        assert_eq!(state.current_index, Some(1)); // starts from end
        state.prev_match();
        assert_eq!(state.current_index, Some(0));
        state.prev_match();
        assert_eq!(state.current_index, Some(1)); // wraps
    }

    #[test]
    fn test_next_match_empty() {
        let mut state = SearchState::new();
        state.next_match();
        assert_eq!(state.current_index, None);
    }

    #[test]
    fn test_activate_clears_state() {
        let mut state = SearchState::new();
        state.query = "old".to_string();
        state.matches.push(SearchMatch {
            logical_line_index: 0,
            byte_start: 0,
            byte_end: 3,
        });
        state.current_index = Some(0);

        state.activate();
        assert!(state.active);
        assert!(state.query.is_empty());
        assert!(state.matches.is_empty());
        assert_eq!(state.current_index, None);
    }

    #[test]
    fn test_confirm_sets_first_match() {
        let lines = make_lines(&["hello", "world"]);
        let mut state = SearchState::new();
        state.active = true;
        state.query = "hello".to_string();
        state.update_matches(&lines);

        state.confirm();
        assert!(!state.active);
        assert_eq!(state.current_index, Some(0));
    }

    #[test]
    fn test_cancel_clears_everything() {
        let mut state = SearchState::new();
        state.active = true;
        state.query = "test".to_string();
        state.current_index = Some(0);

        state.cancel();
        assert!(!state.active);
        assert!(state.query.is_empty());
        assert!(state.matches.is_empty());
        assert_eq!(state.current_index, None);
    }

    #[test]
    fn test_update_matches_clamps_index() {
        let lines = make_lines(&["aaa", "aaa", "aaa"]);
        let mut state = SearchState::new();
        state.query = "aaa".to_string();
        state.update_matches(&lines);
        state.current_index = Some(2);

        // Reduce to fewer matches
        let lines2 = make_lines(&["aaa", "bbb"]);
        state.update_matches(&lines2);
        assert_eq!(state.current_index, Some(0)); // clamped
    }
}
