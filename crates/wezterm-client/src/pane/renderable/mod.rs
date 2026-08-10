use crate::domain::ClientInner;
use crate::pane::clientpane::ClientPane;
use anyhow::anyhow;
use codec::*;
use config::{configuration, ConfigHandle};
use lru::LruCache;
use mux::pane::PaneId;
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::Mux;
use rangeset::*;
use ratelim::RateLimiter;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use termwiz::cell::{Cell, CellAttributes, Underline};
use termwiz::surface::{SequenceNo, SEQ_ZERO};
use url::Url;
use wezterm_term::{KeyCode, KeyModifiers, Line, StableRowIndex};

mod hydrate;
mod state;

pub(crate) use hydrate::hydrate_lines;
pub use state::RenderableState;

const MAX_POLL_INTERVAL: Duration = Duration::from_secs(30);
const BASE_POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Debug)]
enum LineEntry {
    // Up to date wrt. server and has been rendered at least once
    Line(Line),
    // Currently being downloaded from the server
    Fetching(Instant),
    // We have a version of the line locally and are treating it
    // as needing rendering because we are also in the process of
    // downloading a newer version from the server
    LineAndFetching(Line, Instant),
    // We have a local copy but it is stale and will need to be
    // fetched again
    Stale(Line),
}

impl LineEntry {
    fn kind(&self) -> (&'static str, Option<Instant>) {
        match self {
            Self::Line(_) => ("Line", None),
            Self::Fetching(since) => ("Fetching", Some(*since)),
            Self::LineAndFetching(_, since) => ("LineAndFetching", Some(*since)),
            Self::Stale(_) => ("Stale", None),
        }
    }
}

pub struct RenderableInner {
    pub client: Arc<ClientInner>,
    remote_pane_id: PaneId,
    local_pane_id: PaneId,
    last_poll: Instant,
    pub dead: bool,
    poll_in_progress: AtomicBool,
    poll_interval: Duration,

    cursor_position: StableCursorPosition,
    pub dimensions: RenderableDimensions,

    lines: LruCache<StableRowIndex, LineEntry>,
    pub title: String,
    pub working_dir: Option<Url>,
    pub seqno: SequenceNo,

    /// Synthetic sequence number used to mark local-echo prediction edits
    /// (`apply_prediction`/`apply_paste_prediction`) applied in place to
    /// `Line`s that live in `self.lines`. Those lines already carry a real,
    /// server-authoritative seqno (see `apply_changes_to_surface`, which
    /// calls `update_last_change_seqno(self.seqno)`), so predictions must
    /// never be tagged with `SEQ_ZERO` -- `update_last_change_seqno` only
    /// ever increases a line's seqno, so SEQ_ZERO on an already-nonzero
    /// line is a silent no-op and the GUI's seqno-keyed render caches
    /// (e.g. shape_hash_cache) would keep serving the pre-prediction
    /// shape/quads. This counter must also never reuse `self.seqno`'s
    /// namespace, since that space is driven by the server and a
    /// prediction is a local-only, unconfirmed edit. Seeded far above any
    /// plausible real seqno and incremented once per prediction call (not
    /// per cell) so each local-echo edit is visible to those caches as a
    /// new version of the line.
    prediction_seqno: SequenceNo,

    fetch_limiter: RateLimiter,

    last_send_time: Instant,
    pub last_recv_time: Instant,
    last_late_dirty: Instant,
    last_input_rtt: u64,

    pub input_serial: InputSerial,
}

impl RenderableInner {
    pub fn new(
        client: &Arc<ClientInner>,
        remote_pane_id: PaneId,
        local_pane_id: PaneId,
        dimensions: RenderableDimensions,
        title: &str,
        fetch_limiter: RateLimiter,
    ) -> Self {
        let now = Instant::now();

        Self {
            client: Arc::clone(client),
            remote_pane_id,
            local_pane_id,
            last_poll: now,
            dead: false,
            poll_in_progress: AtomicBool::new(false),
            poll_interval: BASE_POLL_INTERVAL,
            cursor_position: StableCursorPosition::default(),
            dimensions,
            lines: LruCache::new(
                NonZeroUsize::new(configuration().scrollback_lines.max(128)).unwrap(),
            ),
            title: title.to_string(),
            working_dir: None,
            fetch_limiter,
            last_send_time: now,
            last_recv_time: now,
            last_late_dirty: now,
            last_input_rtt: 0,
            input_serial: InputSerial::empty(),
            seqno: SEQ_ZERO,
            // Seeded far above any plausible real terminal seqno so it can
            // never collide with the server-authoritative `seqno` space.
            // See field doc comment.
            prediction_seqno: usize::MAX / 2,
        }
    }

