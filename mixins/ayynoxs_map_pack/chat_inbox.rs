// ayynoxs map pack: waypoints shared through private chat messages
// (`/msg <player> [ayynoxs-wp] …`), which work on any server. A chat line
// carrying the marker is kept from the chat and its text waits here for the
// pack's next frame.
use std::sync::Mutex;

pub const MARKER: &str = "[ayynoxs-wp]";
/// Lines waiting at most: a flood of them is dropped.
const MAX: usize = 64;

static INBOX: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Whether `content` is a shared waypoint; its text after the marker waits
/// for `take`.
pub fn seen(content: &crate::text::Text) -> bool {
    let plain = content.plain();
    let Some(at) = plain.find(MARKER) else {
        return false;
    };
    if let Ok(mut inbox) = INBOX.lock() {
        if inbox.len() < MAX {
            inbox.push(plain[at + MARKER.len()..].trim().to_owned());
        }
    }
    true
}

/// The shared waypoints' texts since the last call.
pub fn take() -> Vec<String> {
    INBOX
        .lock()
        .map(|mut inbox| std::mem::take(&mut *inbox))
        .unwrap_or_default()
}
