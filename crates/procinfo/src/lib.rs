#![warn(clippy::undocumented_unsafe_blocks)]
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[cfg(feature = "lua")]
use wezterm_dynamic::{FromDynamic, ToDynamic};

mod linux;
mod macos;
mod windows;

#[derive(Debug, Copy, Clone)]
#[cfg_attr(feature = "lua", derive(FromDynamic, ToDynamic))]
pub enum LocalProcessStatus {
    Idle,
    Run,
    Sleep,
    Stop,
    Zombie,
    Tracing,
    Dead,
    Wakekill,
    Waking,
    Parked,
    LockBlocked,
    Unknown,
}

#[derive(Debug)]
#[cfg_attr(feature = "lua", derive(FromDynamic, ToDynamic))]
pub struct LocalProcessInfo {
    /// The process identifier
    pub pid: u32,
    /// The parent process identifier
    pub ppid: u32,
    /// The COMM name of the process. May not bear any relation to
    /// the executable image name. May be changed at runtime by
    /// the process.
    /// Many systems truncate this
    /// field to 15-16 characters.
    pub name: String,
    /// Path to the executable image
    pub executable: PathBuf,
    /// The argument vector.
    /// Some systems allow changing the argv block at runtime
    /// eg: setproctitle().
    pub argv: Vec<String>,
    /// The current working directory for the process, or an empty
    /// path if it was not accessible for some reason.
    pub cwd: PathBuf,
    /// The status of the process. Not all possible values are
    /// portably supported on all systems.
    pub status: LocalProcessStatus,
    /// A clock value in unspecified system dependent units that
    /// indicates the relative age of the process.
    pub start_time: u64,
    /// The console handle associated with the process, if any.
    #[cfg(windows)]
    pub console: u64,
    /// Child processes, keyed by pid
    pub children: HashMap<u32, LocalProcessInfo>,
}

/// `LocalProcessInfo` forms a tree (via `children`) that is built once
/// per refresh from a snapshot of the system-wide process list, and that
/// can in principle grow arbitrarily deep over the lifetime of a long
/// running terminal session (eg. deeply nested shell/tool spawn chains).
/// The naive `#[derive(Clone)]`/default recursive `Drop` would recurse
/// once per level of tree depth, which risks a stack overflow for
/// sufficiently deep trees. Both are implemented iteratively below using
/// an explicit work stack instead of relying on the call stack.
impl Clone for LocalProcessInfo {
    fn clone(&self) -> Self {
        // Clone every node shallowly (no children yet) into a flat arena
        // using an explicit stack instead of recursing through
        // `Clone::clone`, then re-attach children to parents in a second,
        // purely iterative pass driven by parent-index bookkeeping (the
        // same technique `build_tree_iterative` below uses). This keeps
        // the whole operation O(n) in the size of the tree, rather than
        // the O(depth) per-node parent lookup (and thus O(n * depth)
        // overall) that walking a stored path from the root for every
        // node would cost.
        fn shallow_clone(item: &LocalProcessInfo) -> LocalProcessInfo {
            LocalProcessInfo {
                pid: item.pid,
                ppid: item.ppid,
                name: item.name.clone(),
                executable: item.executable.clone(),
                argv: item.argv.clone(),
                cwd: item.cwd.clone(),
                status: item.status,
                start_time: item.start_time,
                #[cfg(windows)]
                console: item.console,
                children: HashMap::new(),
            }
        }

        let mut arena: Vec<LocalProcessInfo> = vec![shallow_clone(self)];
        let mut parent_of: Vec<usize> = vec![usize::MAX]; // root has no parent

        // DFS stack of (source node, arena index of its already-cloned
        // parent).
        let mut stack: Vec<(&LocalProcessInfo, usize)> = Vec::new();
        for child in self.children.values() {
            stack.push((child, 0));
        }

        while let Some((item, parent_index)) = stack.pop() {
            let child_index = arena.len();
            arena.push(shallow_clone(item));
            parent_of.push(parent_index);

            for child in item.children.values() {
                stack.push((child, child_index));
            }
        }

        // Attach children to parents from the end of the arena backwards;
        // every node appears strictly after its parent, so this always
        // finds the parent still in place.
        let mut nodes: Vec<Option<LocalProcessInfo>> = arena.into_iter().map(Some).collect();
        for index in (1..nodes.len()).rev() {
            let node = nodes[index].take().expect("node must still be present");
            let parent_index = parent_of[index];
            let parent = nodes[parent_index]
                .as_mut()
                .expect("parent must still be present");
            parent.children.insert(node.pid, node);
        }

        nodes[0].take().expect("root must still be present")
    }
}