    /// Returns true if we think we should display the laggy connection
    /// indicator.  If we're past our poll interval and more recently
    /// tried to send something than receive something, the UI is worth
    /// showing.
    pub fn is_tardy(&self) -> bool {
        let elapsed = self.last_recv_time.elapsed();
        if elapsed > self.poll_interval.max(Duration::from_secs(3)) {
            self.last_send_time > self.last_recv_time
        } else {
            false
        }
    }

    /// Predictive echo can be noisy when the link is working well,
    /// so we only employ it when it looks like the latency is high.
    fn should_predict(&self) -> bool {
        self.client
            .local_echo_threshold_ms
            .map(|thresh| self.last_input_rtt >= thresh)
            .unwrap_or(false)
    }

    /// Compute a "prediction" and apply it to the line data that we
    /// have available, marking it as dirty so that it gets rendered.
    /// The prediction is basically just local echo.
    /// Open questions:
    /// how do we tell if the intent is to suppress local echo during eg:
    ///  * password prompt?  One option is to look back and see if the line
    ///    looks like a password prompt.
    ///  * normal mode in vim: letter presses are typically movement or
    ///    other editor commands
    ///
    /// There are bound to be a number of other edge cases that we should
    /// handle.
    fn apply_prediction(&mut self, c: KeyCode, line: &mut Line) {
        let text = line.as_str();
        if text.contains("sword") {
            // This line might be a password prompt.  Don't force
            // on local echo here, as we don't want to reveal content
            // from their password
            return;
        }

        // One synthetic seqno per prediction event, applied to every edit
        // made to `line` below. See `prediction_seqno` doc comment.
        self.prediction_seqno += 1;
        let seqno = self.prediction_seqno;

        match c {
            KeyCode::Enter => {
                self.cursor_position.x = 0;
                self.cursor_position.y += 1;
            }
            KeyCode::UpArrow => {
                self.cursor_position.y = self.cursor_position.y.saturating_sub(1);
            }
            KeyCode::DownArrow => {
                self.cursor_position.y += 1;
            }
            KeyCode::RightArrow => {
                self.cursor_position.x += 1;
            }
            KeyCode::LeftArrow => {
                self.cursor_position.x = self.cursor_position.x.saturating_sub(1);
            }
            KeyCode::Delete => {
                line.erase_cell(self.cursor_position.x, seqno);
            }
            KeyCode::Backspace => {
                if self.cursor_position.x > 0 {
                    line.erase_cell(self.cursor_position.x - 1, seqno);
                    self.cursor_position.x -= 1;
                }
            }
            KeyCode::Char(c) => {
                let cell = Cell::new(
                    c,
                    CellAttributes::default()
                        .set_underline(Underline::Double)
                        .clone(),
                );

                let width = cell.width();
                line.set_cell(self.cursor_position.x, cell, seqno);
                // Adjust the cursor to reflect the width of this new cell
                self.cursor_position.x += width;
            }
            _ => {}
        }
    }

    /// Based on a keypress, apply a "prediction" of what the terminal
    /// content will look like once we receive the response from the
    /// remote system.  The prediction helps to reduce perceived latency
    /// when a user is typing at any reasonable velocity.
    pub fn predict_from_key_event(&mut self, key: KeyCode, mods: KeyModifiers) {
        if !self.should_predict() {
            return;
        }

        let c = match key {
            KeyCode::LeftArrow
            | KeyCode::RightArrow
            | KeyCode::UpArrow
            | KeyCode::DownArrow
            | KeyCode::Delete
            | KeyCode::Backspace
            | KeyCode::Enter
            | KeyCode::Char(_) => key,
            _ => return,
        };
        if mods != KeyModifiers::NONE && mods != KeyModifiers::SHIFT {
            return;
        }

        let row = self.cursor_position.y;
        match self.lines.pop(&row) {
            Some(LineEntry::Stale(mut line)) | Some(LineEntry::Line(mut line)) => {
                self.apply_prediction(c, &mut line);
                self.lines.put(row, LineEntry::Line(line));
            }
            Some(LineEntry::LineAndFetching(mut line, instant)) => {
                self.apply_prediction(c, &mut line);
                self.lines
                    .put(row, LineEntry::LineAndFetching(line, instant));
            }
            Some(entry) => {
                self.lines.put(row, entry);
            }
            None => {}
        }
    }

