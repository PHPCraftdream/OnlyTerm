use super::*;

fn entry(pid: u32, ppid: u32, exe: &str) -> SnapshotExeEntry {
    let mut encoded = [0u16; MAX_PATH];
    let wide: Vec<u16> = exe.encode_utf16().collect();
    let copy_len = wide.len().min(encoded.len().saturating_sub(1));
    encoded[..copy_len].copy_from_slice(&wide[..copy_len]);
    SnapshotExeEntry {
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
    let names = exe_names_from_entries(&entries, 2).unwrap();
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
    let names = exe_names_from_entries(&entries, 10).unwrap();
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
    assert!(!exe_names_from_entries(&entries, 1)
        .unwrap()
        .contains("codex.exe"));
    entries.push(entry(2, 1, "codex.exe"));
    assert!(exe_names_from_entries(&entries, 1)
        .unwrap()
        .contains("codex.exe"));
    entries.pop();
    assert!(!exe_names_from_entries(&entries, 1)
        .unwrap()
        .contains("codex.exe"));
}

#[test]
fn name_lookup_missing_root_is_unknown_not_empty_tree() {
    let entries = [entry(2, 1, "codex.exe")];
    assert_eq!(
        exe_names_from_entries(&entries, 1).unwrap_err().kind(),
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
    let names = exe_names_from_entries(&entries, 1).unwrap();
    assert!(names.contains("codex.exe"));
    assert!(names.contains("node_repl.exe"));
    assert_eq!(names.len(), 4);
}
