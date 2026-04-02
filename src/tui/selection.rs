use std::time::{Duration, Instant};

/// Absolute screen position (terminal coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPos {
    pub col: u16,
    pub row: u16,
}

/// Selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Normal (stream) selection — reading order across lines.
    Normal,
    /// Block (rectangular) selection — same column range on every row.
    Block,
}

/// Text-selection state machine.
#[derive(Debug, Clone)]
pub enum SelectionState {
    /// No active selection.
    None,
    /// Mouse is held down, actively selecting.
    Selecting {
        anchor: ScreenPos,
        current: ScreenPos,
        mode: SelectionMode,
    },
    /// Selection complete, highlight visible, waiting for copy action.
    Selected {
        anchor: ScreenPos,
        current: ScreenPos,
        mode: SelectionMode,
    },
    /// Selection done, showing "Copied!" feedback.
    CopiedFeedback { expires_at: Instant },
}

const FEEDBACK_DURATION: Duration = Duration::from_secs(2);

impl SelectionState {
    pub fn new() -> Self {
        Self::None
    }

    /// Returns the anchor and current positions if actively selecting or selected.
    pub fn selecting_range(&self) -> Option<(ScreenPos, ScreenPos, SelectionMode)> {
        match self {
            Self::Selecting {
                anchor,
                current,
                mode,
            }
            | Self::Selected {
                anchor,
                current,
                mode,
            } => Some((*anchor, *current, *mode)),
            _ => Option::None,
        }
    }

    /// Returns the (start, end) in reading order for a selection.
    pub fn normalized_range(&self) -> Option<(ScreenPos, ScreenPos, SelectionMode)> {
        let (anchor, current, mode) = self.selecting_range()?;
        let (start, end) = if anchor.row < current.row
            || (anchor.row == current.row && anchor.col <= current.col)
        {
            (anchor, current)
        } else {
            (current, anchor)
        };
        Some((start, end, mode))
    }

    /// Whether the selection is finalized and waiting for a copy action.
    pub fn is_selected(&self) -> bool {
        matches!(self, Self::Selected { .. })
    }

    /// Transition from Selecting to Selected (mouse released).
    pub fn finish_selecting(&mut self) {
        if let Self::Selecting {
            anchor,
            current,
            mode,
        } = *self
        {
            *self = Self::Selected {
                anchor,
                current,
                mode,
            };
        }
    }

    /// Whether to show "Copied!" feedback in the status bar.
    pub fn show_copied(&self) -> bool {
        matches!(self, Self::CopiedFeedback { .. })
    }

    /// Expire feedback if the duration has elapsed. Call from tick handler.
    pub fn tick(&mut self) {
        if let Self::CopiedFeedback { expires_at } = self {
            if Instant::now() >= *expires_at {
                *self = Self::None;
            }
        }
    }

    /// Transition to Selecting.
    pub fn start_selecting(anchor: ScreenPos, mode: SelectionMode) -> Self {
        Self::Selecting {
            anchor,
            current: anchor,
            mode,
        }
    }

    /// Transition to CopiedFeedback.
    pub fn copied() -> Self {
        Self::CopiedFeedback {
            expires_at: Instant::now() + FEEDBACK_DURATION,
        }
    }

    /// Clear the selection.
    pub fn clear(&mut self) {
        *self = Self::None;
    }

    /// Whether a selection is actively happening (for rendering).
    #[cfg(test)]
    pub fn is_selecting(&self) -> bool {
        matches!(self, Self::Selecting { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(col: u16, row: u16) -> ScreenPos {
        ScreenPos { col, row }
    }

    #[test]
    fn test_state_none_to_selecting() {
        let state = SelectionState::start_selecting(pos(5, 10), SelectionMode::Normal);
        assert!(state.is_selecting());
        let (anchor, current, mode) = state.selecting_range().unwrap();
        assert_eq!(anchor, pos(5, 10));
        assert_eq!(current, pos(5, 10));
        assert_eq!(mode, SelectionMode::Normal);
    }

    #[test]
    fn test_state_selecting_to_copied() {
        let state = SelectionState::copied();
        assert!(state.show_copied());
        assert!(!state.is_selecting());
    }

    #[test]
    fn test_state_selecting_click() {
        // anchor == current means a single click (no drag)
        let state = SelectionState::start_selecting(pos(3, 3), SelectionMode::Normal);
        let (anchor, current, _) = state.selecting_range().unwrap();
        assert_eq!(anchor, current);
    }

    #[test]
    fn test_state_selecting_cancel_scroll() {
        let mut state = SelectionState::start_selecting(pos(1, 1), SelectionMode::Normal);
        state.clear();
        assert!(matches!(state, SelectionState::None));
    }

    #[test]
    fn test_state_feedback_expires() {
        let mut state = SelectionState::CopiedFeedback {
            expires_at: Instant::now() - Duration::from_millis(1),
        };
        state.tick();
        assert!(matches!(state, SelectionState::None));
    }

    #[test]
    fn test_state_feedback_not_expired() {
        let mut state = SelectionState::CopiedFeedback {
            expires_at: Instant::now() + Duration::from_secs(10),
        };
        state.tick();
        assert!(state.show_copied());
    }

    #[test]
    fn test_selection_range_normalized() {
        // Backward drag (bottom-right to top-left)
        let state = SelectionState::Selecting {
            anchor: pos(10, 5),
            current: pos(2, 1),
            mode: SelectionMode::Normal,
        };
        let (start, end, _) = state.normalized_range().unwrap();
        assert_eq!(start, pos(2, 1));
        assert_eq!(end, pos(10, 5));
    }

    #[test]
    fn test_block_mode_detection() {
        let state = SelectionState::start_selecting(pos(0, 0), SelectionMode::Block);
        let (_, _, mode) = state.selecting_range().unwrap();
        assert_eq!(mode, SelectionMode::Block);
    }

    #[test]
    fn test_finish_selecting_to_selected() {
        let mut state = SelectionState::Selecting {
            anchor: pos(1, 2),
            current: pos(10, 5),
            mode: SelectionMode::Normal,
        };
        state.finish_selecting();
        assert!(state.is_selected());
        assert!(!state.is_selecting());
        let (anchor, current, mode) = state.selecting_range().unwrap();
        assert_eq!(anchor, pos(1, 2));
        assert_eq!(current, pos(10, 5));
        assert_eq!(mode, SelectionMode::Normal);
    }

    #[test]
    fn test_selected_normalized_range() {
        let state = SelectionState::Selected {
            anchor: pos(10, 5),
            current: pos(2, 1),
            mode: SelectionMode::Normal,
        };
        let (start, end, _) = state.normalized_range().unwrap();
        assert_eq!(start, pos(2, 1));
        assert_eq!(end, pos(10, 5));
    }

    #[test]
    fn test_selected_clear() {
        let mut state = SelectionState::Selected {
            anchor: pos(1, 1),
            current: pos(5, 5),
            mode: SelectionMode::Normal,
        };
        state.clear();
        assert!(matches!(state, SelectionState::None));
    }
}
