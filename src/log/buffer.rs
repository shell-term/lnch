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
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(capacity.min(1024)),
            capacity,
        }
    }

    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    pub fn push(&mut self, line: LogLine) {
        if self.lines.len() >= self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub fn lines(&self) -> &VecDeque<LogLine> {
        &self.lines
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.lines.clear();
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
        assert_eq!(buf.lines()[0].content, "line 2");
        assert_eq!(buf.lines()[2].content, "line 4");
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