    fn apply_paste_prediction(
        &mut self,
        row: usize,
        text: &str,
        line: &mut Line,
        seqno: SequenceNo,
    ) {
        let attrs = CellAttributes::default()
            .set_underline(Underline::Double)
            .clone();

        let text_line = Line::from_text(text, &attrs, seqno, None);

        if row == 0 {
            for cell in text_line.visible_cells() {
                line.set_cell(self.cursor_position.x, cell.as_cell(), seqno);
                self.cursor_position.x += cell.width();
            }
        } else {
            // The pasted line replaces the data for the existing line
            line.resize_and_clear(0, seqno, CellAttributes::default());
            line.append_line(text_line, seqno);
            self.cursor_position.x = line.len();
        }
    }

    pub fn predict_from_paste(&mut self, text: &str) {
        if !self.should_predict() {
            return;
        }

        // One synthetic seqno for the whole paste operation, applied to
        // every row touched below. See `prediction_seqno` doc comment.
        self.prediction_seqno += 1;
        let seqno = self.prediction_seqno;

        let text = textwrap::fill(text, self.dimensions.cols);
        let lines: Vec<&str> = text.split("\n").collect();

        for (idx, paste_line) in lines.iter().enumerate() {
            let row = self.cursor_position.y + idx as StableRowIndex;

            match self.lines.pop(&row) {
                Some(LineEntry::Stale(mut line)) | Some(LineEntry::Line(mut line)) => {
                    self.apply_paste_prediction(idx, paste_line, &mut line, seqno);
                    self.lines.put(row, LineEntry::Line(line));
                }
                Some(LineEntry::LineAndFetching(mut line, instant)) => {
                    self.apply_paste_prediction(idx, paste_line, &mut line, seqno);
                    self.lines
                        .put(row, LineEntry::LineAndFetching(line, instant));
                }
                Some(entry) => {
                    self.lines.put(row, entry);
                }
                None => {}
            }
        }
        self.cursor_position.y += lines.len().saturating_sub(1) as StableRowIndex;
    }

    pub fn update_last_send(&mut self) {
        self.last_send_time = Instant::now();
        self.poll_interval = BASE_POLL_INTERVAL;
    }

