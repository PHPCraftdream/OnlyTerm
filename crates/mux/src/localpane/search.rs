//! Bounded, lock-minimized scrollback search for local panes.

use super::*;
use fancy_regex::Regex;
use smol::channel::{bounded, Receiver, Sender};
use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

const SEARCH_BATCH_MAX_PHYSICAL_ROWS: usize = 256;
const SEARCH_WORKER_COUNT: usize = 2;

fn permit_pool(count: usize) -> (Sender<()>, Receiver<()>) {
    let (sender, receiver) = bounded(count);
    for _ in 0..count {
        sender
            .try_send(())
            .expect("search permit channel has capacity");
    }
    (sender, receiver)
}

static SEARCH_PERMITS: LazyLock<(Sender<()>, Receiver<()>)> =
    LazyLock::new(|| permit_pool(SEARCH_WORKER_COUNT));

struct PermitReturn(Sender<()>);

async fn acquire_search_permit() -> anyhow::Result<PermitReturn> {
    acquire_permit_from(&SEARCH_PERMITS).await
}

async fn acquire_permit_from(pool: &(Sender<()>, Receiver<()>)) -> anyhow::Result<PermitReturn> {
    pool.1
        .recv()
        .await
        .map_err(|_| anyhow::anyhow!("search worker permits closed"))?;
    Ok(PermitReturn(pool.0.clone()))
}

impl Drop for PermitReturn {
    fn drop(&mut self) {
        let _ = self.0.try_send(());
    }
}

struct LowercaseOffsets {
    text: String,
    starts: Vec<usize>,
    ends: Vec<usize>,
}

fn lowercase_with_offsets(input: &str) -> LowercaseOffsets {
    // Use the string implementation so contextual rules such as Greek final
    // sigma (`ΟΣ` -> `ος`) are preserved. Per-character lowercase lengths are
    // used only to associate each output byte with its source character.
    let text = input.to_lowercase();
    let mut starts = Vec::with_capacity(text.len() + 1);
    let mut ends = Vec::with_capacity(text.len() + 1);
    let mut output_offset = 0;

    for (source_start, ch) in input.char_indices() {
        let source_end = source_start + ch.len_utf8();
        let output_len = ch.to_lowercase().map(char::len_utf8).sum::<usize>();
        let output_end = output_offset + output_len;
        debug_assert!(output_end <= text.len());
        for _ in output_offset..output_end {
            starts.push(source_start);
            ends.push(source_end);
        }
        output_offset = output_end;
    }
    debug_assert_eq!(output_offset, text.len());
    starts.push(input.len());
    ends.push(input.len());

    LowercaseOffsets { text, starts, ends }
}

fn search_limit_allows_result(current: usize, limit: Option<usize>) -> bool {
    limit.is_none_or(|limit| current < limit)
}

enum CompiledPattern {
    CaseSensitiveString(String),
    CaseInSensitiveString(String),
    Regex(Regex),
}

#[derive(Copy, Clone, Debug)]
struct Coord {
    byte_idx: usize,
    byte_end: usize,
    grapheme_idx: usize,
    width: usize,
    stable_row: StableRowIndex,
}

struct PendingMatch {
    text: String,
    start_x: usize,
    start_y: StableRowIndex,
    end_x: usize,
    end_y: StableRowIndex,
}

type SnapshotBatch = Vec<(Range<StableRowIndex>, Vec<Line>)>;

struct PhysicalSnapshot {
    stable_start: StableRowIndex,
    next_cursor: StableRowIndex,
    lines: Vec<Line>,
}

