//! One command to vet the tree: formatting, clippy, compilation.
//!
//! Invoked as `cargo lint` (see the alias in `.cargo/config.toml`). By
//! default it first fixes everything that can be fixed mechanically and
//! only then reports what is left.
//!
//! Deliberately dependency-free and shell-free: this runs before every
//! check, so its own build has to be instant, and its behavior must not
//! depend on which shell invoked it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What to do with the clippy lints that remain after the fix pass.
#[derive(PartialEq)]
enum Strictness {
    /// Report the count and carry on. The default while the tree still
    /// carries a backlog: a gate that always fails stops meaning anything.
    Report,
    /// Any remaining lint is a failure. For CI, and for the day the
    /// backlog is gone.
    Deny,
}

struct Options {
    fix: bool,
    package: Option<String>,
    strictness: Strictness,
}

fn main() {
    let opts = match parse_args() {
        Ok(opts) => opts,
        Err(msg) => {
            eprintln!("{msg}\n");
            print_usage();
            std::process::exit(2);
        }
    };

    let root = repo_root();
    let mut failures: Vec<String> = Vec::new();

    let scope: Vec<String> = match &opts.package {
        Some(p) => vec!["-p".into(), p.clone()],
        None => vec!["--workspace".into()],
    };

    // `--all-targets` so tests, benches and examples are held to the same
    // standard as the rest: a warning that only lives in the test target is
    // no better than any other, and without this flag nobody ever sees it.
    let all_targets = ["--all-targets".to_string()];

    // rustfmt has to honor `-p` too. It does not share cargo's notion of
    // scope: `cargo fmt --all` reformats every crate in the workspace even
    // when the rest of the run was restricted to one package, which turns
    // "lint one crate" into a 79-file diff nobody asked for.
    let fmt_scope: Vec<String> = match &opts.package {
        Some(p) => vec!["-p".into(), p.clone()],
        None => vec!["--all".into()],
    };

    if opts.fix {
        // Formatting goes first: `clippy --fix` rewrites the same lines, so
        // in the other order the last step would clobber the first.
        let mut fmt_args = vec!["fmt".to_string()];
        fmt_args.extend(fmt_scope.iter().cloned());
        step(&root, "Formatting", &fmt_args, &mut failures);

        // `--fix` writes straight into the working tree and refuses to do so
        // with uncommitted changes unless told otherwise. Here, editing the
        // working tree is the whole point.
        let mut args = vec!["clippy".to_string()];
        args.extend(scope.iter().cloned());
        args.extend(all_targets.iter().cloned());
        args.extend(
            ["--fix", "--allow-dirty", "--allow-staged"]
                .iter()
                .map(|s| s.to_string()),
        );
        // A failure here does not fail the command: some lints have no
        // machine applicable fix, which is an expected outcome, not an error.
        let _ = run(&root, &args);

        // `clippy --fix` does not format what it rewrote.
        step(&root, "Formatting after autofixes", &fmt_args, &mut failures);
    } else {
        let mut fmt_args = vec!["fmt".to_string()];
        fmt_args.extend(fmt_scope.iter().cloned());
        fmt_args.extend(["--", "--check"].iter().map(|s| s.to_string()));
        step(&root, "Formatting (check only)", &fmt_args, &mut failures);
    }

    // Clippy: capture the output so the remainder can be counted and grouped.
    //
    // Deliberately NOT passing `-- -D warnings`, even in Deny mode. Trailing
    // `--` arguments are part of the build fingerprint of every crate in the
    // graph, not just the one named by `-p`, so `-D warnings` promotes lints
    // in unrelated path dependencies to hard errors and the build dies before
    // it ever reaches the package under test. Denying is our job, not rustc's:
    // capture the lints, keep only the ones that belong to the requested
    // scope, and fail on that count.
    let mut args = vec!["clippy".to_string()];
    args.extend(scope.iter().cloned());
    args.extend(all_targets.iter().cloned());
    args.push("--message-format".into());
    args.push("short".into());

    println!("\n==> Clippy");
    let clippy = capture(&root, &args);

    // Same reason in reverse: clippy lints workspace path dependencies too,
    // so a run scoped to one package still reports everyone else's lints.
    // Attribute each lint to a directory and keep only ours.
    // Relative to the workspace root, because that is how clippy spells the
    // paths it reports.
    let package_dir = opts.package.as_deref().and_then(|pkg| {
        package_dir(&root, pkg).map(|dir| {
            dir.strip_prefix(&root)
                .map(|p| p.to_path_buf())
                .unwrap_or(dir)
        })
    });
    if opts.package.is_some() && package_dir.is_none() {
        println!("    note: could not locate the package directory; counting every lint");
    }
    let warnings = summarize(&clippy.output, package_dir.as_deref());
    let total: usize = warnings.values().sum();

    if total > 0 {
        println!("    lints remaining: {total}");
        // Only the top of the list: a thousand lines in the terminal helps
        // nobody, while the per-category counts show what to attack first.
        for (kind, count) in top(&warnings, 10) {
            println!("    {count:>5}  {kind}");
        }
        println!("    full list: cargo clippy --workspace --all-targets");
    } else {
        println!("    clean");
    }

    // A non-zero exit here means clippy itself failed to run (or the code
    // does not compile), which is a failure regardless of strictness.
    if !clippy.ok {
        failures.push("Clippy".into());
    }
    if opts.strictness == Strictness::Deny && total > 0 {
        failures.push(format!("Clippy ({total} lints in scope)"));
    }

    // Final proof that the tree actually builds, across every target.
    let mut check_args = vec!["check".to_string()];
    check_args.extend(scope.iter().cloned());
    check_args.extend(all_targets.iter().cloned());
    step(&root, "Compilation", &check_args, &mut failures);

    println!();
    if failures.is_empty() {
        if total > 0 {
            println!("Formatting and compilation are fine; clippy lints remaining: {total}.");
            println!("Strict mode: cargo lint -- --deny");
        } else {
            println!("All clean: formatting, clippy, compilation.");
        }
        return;
    }

    println!("Failed:");
    for f in &failures {
        println!("  * {f}");
    }
    if opts.fix {
        println!();
        println!("Autofixes have already been applied to the working tree; the above");
        println!("is what no machine could fix.");
    }
    std::process::exit(1);
}

