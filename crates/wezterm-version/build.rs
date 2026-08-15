use std::path::{Path, PathBuf};

/// Walks up from `start` looking for a `.git` entry (either a directory, for
/// a normal checkout, or a file, for a worktree/submodule that points
/// elsewhere via a `gitdir:` line) without depending on a git library.
/// Returns the path to that `.git` entry, if found.
fn find_dot_git(start: &Path) -> Option<PathBuf> {
    let mut dir = start.canonicalize().ok()?;
    loop {
        let candidate = dir.join(".git");
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Short commit hash, captured independently of `ci_tag` below: `git
    // describe` only appends a `-g<hash>` suffix when HEAD is *past* the
    // nearest tag, so a checkout sitting exactly on a release tag would
    // otherwise report a version string with no commit hash in it at all.
    // Callers that want the hash unconditionally (e.g. the Ctrl+I version
    // overlay) use this instead of parsing it back out of `ci_tag`.
    let commit_hash = find_dot_git(Path::new("."))
        .and_then(|_| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short=8", "HEAD"])
                .output()
                .ok()
        })
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ONLYTERM_CI_COMMIT_HASH={}", commit_hash);

    // Ordinal commit number (total commits reachable from HEAD) -- a
    // stable, always-increasing counter that's easier to eyeball/compare
    // than a hash, for the same "which build am I looking at" purpose.
    let commit_count = find_dot_git(Path::new("."))
        .and_then(|_| {
            std::process::Command::new("git")
                .args(["rev-list", "--count", "HEAD"])
                .output()
                .ok()
        })
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ONLYTERM_CI_COMMIT_COUNT={}", commit_count);

    // Wall-clock time this build.rs last actually ran, i.e. the time of
    // the build that produced the current binary -- not the current
    // build invocation's time, since cargo skips rerunning this script
    // (and its cached rustc-env output stays as-is) when nothing tracked
    // by the `cargo:rerun-if-changed` lines above has changed.
    let build_time = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    println!("cargo:rustc-env=ONLYTERM_CI_BUILD_TIME={}", build_time);

    // Tell cargo to re-run this script whenever HEAD moves. This must happen
    // REGARDLESS of which branch below produces the version string: the commit
    // hash and count above are captured on every path, so tying the rerun
    // trigger to only one of them freezes them on the others.
    //
    // Paths are emitted verbatim, NOT canonicalized. On Windows,
    // `canonicalize()` returns an extended-length path (`\\?\D:\...`), which
    // cargo does not match against the plain path it tracks -- so the
    // dependency registered but never fired, and this script stopped running
    // altogether. Measured: a release binary built from a checkout 55 commits
    // past the last time this script ran still reported the older commit,
    // hash and build date, because cargo reused its cached output.
    //
    // Three entries are needed to cover how git stores a branch tip:
    // `HEAD` itself (changes on checkout, and holds the sha when detached),
    // the loose ref file it points at (changes on commit), and `packed-refs`
    // (where `git gc` moves refs, leaving no loose file to watch).
    if let Some(dot_git) = find_dot_git(Path::new(".")) {
        if dot_git.is_dir() {
            let head_path = dot_git.join("HEAD");
            println!("cargo:rerun-if-changed={}", head_path.display());
            println!(
                "cargo:rerun-if-changed={}",
                dot_git.join("packed-refs").display()
            );
            if let Ok(contents) = std::fs::read_to_string(&head_path) {
                if let Some(refname) = contents.trim().strip_prefix("ref: ") {
                    println!("cargo:rerun-if-changed={}", dot_git.join(refname).display());
                }
            }
        }
    }

    // If a file named `.tag` is present, we'll take its contents for the
    // version number that we report in `--version`.
    let mut ci_tag = String::new();
    if let Ok(tag) = std::fs::read("../.tag") {
        if let Ok(s) = String::from_utf8(tag) {
            ci_tag = s.trim().to_string();
            println!("cargo:rerun-if-changed=../.tag");
        }
    } else {
        // Otherwise we'll derive it from the git information.
        //
        // We used to use `git2::Repository::discover` just to locate the
        // `.git` directory so that we could set up a `cargo:rerun-if-changed`
        // on the resolved HEAD ref file (so that rebuilds are triggered by
        // switching branches/commits). That's the only thing git2 was used
        // for here -- the actual version string below has always come from
        // shelling out to the `git` binary via `std::process::Command`, not
        // from git2/libgit2. Replaced with a plain filesystem walk so this
        // build script doesn't need to link libgit2 at all.
        if find_dot_git(Path::new(".")).is_some() {
            // Prefer a human-meaningful version derived from the nearest
            // reachable `v*` tag (e.g. `v0.0.2-alpha`, or
            // `v0.0.2-alpha-3-gabc1234` for commits past the tag) so that
            // `wezterm -h` reflects the project's actual release number
            // instead of only a commit date/hash. Falls back to the
            // date-hash form below if no tag is reachable at all (e.g. a
            // shallow clone with tags not fetched).
            let describe = std::process::Command::new("git")
                .args(["describe", "--tags", "--always", "--dirty=-dirty"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                .filter(|s| !s.is_empty());

            ci_tag = match describe {
                Some(tag) => tag,
                None => std::process::Command::new("git")
                    .args([
                        "-c",
                        "core.abbrev=8",
                        "show",
                        "-s",
                        "--format=%cd-%h",
                        "--date=format:%Y%m%d-%H%M%S",
                    ])
                    .output()
                    .ok()
                    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                    .unwrap_or_default(),
            };
        }
    }

    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=ONLYTERM_TARGET_TRIPLE={}", target);
    println!("cargo:rustc-env=ONLYTERM_CI_TAG={}", ci_tag);
}
