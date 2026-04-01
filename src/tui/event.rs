use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::message::AppEvent;

/// Spawns a background task that reads terminal events and forwards them as AppEvent.
///
/// Uses crossterm's async `EventStream` instead of blocking `poll`/`read` so we
/// never block a tokio worker thread.
pub fn spawn_event_reader(tx: mpsc::Sender<AppEvent>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(200));

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if tx.send(AppEvent::Tick).await.is_err() {
                        break;
                    }
                }
                maybe_event = reader.next() => {
                    match maybe_event {
                        Some(Ok(Event::Key(key))) => {
                            if tx.send(AppEvent::Key(key)).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(Event::Mouse(mouse))) => {
                            if tx.send(AppEvent::Mouse(mouse)).await.is_err() {
                                break;
                            }
                        }
                        Some(Err(_)) | None => break,
                        _ => {}
                    }
                }
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
