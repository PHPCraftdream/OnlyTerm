use std::io::Write;
use std::path::{Path, PathBuf};

/// Extension, without the dot, that the installer registers with Explorer
/// so that opening a startup layout (a `--start-conf` file) launches it.
pub const RUN_FILE_EXTENSION: &str = "onlyterm-run";

/// Name of the folder, beside the config file, that holds the run files.
pub const RUN_DIR_NAME: &str = ".onlyterm-run";

/// The run-files folder for a config file: a sibling of it.
pub fn run_dir_beside(config_file: &Path) -> PathBuf {
    config_file.with_file_name(RUN_DIR_NAME)
}

/// Whether `path` names a run file, judged by its extension alone.
pub fn is_run_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(RUN_FILE_EXTENSION))
}

/// The folder-settings run file for `dir`, inside `run_dir`.
///
/// The name is a readable form of the path plus a hash of its
/// case-folded form, so `D:\a-b` and `D:\a\b` never share a file.
pub fn run_file_for_dir(run_dir: &Path, dir: &Path) -> PathBuf {
    let text = dir.to_string_lossy();
    let trimmed = text.trim_end_matches(['\\', '/']);
    let key = trimmed.replace('/', "\\").to_lowercase();

    let readable: String = trimmed
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .take(80)
        .collect();

    run_dir.join(format!(
        "{}-{:08x}.{}",
        readable,
        fnv1a_32(key.as_bytes()),
        RUN_FILE_EXTENSION
    ))
}

fn fnv1a_32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |hash, &b| {
        (hash ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// Placeholder in the template that becomes the folder the file is made for.
pub const LAUNCH_DIR_VAR: &str = "${LAUNCH_DIR}";

/// The template folder-settings files are made from, inside the run folder.
/// Not a `.onlyterm-run` file, so opening it does not launch anything.
pub const TEMPLATE_FILE_NAME: &str = "template.ktav";

/// Built-in contents of the template: one tab in the launch folder, every
/// other layout setting present as a commented-out example.
pub fn default_template() -> String {
    include_str!("run_files_template.ktav").to_string()
}

/// Fills in the template for `dir`. A path tail after the variable
/// (`${LAUNCH_DIR}\..\lib`) is resolved into a plain path: `..` goes up
/// (never above the root), `.` is dropped. The tail runs over segments that
/// start with `\` or `/` and end at the next separator or line end.
pub fn render_template(template: &str, dir: &Path) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find(LAUNCH_DIR_VAR) {
        out.push_str(&rest[..at]);
        rest = &rest[at + LAUNCH_DIR_VAR.len()..];

        let mut path = dir.to_path_buf();
        while rest.starts_with(['\\', '/']) {
            let segment_len = rest[1..]
                .find(['\\', '/', '\r', '\n'])
                .unwrap_or(rest.len() - 1);
            let segment = &rest[1..1 + segment_len];
            match segment {
                "" | "." => {}
                ".." => {
                    path.pop();
                }
                _ => path.push(segment),
            }
            rest = &rest[1 + segment_len..];
        }
        out.push_str(&path.display().to_string());
    }
    out.push_str(rest);
    out
}

/// Writes `contents` to `path` unless the file already exists.
fn create_if_missing(path: &Path, contents: impl FnOnce() -> String) -> anyhow::Result<()> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => file
            .write_all(contents().as_bytes())
            .map_err(|err| anyhow::anyhow!("writing {}: {}", path.display(), err)),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(anyhow::anyhow!("creating {}: {}", path.display(), err)),
    }
}

/// Creates the run folder and its template, each only if missing; returns
/// the template's path.
pub fn ensure_template(run_dir: &Path) -> anyhow::Result<PathBuf> {
    crate::create_user_owned_dirs(run_dir)?;
    let path = run_dir.join(TEMPLATE_FILE_NAME);
    create_if_missing(&path, default_template)?;
    Ok(path)
}