pub(super) async fn search(
    pane: &LocalPane,
    pattern: Pattern,
    range: Range<StableRowIndex>,
    limit: Option<u32>,
) -> anyhow::Result<Vec<SearchResult>> {
    let limit = limit.map(|limit| limit as usize);
    if limit == Some(0) {
        return Ok(Vec::new());
    }
    let permit = acquire_search_permit().await?;
    let pattern = smol::unblock(move || {
        let _permit = permit;
        let pattern = match pattern {
            Pattern::CaseSensitiveString(s) => CompiledPattern::CaseSensitiveString(s),
            Pattern::CaseInSensitiveString(s) => {
                CompiledPattern::CaseInSensitiveString(s.to_lowercase())
            }
            Pattern::Regex(r) => CompiledPattern::Regex(Regex::new(&r)?),
        };
        Ok::<_, anyhow::Error>(Arc::new(pattern))
    })
    .await?;
    let mut cursor = range.start;
    let mut results = Vec::new();
    let mut uniq_matches: HashMap<String, usize> = HashMap::new();

    while cursor < range.end {
        // Hold one permit while capturing a logical line that may span many
        // physical chunks. Cancellation drops this future and releases the
        // permit before any worker is started.
        let permit_return = acquire_search_permit().await?;

        let (complete_batch, next_cursor) = capture_logical_batch(pane, cursor, range.end).await?;
        if complete_batch.is_empty() {
            break;
        }

        // The worker pool is globally bounded. If this search is cancelled,
        // the blocking worker may finish its current batch, but its permit is
        // returned by PermitReturn and no unbounded worker backlog forms.
        let worker_pattern = Arc::clone(&pattern);
        let remaining_limit = limit.map(|n| n.saturating_sub(results.len()));
        let pending = smol::unblock(move || {
            let _permit_return = permit_return;
            process_batch(complete_batch, &worker_pattern, remaining_limit)
        })
        .await;

        for pending in pending {
            if !search_limit_allows_result(results.len(), limit) {
                break;
            }
            let match_id = match uniq_matches.get(&pending.text).copied() {
                Some(id) => id,
                None => {
                    let id = uniq_matches.len();
                    uniq_matches.insert(pending.text, id);
                    id
                }
            };
            results.push(SearchResult {
                start_x: pending.start_x,
                start_y: pending.start_y,
                end_x: pending.end_x,
                end_y: pending.end_y,
                match_id,
            });
        }

        if !search_limit_allows_result(results.len(), limit) || next_cursor <= cursor {
            break;
        }
        cursor = next_cursor;
    }

    Ok(results)
}

// Waiting is cancellation-safe: no lock is held across an await.
async fn snapshot_physical_batch(
    pane: &LocalPane,
    cursor: StableRowIndex,
    end: StableRowIndex,
) -> anyhow::Result<Option<PhysicalSnapshot>> {
    loop {
        let captured = {
            pane.terminal.try_lock().map(|term| {
                let screen = term.screen();
                let start = cursor.max(screen.phys_to_stable_row_index(0));
                let end = end.min(screen.phys_to_stable_row_index(screen.scrollback_rows()));
                if start >= end {
                    return None;
                }
                let phys_range = screen.stable_range(&(start..end));
                if phys_range.start >= phys_range.end {
                    return None;
                }
                let chunk_end =
                    (phys_range.start + SEARCH_BATCH_MAX_PHYSICAL_ROWS).min(phys_range.end);
                Some(PhysicalSnapshot {
                    stable_start: screen.phys_to_stable_row_index(phys_range.start),
                    next_cursor: screen.phys_to_stable_row_index(chunk_end),
                    lines: screen.lines_in_phys_range(phys_range.start..chunk_end),
                })
            })
        };
        if let Some(captured) = captured {
            return Ok(captured);
        }
        smol::Timer::after(std::time::Duration::from_millis(2)).await;
    }
}

async fn capture_logical_batch(
    pane: &LocalPane,
    cursor: StableRowIndex,
    end: StableRowIndex,
) -> anyhow::Result<(SnapshotBatch, StableRowIndex)> {
    let Some(snapshot) = snapshot_physical_batch(pane, cursor, end).await? else {
        return Ok((Vec::new(), cursor));
    };
    let mut next_cursor = snapshot.next_cursor;
    let (mut stable_start, mut lines) =
        prepend_wrapped_prefix(pane, snapshot.stable_start, snapshot.lines).await?;
    let mut pending_start = stable_start;
    let mut pending = Vec::new();
    let mut complete = Vec::new();
    loop {
        for (offset, line) in lines.into_iter().enumerate() {
            let row = stable_start + offset as StableRowIndex;
            if row >= end && pending.is_empty() {
                return Ok((complete, row));
            }
            let wrapped = line.last_cell_was_wrapped();
            pending.push(line);
            if !wrapped {
                let logical_end = row + 1;
                complete.push((pending_start..logical_end, std::mem::take(&mut pending)));
                pending_start = logical_end;
                if complete.len() >= 64 || logical_end >= end {
                    return Ok((complete, logical_end));
                }
            }
        }
        if pending.is_empty() {
            return Ok((complete, next_cursor));
        }
        smol::future::yield_now().await;
        // Preserve the old logical-line range semantics: a requested physical
        // range may end inside a wrapped line, whose suffix must also match.
        let Some(snapshot) = snapshot_physical_batch(pane, next_cursor, isize::MAX).await? else {
            complete.push((pending_start..next_cursor, pending));
            return Ok((complete, next_cursor));
        };
        if snapshot.stable_start != next_cursor {
            // The old prefix was evicted while yielded. Do not join it to an
            // unrelated surviving suffix or fail the rest of the search.
            pending.clear();
            pending_start = snapshot.stable_start;
        }
        stable_start = snapshot.stable_start;
        next_cursor = snapshot.next_cursor;
        lines = snapshot.lines;
    }
}