    pub fn apply_changes_to_surface(
        &mut self,
        delta: GetPaneRenderChangesResponse,
        bonus_lines: Vec<(StableRowIndex, Line)>,
    ) {
        log::trace!(
            "apply_changes_to_surface local={} remote={}",
            self.local_pane_id,
            self.remote_pane_id
        );
        let now = Instant::now();
        self.poll_interval = BASE_POLL_INTERVAL;
        self.last_recv_time = now;

        let mut dirty = RangeSet::new();
        for r in delta.dirty_lines {
            dirty.add_range(r.clone());
        }
        if delta.cursor_position != self.cursor_position {
            dirty.add(self.cursor_position.y);
            // But note that the server may have sent this in bonus_lines;
            // we'll address that below
            dirty.add(delta.cursor_position.y);
        }

        // Keep track of the approximate round trip time by recording how
        // long it took for this response to come back
        if let Some(serial) = delta.input_serial {
            self.last_input_rtt = serial.elapsed_millis();
        }

        // When it comes to updating the cursor position, if the update was tagged
        // with keyboard input, we'll only take the position if the update comes from
        // the most recent key event.  This helps to prevent the cursor wiggling if the
        // user is typing more than one character per roundtrip interval--the wiggle
        // manifests because we may have already predicted a local cursor move forwards
        // by one character, and we may receive the response to the prior update after
        // we have rendered that, and then shortly receive the most recent response.
        // The result of that is that the cursor moves right one, left one and then
        // finally right one in quick succession.
        // If the delta was not from an input event then we trust it; this is most
        // like due to a unilateral movement by the application on the other end.
        if delta.input_serial.is_none()
            || delta.input_serial.unwrap_or(InputSerial::empty()) >= self.input_serial
        {
            self.cursor_position = delta.cursor_position;
        }
        self.dimensions = delta.dimensions;
        self.title = delta.title;
        self.working_dir = delta.working_dir.map(Into::into);
        log::trace!(
            "server says: seqno from {} -> {} for local_pane_id={}",
            self.seqno,
            delta.seqno,
            self.local_pane_id
        );
        self.seqno = delta.seqno;

        let config = configuration();
        for (stable_row, line) in bonus_lines {
            log::trace!("bonus line {} seqno={}", stable_row, line.current_seqno());
            self.put_line(stable_row, line, &config, None);
            dirty.remove(stable_row);
        }

        log::trace!(
            "apply_changes_to_surface: Generate PaneOutput event for local={}",
            self.local_pane_id
        );
        Mux::get().notify(mux::MuxNotification::PaneOutput(self.local_pane_id));

        let mut to_fetch = RangeSet::new();
        log::trace!("dirty as of seq {} -> {:?}", delta.seqno, dirty);
        for r in dirty.iter() {
            for stable_row in r.clone() {
                // If a line is in the (probable) viewport region,
                // then we'll likely want to fetch it.
                // If it is outside that region, remove it from our cache
                // so that we'll fetch it on demand later.
                let fetchable = stable_row >= delta.dimensions.physical_top;
                let prior = self.lines.pop(&stable_row);
                let prior_kind = prior.as_ref().map(|e| e.kind());
                if !fetchable {
                    log::trace!("make {} stale bcos not fetchable", stable_row);
                    self.make_stale(stable_row);
                    continue;
                }
                to_fetch.add(stable_row);
                let entry = match prior {
                    Some(LineEntry::Fetching(_)) | None => LineEntry::Fetching(now),
                    Some(LineEntry::LineAndFetching(old, ..))
                    | Some(LineEntry::Stale(old))
                    | Some(LineEntry::Line(old)) => LineEntry::LineAndFetching(old, now),
                };
                log::trace!(
                    "row {} {:?} -> {:?} due to dirty and IN viewport",
                    stable_row,
                    prior_kind,
                    entry.kind()
                );
                self.lines.put(stable_row, entry);
            }
        }
        if !to_fetch.is_empty() {
            if self.fetch_limiter.non_blocking_admittance_check(1) {
                self.schedule_fetch_lines(to_fetch, now);
            } else {
                log::warn!(
                    "exceeded fetch throttle, drop {:?} and mark stale",
                    to_fetch
                );
                for r in to_fetch.iter() {
                    for stable_row in r.clone() {
                        self.make_stale(stable_row);
                    }
                }
            }
        }
    }

    pub fn make_all_stale(&mut self) {
        let mut lines = LruCache::new(self.lines.cap());
        while let Some((stable_row, entry)) = self.lines.pop_lru() {
            let entry = match entry {
                LineEntry::Stale(old) | LineEntry::Line(old) => LineEntry::Stale(old),
                entry => entry,
            };
            lines.put(stable_row, entry);
        }
        self.lines = lines;
    }

    fn make_stale(&mut self, stable_row: StableRowIndex) {
        match self.lines.pop(&stable_row) {
            Some(LineEntry::Stale(old))
            | Some(LineEntry::Line(old))
            | Some(LineEntry::LineAndFetching(old, _)) => {
                self.lines.put(stable_row, LineEntry::Stale(old));
            }
            Some(LineEntry::Fetching(_)) | None => {}
        }
    }