/// Creates the folder-settings run file for `dir` from the template unless
/// it already exists; returns its path.
pub fn ensure_run_file_for_dir(run_dir: &Path, dir: &Path) -> anyhow::Result<PathBuf> {
    let template_path = ensure_template(run_dir)?;
    let template = std::fs::read_to_string(&template_path)
        .map_err(|err| anyhow::anyhow!("reading {}: {}", template_path.display(), err))?;
    let path = run_file_for_dir(run_dir, dir);
    create_if_missing(&path, || render_template(&template, dir))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_file_name_is_readable_and_hashed() {
        let path = run_file_for_dir(Path::new("R"), Path::new(r"D:\dev\rust\wezterm"));
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("D--dev-rust-wezterm-"), "{}", name);
        assert!(is_run_file(&path));
        assert_eq!(path.parent().unwrap(), Path::new("R"));
    }

    #[test]
    fn folder_file_ignores_case_trailing_separator_and_slashes() {
        let a = run_file_for_dir(Path::new("R"), Path::new(r"D:\Dev\X\"));
        let b = run_file_for_dir(Path::new("R"), Path::new("d:/dev/x"));
        // "-<8 hex>.onlyterm-run": the hash part only.
        let suffix = |p: &Path| {
            let name = p.file_name().unwrap().to_str().unwrap().to_owned();
            name[name.len() - 22..].to_owned()
        };
        assert_eq!(suffix(&a), suffix(&b));
    }

    #[test]
    fn folder_files_differ_when_only_separators_differ() {
        let a = run_file_for_dir(Path::new("R"), Path::new(r"D:\a-b"));
        let b = run_file_for_dir(Path::new("R"), Path::new(r"D:\a\b"));
        assert_ne!(a, b);
    }

    fn temp_base(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "onlyterm-run-files-{}-{}",
            name,
            std::process::id()
        ))
    }

    #[test]
    fn default_template_makes_a_layout_in_that_folder() {
        let base = temp_base("default");
        let dir = base.join("My Project");
        let run_dir = base.join(RUN_DIR_NAME);
        let path = ensure_run_file_for_dir(&run_dir, &dir).unwrap();

        assert!(run_dir.join(TEMPLATE_FILE_NAME).is_file());
        let layout = crate::StartupLayout::load(&path).unwrap();
        assert_eq!(layout.tabs.len(), 1);
        assert_eq!(layout.tabs[0].root_dir.as_deref(), Some(dir.as_path()));

        std::fs::write(&path, "tabs: [\n    {\n        title: kept\n    }\n]\n").unwrap();
        ensure_run_file_for_dir(&run_dir, &dir).unwrap();
        let kept = crate::StartupLayout::load(&path).unwrap();
        assert_eq!(kept.tabs[0].title.as_deref(), Some("kept"));

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn edited_template_is_used_and_kept() {
        let base = temp_base("edited");
        let dir = base.join("proj");
        let run_dir = base.join(RUN_DIR_NAME);
        std::fs::create_dir_all(&run_dir).unwrap();
        let template = "tabs: [\n    {\n        title: ${LAUNCH_DIR}\n        root_dir: ${LAUNCH_DIR}\n    }\n]\n";
        std::fs::write(run_dir.join(TEMPLATE_FILE_NAME), template).unwrap();

        let path = ensure_run_file_for_dir(&run_dir, &dir).unwrap();
        let layout = crate::StartupLayout::load(&path).unwrap();
        let shown = dir.display().to_string();
        assert_eq!(layout.tabs[0].title.as_deref(), Some(shown.as_str()));
        assert_eq!(layout.tabs[0].root_dir.as_deref(), Some(dir.as_path()));
        assert_eq!(
            std::fs::read_to_string(run_dir.join(TEMPLATE_FILE_NAME)).unwrap(),
            template
        );

        std::fs::remove_dir_all(&base).ok();
    }

    /// The template with every commented-out setting turned on: drops the
    /// `## ` of lines that are settings or block lines, not prose.
    fn uncomment_examples(template: &str) -> String {
        template
            .lines()
            .map(|line| {
                let indent = &line[..line.len() - line.trim_start().len()];
                let Some(rest) = line.trim_start().strip_prefix("## ") else {
                    return line.to_string();
                };
                let key_len = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                let is_setting = key_len > 0 && rest[key_len..].starts_with(": ");
                let is_block = rest.starts_with("    ") || matches!(rest, "{" | "}" | "[" | "]");
                if is_setting || is_block {
                    format!("{}{}", indent, rest)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn default_template_names_every_layout_and_tab_setting() {
        use onlyterm_dynamic::{ToDynamic, Value};
        let template = default_template();
        let tab = crate::StartupTab::default().to_dynamic();
        let layout = crate::StartupLayout::default().to_dynamic();
        for value in [&layout, &tab].iter() {
            let Value::Object(obj) = value else {
                panic!("expected an object, got {:?}", value);
            };
            for key in obj.keys() {
                let Value::String(key) = key else {
                    panic!("expected a string key, got {:?}", key);
                };
                assert!(
                    template.contains(&format!("{}: ", key)),
                    "template lacks `{}`",
                    key
                );
            }
        }
    }

    #[test]
    fn every_example_in_the_default_template_is_valid() {
        let dir = Path::new(r"D:\dev\proj");
        let text = uncomment_examples(&render_template(&default_template(), dir));
        let base = temp_base("examples");
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("all.onlyterm-run");
        std::fs::write(&path, &text).unwrap();
        let layout = crate::StartupLayout::load(&path);
        std::fs::remove_dir_all(&base).ok();
        let layout = layout.unwrap_or_else(|err| panic!("{:#}\n{}", err, text));

        assert_eq!(layout.root_dir.as_deref(), Some(dir));
        assert_eq!(
            layout.vars.get("GIT_PAGER").map(String::as_str),
            Some("cat")
        );
        assert_eq!(layout.commands, vec!["echo Welcome!".to_string()]);
        assert_eq!(layout.shell, Some(crate::Shell::Powershell));
        assert_eq!(layout.admin, Some(false));
        assert_eq!(layout.tabs.len(), 2);

        let first = &layout.tabs[0];
        assert_eq!(first.title.as_deref(), Some("main"));
        assert_eq!(first.vars.get("RUST_LOG").map(String::as_str), Some("info"));
        assert_eq!(first.commands, vec!["git status".to_string()]);
        let options = layout.tab_options(first);
        assert_eq!(options.shell, Some(crate::Shell::Cmd));
        assert_eq!(options.priority, crate::ProcessPriority::BelowNormal);

        assert_eq!(layout.tabs[1].title.as_deref(), Some("parent"));
        assert_eq!(
            layout.tabs[1].root_dir.as_deref(),
            Some(Path::new(r"D:\dev"))
        );
    }

    #[test]
    fn render_replaces_every_launch_dir() {
        let out = render_template("a ${LAUNCH_DIR} b ${LAUNCH_DIR}", Path::new(r"C:\x"));
        assert_eq!(out, r"a C:\x b C:\x");
    }

    #[test]
    fn render_resolves_dot_dot_after_the_variable() {
        let dir = Path::new(r"D:\dev\proj");
        let render = |t: &str| render_template(t, dir);
        assert_eq!(render(r"root_dir: ${LAUNCH_DIR}\.."), r"root_dir: D:\dev");
        assert_eq!(render(r"${LAUNCH_DIR}\..\other\.\src"), r"D:\dev\other\src");
        assert_eq!(render("${LAUNCH_DIR}/../other"), r"D:\dev\other");
        assert_eq!(render(r"${LAUNCH_DIR}\..\..\..\.."), r"D:\");
        assert_eq!(render("a: ${LAUNCH_DIR}\\..\nb: x"), "a: D:\\dev\nb: x");
        assert_eq!(render("${LAUNCH_DIR}\\..\r\n"), "D:\\dev\r\n");
        assert_eq!(
            render("title: ${LAUNCH_DIR} - main"),
            r"title: D:\dev\proj - main"
        );
    }

    #[test]
    fn run_dir_is_a_sibling_of_the_config_file() {
        let dir = run_dir_beside(Path::new("C:/Users/me/.onlyterm.ktav"));
        assert_eq!(dir, Path::new("C:/Users/me/.onlyterm-run"));
    }

    #[test]
    fn run_dir_of_a_bare_file_name_stays_relative() {
        assert_eq!(
            run_dir_beside(Path::new("onlyterm.ktav")),
            Path::new(RUN_DIR_NAME)
        );
    }

    #[test]
    fn run_files_are_recognised_by_extension_only() {
        assert!(is_run_file(Path::new("crush.onlyterm-run")));
        assert!(is_run_file(Path::new("D:/x/CRUSH.ONLYTERM-RUN")));
        assert!(!is_run_file(Path::new("crush.ktav")));
        assert!(!is_run_file(Path::new("onlyterm-run")));
        assert!(!is_run_file(Path::new("crush.onlyterm-run.bak")));
    }
}
