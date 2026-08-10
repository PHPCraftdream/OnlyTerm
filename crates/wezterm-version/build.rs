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

    // If a file named `.tag` is present, we'll take its contents for the
    // version number that we report in wezterm -h.
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
        if let Some(dot_git) = find_dot_git(Path::new(".")) {
            // Resolve HEAD (possibly a symlink-like "ref: refs/heads/foo"
            // pointer file) to the actual ref file that changes on checkout,
            // so that cargo knows to rerun this build script when the
            // current branch/commit changes.
            if dot_git.is_dir() {
                let head_path = dot_git.join("HEAD");
                if let Ok(contents) = std::fs::read_to_string(&head_path) {
                    let contents = contents.trim();
                    if let Some(refname) = contents.strip_prefix("ref: ") {
                        let path = dot_git.join(refname);
                        if path.exists() {
                            if let Ok(canon) = path.canonicalize() {
                                println!("cargo:rerun-if-changed={}", canon.display());
                            }
                        }
                    } else if head_path.exists() {
                        // Detached HEAD: HEAD itself contains the commit sha.
                        if let Ok(canon) = head_path.canonicalize() {
                            println!("cargo:rerun-if-changed={}", canon.display());
                        }
                    }
                }
            }

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
