use super::*;

#[test]
fn snapshot_start_is_recorded_before_fetch_not_when_it_finishes() {
    use std::cell::Cell;
    let at = Instant::now();
    let clock = Cell::new(at);
    let cache = Mutex::new(ProcessSnapshotState::new());
    let entries = snapshot_entries_with(
        &cache,
        || clock.get(),
        |_| {
            clock.set(at + Duration::from_secs(1));
            Ok(())
        },
    );
    assert_eq!(entries.started_at, at);
    let reused = snapshot_entries_with(&cache, || clock.get(), |_| panic!("cache remains warm"));
    assert!(Arc::ptr_eq(&entries, &reused));
    assert_eq!(reused.started_at, at);
}

/// One name buffer for the whole machine, and the range for an entry
/// round-trips back to the path a caller would have gotten from an
/// owned `PathBuf` per process.
#[test]
fn packed_names_round_trip_to_the_same_path() {
    let mut entries = ProcessEntries::new(Instant::now());
    let mut wide: Vec<u16> = "explorer.exe".encode_utf16().collect();
    wide.push(0);
    wide.extend(std::iter::repeat_n(0, 8)); // trailing garbage after the NUL
    entries.push(7, 1, &wide);
    entries.push(9, 7, &"cmd.exe\u{0}".encode_utf16().collect::<Vec<_>>());

    assert_eq!(entries.entries.len(), 2);
    assert!(!entries.names.contains(&0), "packed names must be NUL-free");
    assert_eq!(
        entries.exe_path(&entries.entries[0]),
        PathBuf::from("explorer.exe")
    );
    assert_eq!(
        entries.exe_path(&entries.entries[1]),
        PathBuf::from("cmd.exe")
    );
}

/// The whole point of the double buffer: once a superseded snapshot is
/// dropped by its callers, its allocations come back as the spare and a
/// later refresh refills them instead of growing new ones.
///
/// Reclaim necessarily lags one generation -- a refresh can only take
/// back the generation it just superseded, and the generation it is
/// replacing has to stay alive until the new walk succeeds -- so the
/// steady state is "published generation N plus spare generation N-1",
/// and the reuse is observable from the third refresh on.
#[test]
fn superseded_snapshot_buffers_are_reclaimed_and_refilled_in_place() {
    use std::cell::Cell;
    let at = Instant::now();
    let clock = Cell::new(at);
    let cache = Mutex::new(ProcessSnapshotState::new());

    let fill = |dest: &mut ProcessEntries| {
        for pid in 0..64u32 {
            dest.push(pid, 0, &"a.exe\u{0}".encode_utf16().collect::<Vec<_>>());
        }
        Ok(())
    };

    let first = snapshot_entries_with(&cache, || clock.get(), fill);
    let first_entries_ptr = first.entries.as_ptr();
    let first_capacity = first.entries.capacity();
    drop(first);

    clock.set(at + PROC_SNAPSHOT_TTL);
    let second = snapshot_entries_with(&cache, || clock.get(), fill);
    drop(second);

    clock.set(at + PROC_SNAPSHOT_TTL * 2);
    let third = snapshot_entries_with(
        &cache,
        || clock.get(),
        |dest| {
            assert!(
                dest.entries.capacity() >= first_capacity,
                "refresh should have been handed the reclaimed buffer"
            );
            assert!(dest.entries.is_empty(), "reused buffer must be reset first");
            dest.push(1, 0, &"b.exe\u{0}".encode_utf16().collect::<Vec<_>>());
            Ok(())
        },
    );
    assert_eq!(
        third.entries.as_ptr(),
        first_entries_ptr,
        "the reclaimed allocation should be the one refilled"
    );
    assert_eq!(third.entries.len(), 1);
}

/// A snapshot still held by a caller cannot be reclaimed, so the
/// refresh allocates rather than mutating data someone is reading.
#[test]
fn a_snapshot_still_held_by_a_caller_is_never_reused_as_the_spare() {
    use std::cell::Cell;
    let at = Instant::now();
    let clock = Cell::new(at);
    let cache = Mutex::new(ProcessSnapshotState::new());

    let held = snapshot_entries_with(
        &cache,
        || clock.get(),
        |dest| {
            dest.push(1, 0, &"a.exe\u{0}".encode_utf16().collect::<Vec<_>>());
            Ok(())
        },
    );

    clock.set(at + PROC_SNAPSHOT_TTL);
    let next = snapshot_entries_with(
        &cache,
        || clock.get(),
        |dest| {
            dest.push(2, 0, &"b.exe\u{0}".encode_utf16().collect::<Vec<_>>());
            Ok(())
        },
    );

    // `held` is still alive, so the refresh must not have written into it.
    assert_eq!(held.entries.len(), 1);
    assert_eq!(held.entries[0].pid, 1);
    assert_eq!(next.entries[0].pid, 2);
    let state = cache.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        state.spare.is_none(),
        "a snapshot a caller still holds must not be handed out as scratch"
    );
}

