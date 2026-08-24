//! Process-level slot for the startup tab chooser.
//!
//! When `--choose-tab` is passed, we need to communicate "the first window
//! that finishes construction should open the New Tab Options modal" from
//! `main.rs` (which launches before any windows exist) to
//! `TermWindow::new_window` (which runs later and asynchronously).
//!
//! The slot carries an `Activity` that keeps the mux alive even though the
//! window has zero panes. Without this, `prune_dead_windows` would
//! immediately delete the empty window and terminate the app.
//!
//! It also carries the `--cwd` the launch was given, because the chooser
//! spawns its tab long after the command line has been parsed and forgotten.
//! The Explorer "OnlyTerm Run As" entry is the reason this matters: it passes
//! the folder that was right-clicked, and the shell the user then picks is
//! expected to start there.
//!
//! The slot is one-shot: the first window to call `take()` gets the chooser,
//! every later window gets `None`. This makes the mode one-time, so a
//! `SpawnWindow` during the dialog opens normally.

use mux::activity::Activity;
use parking_lot::Mutex;
use std::path::PathBuf;

/// What `--choose-tab` hands to the first window that finishes construction.
pub struct PendingChooser {
    /// Keeps the tabless mux alive until the dialog resolves.
    pub activity: Activity,
    /// The launch's `--cwd`, if it had one; the spawned tab starts here.
    pub cwd: Option<PathBuf>,
}

static PENDING: Mutex<Option<PendingChooser>> = Mutex::new(None);

/// Arms the chooser for the next window to be constructed, taking ownership
/// of the Activity that keeps the empty mux alive until the dialog resolves.
pub fn arm(activity: Activity, cwd: Option<PathBuf>) {
    *PENDING.lock() = Some(PendingChooser { activity, cwd });
}

/// Consumed by the first TermWindow to finish construction; every later
/// window gets None, so a SpawnWindow during the dialog opens normally.
pub fn take() -> Option<PendingChooser> {
    PENDING.lock().take()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `take()` empties the slot, so the first window gets the chooser
    /// and every later window gets None.
    #[test]
    fn take_consumes_the_slot() {
        arm(Activity::new(), Some(PathBuf::from("C:\\somewhere")));
        let first = take();
        assert_eq!(
            first.as_ref().and_then(|p| p.cwd.as_deref()),
            Some(std::path::Path::new("C:\\somewhere")),
            "the armed cwd must survive to the window that opens the dialog"
        );

        // Intentionally leak the Activity: Activity's Drop calls
        // promise::spawn::spawn_into_main_thread, and under `cargo test`
        // there is no main-thread executor, so dropping would panic.
        // The leak is safe here because the only consequence is a permanent
        // +1 on a global counter that no other test reads.
        std::mem::forget(first);

        assert!(
            take().is_none(),
            "second take() should return None after the slot is consumed"
        );
    }
}