async fn prepend_wrapped_prefix(
    pane: &LocalPane,
    stable_start: StableRowIndex,
    lines: Vec<Line>,
) -> anyhow::Result<(StableRowIndex, Vec<Line>)> {
    let mut start = stable_start;
    let mut prefix_chunks = Vec::new();
    loop {
        if start <= 0 {
            break;
        }
        let previous = snapshot_physical_batch(
            pane,
            start
                .saturating_sub(SEARCH_BATCH_MAX_PHYSICAL_ROWS as StableRowIndex)
                .max(0),
            start,
        )
        .await?;
        let Some(previous) = previous else {
            break;
        };
        if previous.next_cursor != start || previous.stable_start >= start {
            break;
        }
        let mut previous_lines = previous.lines;
        let keep_from = previous_lines
            .iter()
            .rposition(|line| !line.last_cell_was_wrapped())
            .map_or(0, |idx| idx + 1);
        start = previous.stable_start + keep_from as StableRowIndex;
        prefix_chunks.push(previous_lines.split_off(keep_from));
        if keep_from > 0 {
            break;
        }
        smol::future::yield_now().await;
    }
    let mut combined = Vec::new();
    for chunk in prefix_chunks.into_iter().rev() {
        combined.extend(chunk);
    }
    combined.extend(lines);
    Ok((start, combined))
}