/// A walk that fails partway must not become what callers see: the
/// previous complete snapshot stays published, partial data stays
/// private.
#[test]
fn a_failed_walk_keeps_serving_the_last_complete_snapshot() {
    use std::cell::Cell;
    let at = Instant::now();
    let clock = Cell::new(at);
    let cache = Mutex::new(ProcessSnapshotState::new());

    let complete = snapshot_entries_with(
        &cache,
        || clock.get(),
        |dest| {
            dest.push(1, 0, &"a.exe\u{0}".encode_utf16().collect::<Vec<_>>());
            dest.push(2, 1, &"b.exe\u{0}".encode_utf16().collect::<Vec<_>>());
            Ok(())
        },
    );
    assert_eq!(complete.entries.len(), 2);

    clock.set(at + PROC_SNAPSHOT_TTL);
    let after_failure = snapshot_entries_with(
        &cache,
        || clock.get(),
        |dest| {
            dest.push(3, 0, &"partial.exe\u{0}".encode_utf16().collect::<Vec<_>>());
            Err(io::Error::other("walk failed partway"))
        },
    );
    assert_eq!(
        after_failure.entries.len(),
        2,
        "a partial walk must never be published"
    );
    assert_eq!(after_failure.entries[0].pid, 1);
}

/// After a failure the walk is not retried at the TTL cadence: under
/// the memory pressure that caused the failure, every retry is another
/// chance for an allocation to abort the process.
#[test]
fn a_failed_walk_is_not_retried_until_the_backoff_expires() {
    use std::cell::Cell;
    let at = Instant::now();
    let clock = Cell::new(at);
    let cache = Mutex::new(ProcessSnapshotState::new());

    snapshot_entries_with(
        &cache,
        || clock.get(),
        |dest| {
            dest.push(1, 0, &"a.exe\u{0}".encode_utf16().collect::<Vec<_>>());
            Ok(())
        },
    );

    let failed_at = at + PROC_SNAPSHOT_TTL;
    clock.set(failed_at);
    snapshot_entries_with(
        &cache,
        || clock.get(),
        |_| Err(io::Error::other("out of memory")),
    );

    // Well past the TTL, still inside the backoff: no walk attempted.
    clock.set(at + PROC_SNAPSHOT_TTL * 4);
    let stale = snapshot_entries_with(
        &cache,
        || clock.get(),
        |_| panic!("must not retry while backing off"),
    );
    assert_eq!(stale.entries.len(), 1);

    // The backoff is measured from the failure, not from the last
    // successful refresh.
    clock.set(failed_at + PROC_SNAPSHOT_FAILURE_BACKOFF + Duration::from_millis(1));
    let refreshed = snapshot_entries_with(
        &cache,
        || clock.get(),
        |dest| {
            dest.push(2, 0, &"b.exe\u{0}".encode_utf16().collect::<Vec<_>>());
            Ok(())
        },
    );
    assert_eq!(refreshed.entries[0].pid, 2);
}

#[test]
fn activity_stamp_distinguishes_exited_process_even_with_exit_code_259() {
    use std::os::windows::process::CommandExt;
    let mut child = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "exit 259"])
        .creation_flags(winapi::um::winbase::CREATE_NO_WINDOW)
        .spawn()
        .expect("short-lived process fixture");
    let pid = child.id();
    assert_eq!(child.wait().expect("fixture exit").code(), Some(259));
    assert_eq!(LocalProcessInfo::activity_stamp(pid), None);
}

#[test]
fn activity_stamp_reads_identity_without_a_tree_snapshot() {
    let stamp = LocalProcessInfo::activity_stamp(std::process::id()).expect("own process activity");
    assert!(stamp.creation_time > 0);
    assert_eq!(LocalProcessInfo::activity_stamp(u32::MAX), None);
}

#[test]
fn process_tree_resource_usage_reads_the_current_process() {
    // SAFETY: GetCurrentProcessId has no preconditions and no UB.
    let my_pid = unsafe { GetCurrentProcessId() };
    let usage = LocalProcessInfo::process_tree_resource_usage(my_pid)
        .expect("current process must be in its own snapshot");
    assert!(usage.process_count >= 1);
    // A freshly-started test process has done *some* CPU work (at least
    // process startup) and has a non-zero working set by the time this
    // assertion runs.
    assert!(usage.total_working_set_bytes > 0);
}

#[test]
fn process_tree_resource_usage_rejects_an_unknown_pid() {
    // Windows PIDs are always multiples of 4 and, in practice, far
    // smaller than u32::MAX, so this can never collide with a live
    // process and reliably exercises the not-found path.
    assert!(LocalProcessInfo::process_tree_resource_usage(u32::MAX).is_err());
}

#[test]
fn total_physical_memory_bytes_is_populated() {
    assert!(total_physical_memory_bytes() > 0);
}
