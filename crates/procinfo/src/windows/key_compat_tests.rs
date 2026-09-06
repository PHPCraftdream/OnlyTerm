use super::*;

#[test]
fn packed_names_copy_only_live_utf16_and_keep_unicode_names() {
    let mut snapshot = SnapshotExeEntries::default();
    let raw = entry(1, 0, "程序😀.exe");
    snapshot.push(raw.pid, raw.ppid, &raw.exe);
    let expected: Vec<u16> = "程序😀.exe".encode_utf16().collect();
    assert_eq!(snapshot.names, expected);
    for pid in 2..302 {
        snapshot.push(pid, 1, &entry(pid, 1, "cmd.exe").exe);
    }
    let names = super::exe_names_from_entries(&snapshot, 1).unwrap();
    assert!(names.contains("程序😀.exe"));
    assert!(names.contains("cmd.exe"));
    let packed_bytes = snapshot.entries.capacity() * std::mem::size_of::<SnapshotExeEntry>()
        + snapshot.names.capacity() * std::mem::size_of::<u16>();
    assert!(packed_bytes < snapshot.entries.len() * std::mem::size_of::<[u16; MAX_PATH]>());
}

#[test]
fn cached_snapshots_share_storage_and_failed_refresh_preserves_generation() {
    let cache = Mutex::new(None);
    let now = Instant::now();
    let first = snapshot_entries_with(
        &cache,
        || now,
        || {
            Ok(vec![ProcessEntry {
                pid: 1,
                ppid: 0,
                exe: PathBuf::from("shell.exe"),
            }])
        },
    );
    let warm = snapshot_entries_with(&cache, || now, || panic!("warm snapshot must not refresh"));
    assert!(Arc::ptr_eq(&first, &warm));
    let later = now + PROC_SNAPSHOT_TTL;
    let stale = snapshot_entries_with(
        &cache,
        || later,
        || Err(io::Error::other("snapshot unavailable")),
    );
    assert!(Arc::ptr_eq(&first, &stale));
    let replaced = snapshot_entries_with(&cache, || later, || Ok(Vec::new()));
    assert!(replaced.is_empty());
    assert_eq!(first[0].exe, PathBuf::from("shell.exe"));
}

#[test]
fn cwd_only_lookup_does_not_read_command_line() {
    assert!(
        read_optional_argv(false, || panic!("cwd lookup must not read argv"))
            .unwrap()
            .is_empty()
    );
    assert!(read_optional_argv(true, || None).is_none());
    let command = "shell.exe arg\0".encode_utf16().collect();
    assert_eq!(
        read_optional_argv(true, || Some(command)).unwrap(),
        ["shell.exe", "arg"]
    );
}

struct TestExeEntry {
    pid: u32,
    ppid: u32,
    exe: [u16; MAX_PATH],
}

fn names_for_entries(entries: &[TestExeEntry], root: u32) -> io::Result<HashSet<String>> {
    let mut snapshot = SnapshotExeEntries::default();
    for entry in entries {
        snapshot.push(entry.pid, entry.ppid, &entry.exe);
    }
    super::exe_names_from_entries(&snapshot, root)
}

fn entry(pid: u32, ppid: u32, exe: &str) -> TestExeEntry {
    let mut encoded = [0u16; MAX_PATH];
    let wide: Vec<u16> = exe.encode_utf16().collect();
    let copy_len = wide.len().min(encoded.len().saturating_sub(1));
    encoded[..copy_len].copy_from_slice(&wide[..copy_len]);
    TestExeEntry {
        pid,
        ppid,
        exe: encoded,
    }
}

#[test]
fn wchar_read_size_validation_rejects_malformed_or_oversized_lengths() {
    assert_eq!(wchar_read_len(0), Some(0));
    assert_eq!(wchar_read_len(MAX_PATH * 4), Some(MAX_PATH * 2));
    assert_eq!(wchar_read_len(1), None);
    assert_eq!(wchar_read_len(MAX_PATH * 4 + 2), None);
}

#[test]
fn short_wchar_reads_are_truncated_to_complete_code_units_and_terminated() {
    assert_eq!(
        finish_wchar_read(vec![b'a' as u16, b'b' as u16], 3),
        vec![b'a' as u16, 0]
    );
    assert_eq!(
        finish_wchar_read(vec![b'a' as u16, 0, b'b' as u16], 6),
        vec![b'a' as u16, 0]
    );
    assert_eq!(finish_wchar_read(Vec::new(), 0), vec![0]);
}

#[test]
fn name_lookup_follows_wrappers_but_excludes_other_panes() {
    // Deliberately not in parent-first order. The younger node_repl
    // must not hide Codex, and Claude in another pane must not leak in.
    let entries = vec![
        entry(9, 8, "node_repl.exe"),
        entry(8, 7, "codex.exe"),
        entry(1, 0, "onlyterm-gui.exe"),
        entry(10, 1, "cmd.exe"),
        entry(11, 10, "claude.exe"),
        entry(2, 1, "cmd.exe"),
        entry(3, 2, "powershell.exe"),
        entry(4, 3, "cmd.exe"),
        entry(5, 4, "powershell.exe"),
        entry(6, 5, "cmd.exe"),
        entry(7, 6, "node.exe"),
    ];
    let names = names_for_entries(&entries, 2).unwrap();
    assert_eq!(
        names,
        [
            "cmd.exe",
            "powershell.exe",
            "node.exe",
            "codex.exe",
            "node_repl.exe"
        ]
        .into_iter()
        .map(String::from)
        .collect()
    );
    let names = names_for_entries(&entries, 10).unwrap();
    assert_eq!(
        names,
        ["cmd.exe", "claude.exe"]
            .into_iter()
            .map(String::from)
            .collect()
    );
}

#[test]
fn name_lookup_tracks_start_and_exit_in_each_snapshot() {
    let mut entries = vec![entry(1, 0, "cmd.exe")];
    assert!(!names_for_entries(&entries, 1)
        .unwrap()
        .contains("codex.exe"));
    entries.push(entry(2, 1, "codex.exe"));
    assert!(names_for_entries(&entries, 1)
        .unwrap()
        .contains("codex.exe"));
    entries.pop();
    assert!(!names_for_entries(&entries, 1)
        .unwrap()
        .contains("codex.exe"));
}

#[test]
fn name_lookup_missing_root_is_unknown_not_empty_tree() {
    let entries = [entry(2, 1, "codex.exe")];
    assert_eq!(
        names_for_entries(&entries, 1).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn name_lookup_handles_cycles_and_deep_wrapper_chains() {
    let mut entries = vec![entry(1, 2, "cmd.exe"), entry(2, 1, "codex.exe")];
    for pid in 3..10_000 {
        entries.push(entry(pid, pid - 1, "powershell.exe"));
    }
    entries.push(entry(10_000, 9_999, "node_repl.exe"));
    let names = names_for_entries(&entries, 1).unwrap();
    assert!(names.contains("codex.exe"));
    assert!(names.contains("node_repl.exe"));
    assert_eq!(names.len(), 4);
}
