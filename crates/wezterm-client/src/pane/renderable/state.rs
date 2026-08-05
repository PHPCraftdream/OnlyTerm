use super::{LineEntry, RenderableInner};
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use promise::BrokenPromise;
use rangeset::*;
use std::cell::RefCell;
use std::ops::Range;
use std::time::{Duration, Instant};
use termwiz::cell::CellAttributes;
use termwiz::color::AnsiColor;
use termwiz::surface::{SequenceNo, SEQ_ZERO};
use wezterm_term::{Line, StableRowIndex};

pub struct RenderableState {
    pub inner: RefCell<RenderableInner>,
}

impl RenderableState {
    pub fn get_cursor_position(&self) -> StableCursorPosition {
        self.inner.borrow().cursor_position
    }

    pub fn get_lines(&self, lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
        let mut inner = self.inner.borrow_mut();
        let mut result = vec![];
        let mut to_fetch = RangeSet::new();
        let now = Instant::now();

        for idx in lines.clone() {
            let entry = match inner.lines.pop(&idx) {
                Some(LineEntry::Line(line)) => {
                    result.push(line.clone());
                    if line.changed_since(inner.seqno) {
                        to_fetch.add(idx);
                        LineEntry::Stale(line)
                    } else {
                        LineEntry::Line(line)
                    }
                }
                Some(LineEntry::LineAndFetching(line, then)) => {
                    result.push(line.clone());
                    LineEntry::LineAndFetching(line, then)
                }
                Some(LineEntry::Fetching(then)) => {
                    result.push(Line::with_width(inner.dimensions.cols, SEQ_ZERO));
                    LineEntry::Fetching(then)
                }
                Some(LineEntry::Stale(line)) => {
                    result.push(line.clone());
                    to_fetch.add(idx);
                    LineEntry::LineAndFetching(line, now)
                }
                None => {
                    result.push(Line::with_width(inner.dimensions.cols, SEQ_ZERO));
                    to_fetch.add(idx);
                    LineEntry::Fetching(now)
                }
            };

            if inner.client.overlay_lag_indicator
                && idx == inner.dimensions.physical_top
                && inner.is_tardy()
            {
                let status = format!(
                    "wezterm: {:.0?}⏳since last response",
                    inner.last_recv_time.elapsed()
                );
                // Right align it in the tab
                let col = inner
                    .dimensions
                    .cols
                    .saturating_sub(wezterm_term::unicode_column_width(&status, None));

                let mut attr = CellAttributes::default();
                attr.set_foreground(AnsiColor::White);
                attr.set_background(AnsiColor::Blue);

                result
                    .last_mut()
                    .unwrap()
                    .overlay_text_with_attribute(col, &status, attr, SEQ_ZERO);
            }

            inner.lines.put(idx, entry);
        }

        log::trace!(
            "get_lines: {:?}, num result lines={}, will fetch {:?}",
            lines,
            result.len(),
            to_fetch
        );

        inner.schedule_fetch_lines(to_fetch, now);
        (lines.start, result)
    }

    pub fn get_current_seqno(&self) -> SequenceNo {
        self.inner.borrow().seqno
    }

    pub fn get_changed_since(
        &self,
        lines: Range<StableRowIndex>,
        seqno: SequenceNo,
    ) -> RangeSet<StableRowIndex> {
        let mut inner = self.inner.borrow_mut();
        if let Err(err) = inner.poll() {
            // We allow for BrokenPromise here for now; for a TLS backed
            // session it indicates that we'll retry.  For a local unix
            // domain session it is terminal... but we will detect that
            // terminal condition elsewhere
            if let Err(err) = err.downcast::<BrokenPromise>() {
                log::error!("remote tab poll failed: {}, marking as dead", err);
                inner.dead = true;
            }
        }

        let mut result = RangeSet::new();
        for r in lines {
            match inner.lines.get(&r) {
                None => {
                    result.add(r);
                }
                Some(
                    LineEntry::Line(line)
                    | LineEntry::Stale(line)
                    | LineEntry::LineAndFetching(line, _),
                ) if line.changed_since(seqno) => {
                    result.add(r);
                }
                _ => {}
            }
        }

        // If we're behind receiving an update, invalidate the top row so
        // that the indicator will update in a more timely fashion
        if inner.is_tardy() {
            // ... but take care to avoid always reporting it as dirty, so
            // that we don't end up busy looping just to repaint it
            if inner.last_late_dirty.elapsed() >= Duration::from_secs(1) {
                result.add(inner.dimensions.physical_top);
                inner.last_late_dirty = Instant::now();
            }
        }

        if !result.is_empty() {
            log::trace!("get_changed_since: {} -> {:?}", seqno, result);
        }

        result
    }

    pub fn get_dimensions(&self) -> RenderableDimensions {
        self.inner.borrow().dimensions
    }
}