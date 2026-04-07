use std::collections::VecDeque;
use std::time::Instant;

const DEFAULT_CAPACITY: usize = 10_000;

#[derive(Debug, Clone)]
pub struct LogLine {
    pub content: String,
    pub is_stderr: bool,
    #[allow(dead_code)]
    pub timestamp: Instant,
}

pub struct LogBuffer {
    lines: VecDeque<LogLine>,
    capacity: usize,
    /// Monotonically increasing counter, bumped on every push/clear.
    /// Used as a cache key for line-wrapping.
    generation: u64,
    /// Whether the overflow warning has already been shown.
    overflow_notified: bool,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(capacity.min(1024)),
            capacity,
            generation: 0,
            overflow_notified: false,
        }
    }

    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    pub fn push(&mut self, line: LogLine) {
        if self.lines.len() >= self.capacity {
            self.lines.pop_front();
            if !self.overflow_notified {
                self.overflow_notified = true;
                // Replace the oldest line with a warning marker.
                if let Some(front) = self.lines.front_mut() {
                    front.content = format!(
                        "[lnch] Log buffer full ({} lines). Oldest lines have been dropped.",
                        self.capacity
                    );
                    front.is_stderr = true;
                }
            }
        }
        self.lines.push_back(line);
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn lines(&self) -> &VecDeque<LogLine> {
        &self.lines
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.generation = self.generation.wrapping_add(1);
        self.overflow_notified = false;
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_line(content: &str) -> LogLine {
        LogLine {
            content: content.to_string(),
            is_stderr: false,
            timestamp: Instant::now(),
        }
    }

    #[test]
    fn test_push_within_capacity() {
        let mut buf = LogBuffer::new(5);
        buf.push(make_line("line 1"));
        buf.push(make_line("line 2"));
        buf.push(make_line("line 3"));
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.lines()[0].content, "line 1");
        assert_eq!(buf.lines()[2].content, "line 3");
    }

    #[test]
    fn test_push_overflow_drops_oldest() {
        let mut buf = LogBuffer::new(3);
        buf.push(make_line("line 1"));
        buf.push(make_line("line 2"));
        buf.push(make_line("line 3"));
        buf.push(make_line("line 4"));
        assert_eq!(buf.len(), 3);
        // First overflow replaces the oldest line with a warning
        assert!(buf.lines()[0].content.contains("Log buffer full"));
        assert!(buf.lines()[0].is_stderr);
        assert_eq!(buf.lines()[2].content, "line 4");
    }

    #[test]
    fn test_overflow_warning_shown_only_once() {
        let mut buf = LogBuffer::new(3);
        buf.push(make_line("line 1"));
        buf.push(make_line("line 2"));
        buf.push(make_line("line 3"));
        buf.push(make_line("line 4")); // first overflow → warning
        buf.push(make_line("line 5")); // second overflow → no warning
        assert_eq!(buf.len(), 3);
        // Warning was on line[0] but has been shifted out; no new warning added
        assert_eq!(buf.lines()[0].content, "line 3");
        assert_eq!(buf.lines()[2].content, "line 5");
    }

    #[test]
    fn test_overflow_warning_resets_on_clear() {
        let mut buf = LogBuffer::new(3);
        buf.push(make_line("line 1"));
        buf.push(make_line("line 2"));
        buf.push(make_line("line 3"));
        buf.push(make_line("line 4")); // overflow → warning
        buf.clear();
        buf.push(make_line("a"));
        buf.push(make_line("b"));
        buf.push(make_line("c"));
        buf.push(make_line("d")); // overflow again → warning again
        assert!(buf.lines()[0].content.contains("Log buffer full"));
    }

    #[test]
    fn test_clear() {
        let mut buf = LogBuffer::new(5);
        buf.push(make_line("line 1"));
        buf.push(make_line("line 2"));
        buf.clear();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }
}