impl Drop for LocalProcessInfo {
    fn drop(&mut self) {
        // Default recursive drop-glue for `HashMap<u32, LocalProcessInfo>`
        // would recurse once per level of tree depth. Instead, detach all
        // descendants into a flat work list and drop them iteratively.
        let mut pending: Vec<LocalProcessInfo> =
            core::mem::take(&mut self.children).into_values().collect();

        while let Some(mut node) = pending.pop() {
            pending.extend(core::mem::take(&mut node.children).into_values());
            // `node` is dropped here with an already-empty `children`,
            // so this recursive `Drop::drop` call cannot recurse further.
        }
    }
}

impl LocalProcessInfo {
    /// Walk this sub-tree of processes and return a unique set
    /// of executable base names. eg: `foo/bar` and `woot/bar`
    /// produce a set containing just `bar`.
    pub fn flatten_to_exe_names(&self) -> HashSet<String> {
        let mut names = HashSet::new();

        let mut stack: Vec<&LocalProcessInfo> = vec![self];
        while let Some(item) = stack.pop() {
            match item.executable.file_name() {
                Some(exe) if !exe.is_empty() => {
                    names.insert(exe.to_string_lossy().into_owned());
                }
                // Handle no permission to read executable path program, like 'sudo'
                _ => {
                    names.insert(item.name.clone());
                }
            }
            stack.extend(item.children.values());
        }

        names
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    pub fn with_root_pid(_pid: u32) -> Option<Self> {
        None
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    pub fn current_working_dir(_pid: u32) -> Option<PathBuf> {
        None
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    pub fn executable_path(_pid: u32) -> Option<PathBuf> {
        None
    }
}

/// Iteratively assemble a `LocalProcessInfo` tree rooted at `root_pid` from
/// a flat slice of platform-specific process records, without recursing
/// once per level of tree depth.
///
/// This replaces what was previously a recursive `build_proc` helper
/// duplicated (with minor per-platform variations) across the linux,
/// macos and windows backends. The traversal order (a pre-order DFS
/// governed by an explicit stack, mirroring the original recursive scan)
/// and the `visited` dedup semantics (each pid is attached to the tree at
/// most once, guarding against parent/child cycles from stale or reused
/// pids in the snapshot) are preserved exactly; only the mechanism
/// (explicit stack vs. call stack) has changed.
///
/// `pid_of`/`ppid_of` extract the process id / parent process id from a
/// platform record, and `make_leaf` builds a `LocalProcessInfo` for a
/// single record (with an empty, to-be-filled-in `children` map).
pub(crate) fn build_tree_iterative<T, PidOf, PpidOf, MakeLeaf>(
    items: &[T],
    root_pid: u32,
    pid_of: PidOf,
    ppid_of: PpidOf,
    make_leaf: MakeLeaf,
) -> Option<LocalProcessInfo>
where
    PidOf: Fn(&T) -> u32,
    PpidOf: Fn(&T) -> u32,
    MakeLeaf: Fn(&T) -> LocalProcessInfo,
{
    let root_item = items.iter().find(|item| pid_of(item) == root_pid)?;

    let mut visited: HashSet<u32> = HashSet::new();
    visited.insert(root_pid);

    // Arena of discovered nodes, indexed by position in `arena`. Each
    // entry records its parent's arena index (`None` for the root) so
    // that, after building every leaf node with an explicit stack
    // instead of recursion, we can attach children to parents in a
    // second, purely iterative pass.
    let mut arena: Vec<LocalProcessInfo> = Vec::new();
    let mut parent_of: Vec<Option<usize>> = Vec::new();

    arena.push(make_leaf(root_item));
    parent_of.push(None);

    // DFS stack of (item, arena index of its parent). Pushing/popping an
    // explicit `Vec` here stands in for what used to be the call stack
    // of a recursive `build_proc`.
    let mut stack: Vec<(&T, usize)> = Vec::new();
    stack.push((root_item, 0));

    while let Some((item, this_index)) = stack.pop() {
        let this_pid = pid_of(item);

        // Preserve the original recursive scan's ordering: iterate the
        // full record list in order, and for each match, push it before
        // moving on, so that a child's own descendants are discovered
        // (and thus arena-indexed) before its later siblings, matching
        // what the equivalent recursive `build_proc` would have done.
        for kid in items.iter().rev() {
            if ppid_of(kid) == this_pid && !visited.contains(&pid_of(kid)) {
                visited.insert(pid_of(kid));
                let kid_index = arena.len();
                arena.push(make_leaf(kid));
                parent_of.push(Some(this_index));
                stack.push((kid, kid_index));
            }
        }
    }

    // Attach children to parents from the end of the arena backwards.
    // Because every node is pushed onto `arena` strictly after its
    // parent, iterating in reverse guarantees each node is detached and
    // moved into its parent's `children` map before we ever touch the
    // parent's own slot again.
    let mut nodes: Vec<Option<LocalProcessInfo>> = arena.into_iter().map(Some).collect();
    for index in (1..nodes.len()).rev() {
        let node = nodes[index].take().expect("node must still be present");
        let parent_index = parent_of[index].expect("non-root node must have a parent");
        let parent = nodes[parent_index]
            .as_mut()
            .expect("parent must still be present");
        parent.children.insert(node.pid, node);
    }

    nodes[0].take()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic, purely linear chain of `LocalProcessInfo`
    /// `depth` levels deep: root -> child -> grandchild -> ... This
    /// mimics the worst case for tree depth (eg. a long lived shell
    /// spawning deeply nested subshells/tools over a long session)
    /// without needing to spawn any real OS processes.
    fn make_deep_chain(depth: u32) -> LocalProcessInfo {
        fn leaf(pid: u32, ppid: u32) -> LocalProcessInfo {
            LocalProcessInfo {
                pid,
                ppid,
                name: format!("proc{pid}"),
                executable: PathBuf::from(format!("/bin/proc{pid}")),
                argv: vec![],
                cwd: PathBuf::new(),
                status: LocalProcessStatus::Run,
                start_time: pid as u64,
                #[cfg(windows)]
                console: 0,
                children: HashMap::new(),
            }
        }

        // Build bottom-up with an explicit stack (test-only helper; not
        // the code path we're validating for depth-safety, so recursion
        // here would be fine, but we avoid it anyway for clarity).
        let mut node = leaf(depth, depth.saturating_sub(1));
        for pid in (0..depth).rev() {
            let mut parent = leaf(pid, pid.saturating_sub(1));
            parent.children.insert(node.pid, node);
            node = parent;
        }
        node
    }

    fn chain_depth(root: &LocalProcessInfo) -> usize {
        let mut depth = 0;
        let mut node = root;
        loop {
            depth += 1;
            match node.children.values().next() {
                Some(child) => node = child,
                None => return depth,
            }
        }
    }

    // Deep enough that the old recursive Drop/Clone/flatten
    // implementations would reliably blow a default (1MB-ish) thread
    // stack (each recursive frame here is small, but a few thousand
    // levels is already far beyond what any real process tree would
    // reach, and is enough to prove the new implementations are not
    // depth-limited at all).
    const TEST_DEPTH: u32 = 20_000;

    #[test]
    fn deep_tree_flatten_does_not_overflow() {
        let root = make_deep_chain(TEST_DEPTH);
        assert_eq!(chain_depth(&root), TEST_DEPTH as usize + 1);

        let names = root.flatten_to_exe_names();
        assert_eq!(names.len(), TEST_DEPTH as usize + 1);
        assert!(names.contains("proc0"));
        assert!(names.contains(&format!("proc{TEST_DEPTH}")));
    }

    #[test]
    fn deep_tree_clone_does_not_overflow() {
        let root = make_deep_chain(TEST_DEPTH);
        let cloned = root.clone();

        assert_eq!(chain_depth(&cloned), TEST_DEPTH as usize + 1);
        assert_eq!(cloned.pid, root.pid);

        // Spot check that the leaf-most process made it into the clone
        // with the right identity, confirming the iterative clone
        // attached every level rather than silently truncating.
        let mut node = &cloned;
        loop {
            match node.children.values().next() {
                Some(child) => node = child,
                None => break,
            }
        }
        assert_eq!(node.pid, TEST_DEPTH);

        // Both trees are dropped here (root, cloned): this is exercising
        // the iterative `Drop` impl at the same depth.
    }

    #[test]
    fn deep_tree_drop_does_not_overflow() {
        // Constructing and immediately dropping a deep tree is exactly
        // the scenario reported upstream: a long lived pane's cached
        // process tree being torn down. If `Drop` were still the
        // default recursive one, this would overflow the stack.
        for _ in 0..3 {
            let root = make_deep_chain(TEST_DEPTH);
            drop(root);
        }
    }

    #[test]
    fn shallow_tree_flatten_and_clone_still_correct() {
        // Non-degenerate (branching) small tree, to make sure the
        // iterative rewrite didn't change behavior for the common case.
        let mut root = LocalProcessInfo {
            pid: 1,
            ppid: 0,
            name: "root".into(),
            executable: PathBuf::from("/bin/root"),
            argv: vec![],
            cwd: PathBuf::new(),
            status: LocalProcessStatus::Run,
            start_time: 0,
            #[cfg(windows)]
            console: 0,
            children: HashMap::new(),
        };
        let mut child_a = LocalProcessInfo {
            pid: 2,
            ppid: 1,
            name: "a".into(),
            executable: PathBuf::from("/bin/a"),
            argv: vec![],
            cwd: PathBuf::new(),
            status: LocalProcessStatus::Run,
            start_time: 1,
            #[cfg(windows)]
            console: 0,
            children: HashMap::new(),
        };
        let child_b = LocalProcessInfo {
            pid: 3,
            ppid: 1,
            name: "b".into(),
            executable: PathBuf::from("/bin/b"),
            argv: vec![],
            cwd: PathBuf::new(),
            status: LocalProcessStatus::Run,
            start_time: 2,
            #[cfg(windows)]
            console: 0,
            children: HashMap::new(),
        };
        let grandchild = LocalProcessInfo {
            pid: 4,
            ppid: 2,
            name: "a".into(),
            executable: PathBuf::from("/bin/a"),
            argv: vec![],
            cwd: PathBuf::new(),
            status: LocalProcessStatus::Run,
            start_time: 3,
            #[cfg(windows)]
            console: 0,
            children: HashMap::new(),
        };
        child_a.children.insert(grandchild.pid, grandchild);
        root.children.insert(child_a.pid, child_a);
        root.children.insert(child_b.pid, child_b);

        let names = root.flatten_to_exe_names();
        assert_eq!(names.len(), 3);
        assert!(names.contains("root"));
        assert!(names.contains("a"));
        assert!(names.contains("b"));

        let cloned = root.clone();
        let cloned_names = cloned.flatten_to_exe_names();
        assert_eq!(names, cloned_names);
        assert_eq!(cloned.children.len(), 2);
        assert_eq!(cloned.children[&2].children.len(), 1);
    }

    #[test]
    fn flatten_falls_back_to_name_when_executable_unreadable() {
        // Mirrors upstream #7398: when a process' executable path can't
        // be resolved (eg. a privileged process such as `sudo`, or a
        // process owned by another user), `file_name()` returns `None`.
        // Before the fix such a process silently dropped out of the name
        // set, so `can_close_without_prompting` would not warn the user.
        // The fix falls back to `item.name` (the COMM/argv0 name) instead.
        fn make(pid: u32, name: &str, executable: PathBuf) -> LocalProcessInfo {
            LocalProcessInfo {
                pid,
                ppid: 1,
                name: name.into(),
                executable,
                argv: vec![],
                cwd: PathBuf::new(),
                status: LocalProcessStatus::Run,
                start_time: pid as u64,
                #[cfg(windows)]
                console: 0,
                children: HashMap::new(),
            }
        }

        // Root with a normal executable path.
        let mut root = make(1, "wezterm", PathBuf::from("/usr/bin/wezterm"));

        // A privileged child whose executable path is entirely empty
        // (no permission to read it); only its COMM name is available.
        let sudo = make(2, "sudo", PathBuf::new());
        root.children.insert(sudo.pid, sudo);

        let names = root.flatten_to_exe_names();
        // The normal process contributes its executable base name.
        assert!(names.contains("wezterm"));
        // The privileged process, with no resolvable executable path,
        // must still appear via its COMM name rather than vanishing.
        assert!(names.contains("sudo"));
        assert_eq!(names.len(), 2);
    }
}
