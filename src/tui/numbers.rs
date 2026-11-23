//! What the resident board tells serve about its `#` column, for `rexboard jump --shown N`: the key
//! of each row it shows, from number 1, sent when the numbering changes and cleared when the
//! board exits.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use serde_json::json;

use crate::client;

/// How long an exit waits for serve to take the clear: serve is local, so this is generous.
const CLEAR_WITHIN: Duration = Duration::from_millis(500);

/// Set once something was published and not yet cleared, so `clear` asks serve only then.
static PUBLISHED: AtomicBool = AtomicBool::new(false);

/// False once the board is leaving: a send still queued is dropped, not made after the clear.
static OPEN: AtomicBool = AtomicBool::new(true);

fn tell(keys: Option<&[String]>) -> bool {
    let request = json!({ "cmd": "numbers", "owner": std::process::id(), "keys": keys });
    matches!(client::ask_running(&request), Ok(Some(_)))
}

/// Sends the numbering to serve, in order and off the board's loop; a send that did not reach
/// serve (it is down) is sent again at the next draw.
pub struct Publisher {
    tx: Sender<Vec<String>>,
    sent: Option<Vec<String>>,
    failed: std::sync::Arc<AtomicBool>,
}

impl Publisher {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<Vec<String>>();
        let publisher = Self::to(tx);
        let flag = publisher.failed.clone();
        thread::spawn(move || {
            while let Ok(mut keys) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    keys = newer;
                }
                if !OPEN.load(Ordering::SeqCst) {
                    break;
                }
                PUBLISHED.store(true, Ordering::SeqCst);
                flag.store(!tell(Some(&keys)), Ordering::SeqCst);
            }
        });
        publisher
    }

    fn to(tx: Sender<Vec<String>>) -> Self {
        Self {
            tx,
            sent: None,
            failed: std::sync::Arc::new(AtomicBool::new(false)),
        }
    }

    /// The numbering as drawn now; sent when it differs from the last one serve took.
    pub fn publish(&mut self, keys: Vec<String>) {
        if self.failed.swap(false, Ordering::SeqCst) {
            self.sent = None;
        }
        if self.sent.as_ref() != Some(&keys) {
            self.sent = Some(keys.clone());
            let _ = self.tx.send(keys);
        }
    }

    /// Serve restarted and knows nothing: the next draw sends the numbering again.
    pub fn forget(&mut self) {
        self.sent = None;
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        clear();
    }
}

/// Tells serve this board shows no numbers any more. Best effort and quick, from any exit,
/// including one that never runs destructors. serve ignores it when another board has published
/// since.
pub fn clear() {
    OPEN.store(false, Ordering::SeqCst);
    if !PUBLISHED.swap(false, Ordering::SeqCst) {
        return;
    }
    let (done, wait) = mpsc::channel();
    thread::spawn(move || {
        tell(None);
        let _ = done.send(());
    });
    let _ = wait.recv_timeout(CLEAR_WITHIN);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(of: &[&str]) -> Vec<String> {
        of.iter().map(|key| (*key).to_owned()).collect()
    }

    #[test]
    fn the_numbering_is_sent_when_it_changes_and_again_after_serve_was_gone() {
        let (tx, rx) = mpsc::channel();
        let mut publisher = Publisher::to(tx);
        for shown in [&["a", "b"][..], &["a", "b"], &["a", "b"]] {
            publisher.publish(keys(shown));
        }
        assert_eq!(rx.try_iter().count(), 1, "the same numbering is sent once");
        publisher.publish(keys(&["b", "a"]));
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [keys(&["b", "a"])]);
        publisher.forget();
        publisher.publish(keys(&["b", "a"]));
        assert_eq!(
            rx.try_iter().count(),
            1,
            "serve restarted, so it is told again"
        );
        publisher.failed.store(true, Ordering::SeqCst);
        publisher.publish(keys(&["b", "a"]));
        assert_eq!(
            rx.try_iter().count(),
            1,
            "a send that did not reach serve is repeated"
        );
    }
}
