// ayynoxs map pack: waypoints shared through private chat messages
// (`/msg <player> [ayynoxs-wp] …`), which work on any server. A private
// message carrying the marker is kept from the chat and its text waits here
// for the pack's next frame; public chat never adds waypoints.
use std::sync::Mutex;

pub const MARKER: &str = "[ayynoxs-wp]";
/// Lines waiting at most: a flood of them is dropped.
const MAX: usize = 64;

static INBOX: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A private message to this player (`/msg`), and the sender's own copy of
/// one: other chat lines never carry waypoints.
const INCOMING: &str = "commands.message.display.incoming";
const OUTGOING: &str = "commands.message.display.outgoing";

/// Whether `content` is a shared waypoint to keep out of the chat: a private
/// message whose text after the marker waits for `take`, or the sender's
/// copy of one. Public lines with the marker stay as they are.
pub fn seen(content: &crate::text::Text) -> bool {
    let key = content.translation_key();
    if key != Some(INCOMING) && key != Some(OUTGOING) {
        return false;
    }
    let plain = content.plain();
    let Some(at) = plain.find(MARKER) else {
        return false;
    };
    if key == Some(OUTGOING) {
        return true;
    }
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