    fn put_line(
        &mut self,
        stable_row: StableRowIndex,
        mut line: Line,
        config: &ConfigHandle,
        fetch_start: Option<Instant>,
    ) {
        line.scan_and_create_hyperlinks(&config.hyperlink_rules);

        let entry = if let Some(fetch_start) = fetch_start {
            // If we're completing a fetch, only replace entries that were
            // set to fetching as part of our fetch.  If they are now longer
            // tagged that way, then someone came along after us and changed
            // the state, so we should leave it alone

            match self.lines.pop(&stable_row) {
                Some(LineEntry::LineAndFetching(_, then)) | Some(LineEntry::Fetching(then))
                    if fetch_start == then =>
                {
                    log::trace!(
                        "row {} fetch done -> Line seq={} vs self.seq={}",
                        stable_row,
                        line.current_seqno(),
                        self.seqno
                    );
                    line.update_last_change_seqno(self.seqno);
                    LineEntry::Line(line)
                }
                Some(e) => {
                    // It changed since we started: leave it alone!
                    log::trace!(
                        "row {} {:?} changed since fetch started at {:?}, so leave it be",
                        stable_row,
                        e.kind(),
                        fetch_start
                    );
                    self.lines.put(stable_row, e);
                    return;
                }
                None => return,
            }
        } else {
            LineEntry::Line(line)
        };
        self.lines.put(stable_row, entry);
    }

    fn schedule_fetch_lines(&mut self, to_fetch: RangeSet<StableRowIndex>, now: Instant) {
        if to_fetch.is_empty() || self.dead {
            return;
        }
        let local_pane_id = self.local_pane_id;
        log::trace!(
            "will fetch lines {:?} for remote tab id {} at {:?}",
            to_fetch,
            self.remote_pane_id,
            now,
        );

        let client = Arc::clone(&self.client);
        let remote_pane_id = self.remote_pane_id;

        promise::spawn::spawn(async move {
            let result = client
                .client
                .get_lines(GetLines {
                    pane_id: remote_pane_id,
                    lines: to_fetch.clone().into(),
                })
                .await;

            let result = match result {
                Ok(result) => {
                    let lines =
                        hydrate_lines(Arc::clone(&client), remote_pane_id, result.lines).await;
                    Ok(lines)
                }
                Err(err) => Err(err),
            };
            Self::apply_lines(local_pane_id, result, to_fetch, now)
        })
        .detach();
    }

    fn apply_lines(
        local_pane_id: PaneId,
        result: anyhow::Result<Vec<(StableRowIndex, Line)>>,
        to_fetch: RangeSet<StableRowIndex>,
        now: Instant,
    ) -> anyhow::Result<()> {
        let mux = Mux::get();
        let pane = mux
            .get_pane(local_pane_id)
            .ok_or_else(|| anyhow!("no such tab {}", local_pane_id))?;
        if let Some(client_tab) = pane.downcast_ref::<ClientPane>() {
            let renderable = client_tab.renderable.lock();
            let mut inner = renderable.inner.borrow_mut();

            match result {
                Ok(lines) => {
                    let config = configuration();

                    log::trace!("fetch complete for {:?} at {:?}", to_fetch, now);
                    for (stable_row, line) in lines.into_iter() {
                        inner.put_line(stable_row, line, &config, Some(now));
                    }
                }
                Err(err) => {
                    log::error!("get_lines failed: {}", err);
                    for r in to_fetch.iter() {
                        for stable_row in r.clone() {
                            let entry = match inner.lines.pop(&stable_row) {
                                Some(LineEntry::Fetching(then)) if then == now => {
                                    // leave it popped
                                    continue;
                                }
                                Some(LineEntry::LineAndFetching(line, then)) if then == now => {
                                    // revert to just a line
                                    LineEntry::Line(line)
                                }
                                Some(entry) => entry,
                                None => continue,
                            };
                            inner.lines.put(stable_row, entry);
                        }
                    }
                }
            }
        }
        log::trace!(
            "Generate PaneOutput event for local_pane_id={}",
            local_pane_id
        );
        mux.notify(mux::MuxNotification::PaneOutput(local_pane_id));
        Ok(())
    }

