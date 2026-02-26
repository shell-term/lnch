use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

use crate::message::AppEvent;

/// Spawns a background task that reads terminal events and forwards them as AppEvent
pub fn spawn_event_reader(tx: mpsc::Sender<AppEvent>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let tick_rate = Duration::from_millis(200);
        loop {
            if event::poll(tick_rate).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if tx.send(AppEvent::Key(key)).await.is_err() {
                        break;
                    }
                }
            } else if tx.send(AppEvent::Tick).await.is_err() {
                break;
            }
        }
    })
}

/// Determine what action to take based on a key event.
/// Returns true if the app should quit.
pub fn should_quit(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}