fn process_batch(
    batch: SnapshotBatch,
    pattern: &CompiledPattern,
    limit: Option<usize>,
) -> Vec<PendingMatch> {
    let mut pending = Vec::new();
    for (stable_range, owned_lines) in batch {
        if owned_lines.is_empty() {
            continue;
        }
        let lines: Vec<&Line> = owned_lines.iter().collect();
        let haystack = if lines.len() == 1 {
            lines[0].as_str()
        } else {
            let mut text = String::new();
            for line in &lines {
                text.push_str(&line.as_str());
            }
            Cow::Owned(text)
        };
        if haystack.is_empty() {
            continue;
        }

        let stable_idx = stable_range.start;
        let mut offsets = None;
        let haystack = match pattern {
            CompiledPattern::CaseInSensitiveString(_) if haystack.is_ascii() => {
                Cow::Owned(haystack.to_ascii_lowercase())
            }
            CompiledPattern::CaseInSensitiveString(_) => {
                let LowercaseOffsets { text, starts, ends } =
                    lowercase_with_offsets(haystack.as_ref());
                offsets = Some((starts, ends));
                Cow::Owned(text)
            }
            _ => haystack,
        };
        let mut coords = None;

        match pattern {
            CompiledPattern::CaseInSensitiveString(needle)
            | CompiledPattern::CaseSensitiveString(needle) => {
                for (idx, matched) in haystack.match_indices(needle) {
                    let (start, end) = if matched.is_empty() {
                        let position = offsets.as_ref().map_or(idx, |(starts, _)| starts[idx]);
                        (position, position)
                    } else {
                        offsets
                            .as_ref()
                            .map_or((idx, idx + matched.len()), |(starts, ends)| {
                                (starts[idx], ends[idx + matched.len() - 1])
                            })
                    };
                    if !push_pending(
                        matched,
                        start..end,
                        &lines,
                        stable_idx,
                        limit,
                        &mut coords,
                        &mut pending,
                    ) {
                        return pending;
                    }
                }
            }
            CompiledPattern::Regex(regex) => {
                for capture_result in regex.captures_iter(&haystack) {
                    match capture_result {
                        Ok(captures) => {
                            for idx in (0..captures.len()).rev() {
                                if let Some(matched) = captures.get(idx) {
                                    if !push_pending(
                                        matched.as_str(),
                                        matched.start()..matched.end(),
                                        &lines,
                                        stable_idx,
                                        limit,
                                        &mut coords,
                                        &mut pending,
                                    ) {
                                        return pending;
                                    }
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            log::warn!("line {stable_idx} search error: {err}");
                            break;
                        }
                    }
                }
            }
        }
    }
    pending
}

fn push_pending(
    text: &str,
    byte_range: Range<usize>,
    lines: &[&Line],
    stable_idx: StableRowIndex,
    limit: Option<usize>,
    coords: &mut Option<Vec<Coord>>,
    pending: &mut Vec<PendingMatch>,
) -> bool {
    if !search_limit_allows_result(pending.len(), limit) {
        return false;
    }
    if coords.is_none() {
        coords.replace(make_coords(lines, stable_idx));
    }
    let coords = coords.as_ref().unwrap();
    let (start_x, start_y) = haystack_idx_to_coord(byte_range.start, coords, false);
    let (end_x, end_y) = if byte_range.is_empty() {
        (start_x, start_y)
    } else {
        haystack_idx_to_coord(byte_range.end, coords, true)
    };
    pending.push(PendingMatch {
        text: text.to_owned(),
        start_x,
        start_y,
        end_x,
        end_y,
    });
    true
}

fn make_coords(lines: &[&Line], stable_row: StableRowIndex) -> Vec<Coord> {
    let mut byte_idx = 0;
    let mut coords = vec![];
    for (row_idx, line) in lines.iter().enumerate() {
        for cell in line.visible_cells() {
            coords.push(Coord {
                byte_idx,
                byte_end: byte_idx + cell.str().len(),
                grapheme_idx: cell.cell_index(),
                width: cell.width().max(1),
                stable_row: stable_row + row_idx as StableRowIndex,
            });
            byte_idx += cell.str().len();
        }
    }
    coords
}

fn haystack_idx_to_coord(idx: usize, coords: &[Coord], end: bool) -> (usize, StableRowIndex) {
    let index = coords
        .binary_search_by(|entry| entry.byte_idx.cmp(&idx))
        .unwrap_or_else(|index| {
            if !end && index > 0 && idx < coords[index - 1].byte_end {
                index - 1
            } else {
                index
            }
        });
    let coord = coords.get(index).copied().unwrap_or_else(|| {
        let last = coords.last().unwrap();
        Coord {
            grapheme_idx: last.grapheme_idx + last.width,
            ..*last
        }
    });
    (coord.grapheme_idx, coord.stable_row)
}

#[cfg(test)]
mod tests {
    use super::{lowercase_with_offsets, search_limit_allows_result};
    use std::future::Future;

    #[test]
    fn snapshot_waits_cooperatively_and_can_be_cancelled_while_locked() {
        let (pane, _) = super::super::tests::make_pane();
        let guard = pane.terminal.lock();
        let mut snapshot = Box::pin(super::snapshot_physical_batch(&pane, 0, 1));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(snapshot.as_mut().poll(&mut cx).is_pending());
        drop(snapshot);
        drop(guard);
        assert!(smol::block_on(super::snapshot_physical_batch(&pane, 0, 1))
            .unwrap()
            .is_some());
    }

    #[test]
    fn history_eviction_during_wrapped_capture_keeps_surviving_suffix() {
        let (pane, _) = super::super::tests::make_pane();
        pane.terminal
            .lock()
            .advance_bytes(format!("{}END", "a".repeat(80 * 300)).as_bytes());
        let mut capture = Box::pin(super::capture_logical_batch(&pane, 0, isize::MAX));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(capture.as_mut().poll(&mut cx).is_pending());
        pane.terminal.lock().erase_scrollback();
        let earliest = pane.terminal.lock().screen().phys_to_stable_row_index(0);
        assert!(earliest > 256);
        let (batch, next) = smol::block_on(capture).unwrap();
        assert!(next > earliest);
        assert!(batch.iter().all(|(range, _)| range.start >= earliest));
        assert!(batch
            .iter()
            .flat_map(|(_, lines)| lines)
            .any(|line| line.as_str().contains("END")));
    }

    #[test]
    fn cancelling_a_running_worker_keeps_its_admission_until_it_finishes() {
        let pool = super::permit_pool(1);
        let permit = smol::block_on(super::acquire_permit_from(&pool)).unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let worker = smol::unblock(move || {
            let _permit = permit;
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        started_rx.recv().unwrap();
        drop(worker);
        assert!(pool.1.try_recv().is_err());
        release_tx.send(()).unwrap();
        let recovered = smol::block_on(super::acquire_permit_from(&pool)).unwrap();
        drop(recovered);
        assert!(pool.1.try_recv().is_ok());
    }

    #[test]
    fn lowercase_offsets_cover_expanding_unicode_spans() {
        let folded = lowercase_with_offsets("\u{130}x");
        assert_eq!(folded.text, "i\u{307}x");
        assert_eq!(folded.starts, vec![0, 0, 0, 2, 3]);
        assert_eq!(folded.ends, vec![2, 2, 2, 3, 3]);
    }

    #[test]
    fn lowercase_offsets_preserve_contextual_greek_sigma() {
        let folded = lowercase_with_offsets("\u{39f}\u{3a3}");
        assert_eq!(folded.text, "\u{3bf}\u{3c2}");
        assert_eq!(folded.starts, vec![0, 0, 2, 2, 4]);
        assert_eq!(folded.ends, vec![2, 2, 4, 4, 4]);
    }

    #[test]
    fn search_limit_is_checked_before_each_result() {
        assert!(search_limit_allows_result(0, Some(1)));
        assert!(!search_limit_allows_result(1, Some(1)));
        assert!(!search_limit_allows_result(3, Some(1)));
        assert!(search_limit_allows_result(3, None));
    }
}