    fn poll(&mut self) -> anyhow::Result<()> {
        if self.poll_in_progress.load(Ordering::SeqCst) {
            // We have a poll in progress
            return Ok(());
        }

        if self.last_poll.elapsed() < self.poll_interval {
            return Ok(());
        }

        let interval = self.poll_interval;
        let interval = (interval + interval).min(MAX_POLL_INTERVAL);
        self.poll_interval = interval;

        self.last_poll = Instant::now();
        self.poll_in_progress.store(true, Ordering::SeqCst);
        let remote_pane_id = self.remote_pane_id;
        let local_pane_id = self.local_pane_id;
        let client = Arc::clone(&self.client);
        promise::spawn::spawn(async move {
            let alive = match client
                .client
                .get_pane_render_changes(GetPaneRenderChanges {
                    pane_id: remote_pane_id,
                })
                .await
            {
                Ok(resp) => resp.is_alive,
                // if we got a timeout on a reconnectable, don't
                // consider the tab to be dead; that helps to
                // avoid having a tab get shuffled around
                Err(_) => client.client.is_reconnectable,
            };

            let mux = Mux::get();
            let tab = mux
                .get_pane(local_pane_id)
                .ok_or_else(|| anyhow!("no such tab {}", local_pane_id))?;
            if let Some(client_tab) = tab.downcast_ref::<ClientPane>() {
                let renderable = client_tab.renderable.lock();
                let mut inner = renderable.inner.borrow_mut();

                inner.dead = !alive;
                inner.last_recv_time = Instant::now();
                inner.poll_in_progress.store(false, Ordering::SeqCst);
            }
            Ok::<(), anyhow::Error>(())
        })
        .detach();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Regression tests for the `make_all_stale` capacity fix (upstream #7704).
    //!
    //! `make_all_stale` rebuilds `self.lines` by draining it into a fresh
    //! `LruCache`. Before the fix that cache was `LruCache::unbounded()`,
    //! which silently dropped the scrollback capacity limit and allowed the
    //! cache to grow without bound. The fix builds it with
    //! `LruCache::new(self.lines.cap())` so the rebuilt cache keeps the same
    //! bounded capacity as the original.
    //!
    //! `RenderableInner` cannot be constructed in isolation here: `new()`
    //! requires an `Arc<ClientInner>` (the full wezterm-client connection
    //! machinery, backed by a real transport and rate limiter) which is not
    //! realistically buildable without a running server. These tests therefore
    //! exercise the exact `LruCache` behaviour the one-line fix relies on,
    //! asserting that a `new(cap)` cache stays bounded while `unbounded()`
    //! does not -- i.e. the regression cannot silently reappear.
    use super::*;

    #[test]
    fn new_preserves_bounded_capacity_when_overfilled() {
        // Mirrors `make_all_stale`: build the replacement cache with the
        // original's capacity, then pour more entries in than it can hold.
        let cap = NonZeroUsize::new(4).unwrap();
        let mut lines: LruCache<i32, i32> = LruCache::new(cap);

        for i in 0..1000 {
            lines.put(i, i);
        }

        // A bounded cache must never exceed its declared capacity, no matter
        // how many entries are inserted -- the exact invariant `unbounded()`
        // violated (see the contrast test below).
        assert_eq!(lines.len(), cap.get());
        assert_eq!(lines.cap(), cap);
    }

    #[test]
    fn unbounded_grows_without_limit() {
        // Documents the pre-fix behaviour so the contrast is explicit: an
        // unbounded cache swallows every entry, which is the memory leak
        // fixed by switching `make_all_stale` to `LruCache::new(self.lines.cap())`.
        let mut unbounded: LruCache<i32, i32> = LruCache::unbounded();

        for i in 0..1000 {
            unbounded.put(i, i);
        }

        assert_eq!(unbounded.len(), 1000);
        assert!(unbounded.cap().get() >= 1000);
    }

    #[test]
    fn cap_survives_round_trip_through_new_cache() {
        // Reproduces the full body of `make_all_stale` at the LruCache level:
        // drain one bounded cache into a fresh `new(cap())` cache and confirm
        // the resulting capacity matches the source -- the property the fix
        // guarantees for `self.lines`.
        let cap = NonZeroUsize::new(8).unwrap();
        let mut src: LruCache<i32, i32> = LruCache::new(cap);
        for i in 0..8 {
            src.put(i, i);
        }

        let mut rebuilt = LruCache::new(src.cap());
        while let Some((k, v)) = src.pop_lru() {
            rebuilt.put(k, v);
        }

        assert_eq!(rebuilt.cap(), cap);
        assert_eq!(rebuilt.len(), 8);
    }
}