struct Captured {
    ok: bool,
    output: String,
}

fn repo_root() -> PathBuf {
    // This file lives at <repo>/xtask/src/main.rs, so the root is one level
    // above this package's manifest directory.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask always has a parent directory")
        .to_path_buf()
}

fn cargo() -> String {
    // Under `cargo run`, CARGO points at the very toolchain that launched
    // us; there is no reason to fail when it is absent.
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn run(root: &Path, args: &[String]) -> bool {
    Command::new(cargo())
        .args(args)
        .current_dir(root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn capture(root: &Path, args: &[String]) -> Captured {
    let out = Command::new(cargo())
        .args(args)
        .current_dir(root)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output();

    match out {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stderr).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stdout));
            Captured {
                ok: out.status.success(),
                output: text,
            }
        }
        Err(err) => Captured {
            ok: false,
            output: format!("failed to run cargo: {err}"),
        },
    }
}

fn step<S: AsRef<str>>(root: &Path, title: &str, args: &[S], failures: &mut Vec<String>) {
    println!("\n==> {title}");
    let args: Vec<String> = args.iter().map(|s| s.as_ref().to_string()).collect();
    if !run(root, &args) {
        failures.push(title.to_string());
    }
}

/// Directory of the workspace member named `pkg`, relative to `root`.
///
/// Cargo knows this, but only via `cargo metadata`, whose answer is JSON and
/// would drag a parser into a crate that is deliberately dependency-free.
/// Package names do not reliably match directory names here (`portable-pty`
/// lives in `crates/pty`, `wezterm-term` in `crates/term`), so the manifests
/// are read directly. Depth 3 covers `crates/<name>`, `crates/<a>/<b>` and
/// top-level members like `xtask`.
fn package_dir(root: &Path, pkg: &str) -> Option<PathBuf> {
    fn find(dir: &Path, pkg: &str, depth: usize) -> Option<PathBuf> {
        if depth == 0 {
            return None;
        }
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
            if let Ok(text) = std::fs::read_to_string(&manifest) {
                for line in text.lines() {
                    let line = line.trim();
                    // Only the `[package]` name matters. Dependency entries
                    // are `foo = { ... }`, never `name = "foo"`, so matching
                    // the `name =` key is enough to avoid false positives.
                    if let Some(rest) = line.strip_prefix("name") {
                        let rest = rest.trim_start();
                        if let Some(rest) = rest.strip_prefix('=') {
                            if rest.trim().trim_matches('"') == pkg {
                                return Some(dir.to_path_buf());
                            }
                        }
                    }
                }
            }
        }
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "target" || name.starts_with('.') {
                continue;
            }
            if let Some(found) = find(&path, pkg, depth - 1) {
                return Some(found);
            }
        }
        None
    }

    find(&root.join("crates"), pkg, 3).or_else(|| find(root, pkg, 2))
}

