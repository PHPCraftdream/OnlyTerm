use std::io::Write;


/// Allows sharing the writer between the Pane and the Terminal.
/// This could potentially be eliminated in the future if we can
/// teach the Pane impl to reference the writer in the Termninal,
/// but the Pane trait returns a RefMut and that makes it a bit
/// awkward at the moment.
///
/// This is a non-blocking, thread-backed wrapper over the real
/// (blocking) pty writer, in the same spirit as `wezterm_term`'s private
/// `ThreadedWriter`: `write`/`flush` here just enqueue onto an unbounded
/// channel and return immediately; a single detached background thread
/// (one per pane, spawned in `WriterWrapper::new`) drains the channel and
/// performs the real, potentially-blocking writes.
///
/// Why this exists: `Pane::writer()` (`crates/mux/src/localpane.rs`) hands
/// out a lock guard directly over a `WriterWrapper` clone, and roughly a
/// dozen call sites across `wezterm-gui` (paste, `SendString`, IME
/// composition, character-picker insertion, quick-select, ...) call
/// `pane.writer().write_all(...)` synchronously from the GUI thread. The
/// old implementation here was a direct, blocking pass-through
/// (`self.writer.lock().write(buf)` = a real `WriteFile`/pipe write): if
/// the child process wasn't reading its stdin (e.g. a full pipe buffer),
/// any of those call sites could block the GUI thread forever and freeze
/// every window in the process -- the same class of bug fixed for
/// `LocalPane::kill()`'s soft-interrupt write, just not limited to
/// `kill()`.
///
/// Every clone of a `WriterWrapper` shares the same `Sender` and so
/// enqueues onto the *same* background thread/queue. This matters because
/// `LocalDomain::spawn_pane` and `TmuxDomain`'s pane spawn (the two
/// constructors of a `WriterWrapper`) both hand one clone to the `Pane`
/// impl (`LocalPane.writer`) and a second clone into `Terminal::new`
/// (wrapped in `wezterm_term`'s own internal writer machinery): keeping
/// them on one shared thread/queue preserves whatever relative ordering
/// they already had (there was never a *strict* ordering guarantee
/// between the two independent paths, and this change doesn't need to
/// invent one -- see `TerminalState::new_with_nonblocking_writer`, which
/// `Terminal::new_with_nonblocking_writer` uses here specifically to
/// avoid wrapping this already-non-blocking writer in a second, redundant
/// thread/queue of its own).
///
/// Queue depth is intentionally unbounded, matching `ThreadedWriter`:
/// bounding it would only trade an already-vanishingly-unlikely
/// unbounded-memory-growth risk for a very real risk of turning a
/// slow/stuck child process back into a mechanism that can block a caller
/// (once a bounded channel is full, `send` either blocks or the caller
/// has to drop data) -- exactly what this type exists to avoid.
///
/// A real write failure (the pty is gone or broken) can now only be
/// observed asynchronously, on the background thread, well after
/// `write`/`flush` already returned `Ok` to the caller. That failure is
/// not silently swallowed: it's logged once (further failures are not
/// re-logged, since once the real writer is broken every subsequent
/// write will fail the same way and the pane's process exiting will
/// naturally surface via the existing, independent `is_dead()` /
/// `child_waiter` machinery in `LocalPane` -- there is no need for this
/// type to also reach back into the pane to mark it dead).
#[derive(Clone)]
pub(crate) struct WriterWrapper {
    sender: std::sync::mpsc::Sender<WriterWrapperMessage>,
}

enum WriterWrapperMessage {
    Data(Vec<u8>),
    Flush,
}

impl WriterWrapper {
    pub fn new(mut writer: Box<dyn Write + Send>) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel::<WriterWrapperMessage>();

        let builder = std::thread::Builder::new().name("pane-writer".into());
        if let Err(err) = builder.spawn(move || {
            let mut failed = false;
            while let Ok(msg) = receiver.recv() {
                if failed {
                    // The real writer already failed once; every
                    // subsequent write will fail the same way (broken
                    // pipe / gone pty). Keep draining the channel so
                    // senders never block or error out on a closed
                    // channel, but don't attempt more real I/O or spam
                    // the log.
                    continue;
                }
                let result = match msg {
                    WriterWrapperMessage::Data(buf) => writer.write_all(&buf),
                    WriterWrapperMessage::Flush => writer.flush(),
                };
                if let Err(err) = result {
                    log::error!(
                        "pane writer thread: write to pty failed, pty is likely \
                         gone; further writes to this pane will be silently \
                         discarded (the pane's process exiting will surface \
                         normally via the usual exit-status path): {:#}",
                        err
                    );
                    failed = true;
                }
            }
        }) {
            // Spawning a thread should essentially never fail (it means
            // the OS is out of resources); if it does, fall back to a
            // wrapper with no live receiver, so writes/flushes below
            // still return promptly (as a `BrokenPipe` error) instead of
            // panicking, rather than trying to do a blocking write here.
            log::error!(
                "Failed to spawn pane-writer thread; pane writes will fail: {:#}",
                err
            );
        }

        Self { sender }
    }
}

impl std::io::Write for WriterWrapper {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.sender
            .send(WriterWrapperMessage::Data(buf.to_vec()))
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::BrokenPipe, err))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.sender
            .send(WriterWrapperMessage::Flush)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::BrokenPipe, err))?;
        Ok(())
    }
}