/// Group `--message-format short` output by lint text.
///
/// Lines look like:
/// `crates\foo\src\lib.rs:12:5: warning: the lint text: help: ...`
/// Only the text between `warning: ` and the first `: help:` matters, and
/// anything in backticks is masked out -- otherwise the same lint about two
/// different variables would count as two distinct categories.
fn summarize(output: &str, only_under: Option<&Path>) -> BTreeMap<String, usize> {
    // Clippy reports paths relative to the workspace root, with the platform
    // separator. `only_under` is likewise relative to the root (see the call
    // site), so a plain prefix match is exact -- no substring guessing, which
    // would confuse `crates/window` with `crates/wezterm-gui/src/window`.
    let prefix = only_under.map(|dir| {
        let mut s = dir.to_string_lossy().replace('\\', "/");
        if !s.ends_with('/') {
            s.push('/');
        }
        s
    });

    let mut map = BTreeMap::new();
    for line in output.lines() {
        let Some(pos) = line.find(": warning: ") else {
            continue;
        };
        if let Some(prefix) = &prefix {
            let path = line[..pos].split(':').next().unwrap_or("").replace('\\', "/");
            if !path.starts_with(prefix.as_str()) {
                continue;
            }
        }
        let rest = &line[pos + ": warning: ".len()..];
        // Skip lines like "warning: `foo` (lib) generated N warnings" --
        // that is a tally, not an individual lint.
        if rest.contains("generated ") && rest.contains(" warning") {
            continue;
        }
        let text = match rest.find(": help: ") {
            Some(p) => &rest[..p],
            None => rest,
        };
        *map.entry(mask_identifiers(text)).or_insert(0) += 1;
    }
    map
}

/// Replace everything inside backticks with `X`, so that lints differing
/// only by identifier collapse into one category.
fn mask_identifiers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    for ch in text.chars() {
        if ch == '`' {
            if !inside {
                out.push_str("`X`");
            }
            inside = !inside;
            continue;
        }
        if !inside {
            out.push(ch);
        }
    }
    out
}

fn top(map: &BTreeMap<String, usize>, n: usize) -> Vec<(String, usize)> {
    let mut v: Vec<(String, usize)> = map.iter().map(|(k, c)| (k.clone(), *c)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

fn parse_args() -> Result<Options, String> {
    let mut opts = Options {
        fix: true,
        package: None,
        strictness: Strictness::Report,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // The alias in `.cargo/config.toml` already ends with `--`, but
            // writing `cargo lint -- --deny` is the natural habit, so an
            // extra separator is simply ignored.
            "--" => {}
            "--no-fix" => opts.fix = false,
            "--deny" => opts.strictness = Strictness::Deny,
            "-p" | "--package" => {
                opts.package = Some(
                    args.next()
                        .ok_or_else(|| format!("{arg} requires a package name"))?,
                );
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(opts)
}

fn print_usage() {
    println!(
        "\
cargo lint -- [flags]

  One command: formatting, clippy, compilation. By default it fixes
  everything fixable by machine and prints what remains, by category.

  --no-fix          change nothing, only check. In this mode a formatting
                    mismatch is a failure too.
  --deny            treat any remaining clippy lint as a failure.
  -p, --package N   restrict everything to a single package.
  -h, --help        this help.

  cargo lint-check  same as cargo lint -- --no-fix --deny"
    );
}
