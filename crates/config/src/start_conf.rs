use crate::keyassignment::ProcessPriority;
use crate::ktav_value::ktav_value_to_dynamic;
use crate::shell::Shell;
use ktav::value::Value as KtavValue;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use wezterm_dynamic::{FromDynamic, FromDynamicOptions, ToDynamic, UnknownFieldAction};

/// A single tab entry inside a `--start-conf` startup layout document; see
/// `StartupLayout` for the shape of the containing document.
#[derive(Debug, Clone, Default, FromDynamic, ToDynamic)]
pub struct StartupTab {
    /// Sets the tab's title immediately after it is spawned (the same
    /// effect as `wezterm cli set-tab-title`, just applied at startup).
    /// The user is still free to rename the tab afterwards.
    pub title: Option<String>,

    /// The working directory this tab's shell starts in, overriding
    /// `StartupLayout::root_dir` (which in turn overrides the usual
    /// `default_cwd` config/domain default). A relative path is resolved
    /// against the directory containing *this layout file*, not the
    /// process's own launch directory -- see `StartupLayout::load`.
    pub root_dir: Option<PathBuf>,

    /// Extra environment variables for just this tab, applied on top of
    /// `StartupLayout::vars`: a key present in both wins with this tab's
    /// value.
    #[dynamic(default)]
    pub vars: HashMap<String, String>,

    /// Shell command lines "typed" into the tab right after its shell
    /// starts, run after `StartupLayout::commands`, in the order given.
    ///
    /// These are typed straight in, without waiting for a prompt, so a
    /// command that starts an interactive or full-screen program swallows
    /// everything listed after it -- put such a command last. `git branch`
    /// is the usual surprise here: past a screenful of output git pipes
    /// through its pager, and the pager eats the following commands as its
    /// own keystrokes. Setting `GIT_PAGER: cat` in `vars` avoids that.
    #[dynamic(default)]
    pub commands: Vec<String>,

    /// Which shell to launch, overriding `StartupLayout::shell`. When
    /// neither is set the usual `default_prog` applies.
    pub shell: Option<Shell>,

    /// Process priority for this tab's shell, overriding
    /// `StartupLayout::priority`.
    pub priority: Option<ProcessPriority>,

    /// Whether to launch this tab elevated, overriding
    /// `StartupLayout::admin`. Each elevated tab raises its own UAC prompt,
    /// which the user has to accept before the next tab is spawned.
    pub admin: Option<bool>,
}

/// The document shape parsed from a `--start-conf <file>.ktav` file: a set
/// of tabs to open at startup, each with its own title, working directory,
/// extra environment variables and pre-typed shell commands, plus
/// layout-wide `root_dir`/`vars`/`commands` that every tab inherits.
///
/// This file is parsed standalone (see `StartupLayout::load`), through the
/// same ktav pipeline the main config uses but *not* merged with or
/// influenced by the user's regular `.onlyterm.ktav`.
#[derive(Debug, Clone, Default, FromDynamic, ToDynamic)]
pub struct StartupLayout {
    /// The working directory tabs start in when they don't set their own
    /// `root_dir`. Falls back to the usual `default_cwd` config/domain
    /// default when neither this nor a tab's own `root_dir` is set. A
    /// relative path is resolved against the directory containing *this
    /// layout file* (see `StartupLayout::load`), so a layout file checked
    /// into a project stays portable regardless of where `onlyterm` is
    /// launched from.
    pub root_dir: Option<PathBuf>,

    /// Environment variables applied to every tab, before that tab's own
    /// `vars` (which may override individual keys).
    #[dynamic(default)]
    pub vars: HashMap<String, String>,

    /// Shell command lines run in every tab, before that tab's own
    /// `commands`. See `StartupTab::commands` for the caveat about
    /// interactive commands swallowing the ones after them.
    #[dynamic(default)]
    pub commands: Vec<String>,

    /// Which shell tabs launch when they don't name their own. Unset means
    /// the usual `default_prog`.
    pub shell: Option<Shell>,

    /// Process priority for tabs that don't set their own.
    pub priority: Option<ProcessPriority>,

    /// Whether tabs launch elevated when they don't say otherwise. Note that
    /// each elevated tab raises its own UAC prompt.
    pub admin: Option<bool>,

    /// The tabs to open, in order. Must be non-empty.
    #[dynamic(default)]
    pub tabs: Vec<StartupTab>,
}

/// How a single tab resolves after `StartupTab`'s settings are laid over the
/// layout-wide ones -- the same "tab wins" rule `root_dir` and `vars` follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedTabOptions {
    pub shell: Option<Shell>,
    pub priority: ProcessPriority,
    pub admin: bool,
}

impl StartupLayout {
    /// Resolves the launch options for `tab` against this layout's defaults.
    pub fn tab_options(&self, tab: &StartupTab) -> ResolvedTabOptions {
        ResolvedTabOptions {
            shell: tab.shell.or(self.shell),
            priority: tab
                .priority
                .or(self.priority)
                .unwrap_or(ProcessPriority::Normal),
            admin: tab.admin.or(self.admin).unwrap_or(false),
        }
    }

    /// Loads and parses a `--start-conf` file. Deliberately standalone
    /// from `Config::load_with_overrides` (`crates/config/src/config_impl.rs`)
    /// -- same ktav-to-`wezterm_dynamic`-to-struct pipeline, but this
    /// document has its own, much smaller shape and isn't a config
    /// override source.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| anyhow::anyhow!("Error opening {}: {}", path.display(), err))?;
        // Skip a potential BOM, same as the main config loader does.
        let text = text.trim_start_matches('\u{FEFF}');

        let parsed = ktav::parse(text)
            .map_err(|err| anyhow::anyhow!("Error parsing {}: {}", path.display(), err))?;

        if !matches!(parsed, KtavValue::Object(_)) {
            anyhow::bail!(
                "Error parsing {}: a --start-conf file must be a top-level ktav \
                 object (`key: value` pairs, e.g. `tabs: [ {{ title: ... }} ]`)",
                path.display()
            );
        }

        let dyn_value = ktav_value_to_dynamic(&parsed);
        let mut layout: StartupLayout = FromDynamic::from_dynamic(
            &dyn_value,
            FromDynamicOptions {
                unknown_fields: UnknownFieldAction::Deny,
                deprecated_fields: UnknownFieldAction::Warn,
            },
        )
        .map_err(|err| {
            anyhow::anyhow!(
                "Error converting {} to a startup layout: {err}",
                path.display()
            )
        })?;

        if layout.tabs.is_empty() {
            anyhow::bail!(
                "{}: no tabs defined (the `tabs` array is empty or missing) -- \
                 nothing to spawn",
                path.display()
            );
        }

        // Resolve any relative `root_dir` against the directory containing
        // *this* layout file (not the process's launch directory), so a
        // layout file checked into a project keeps working no matter where
        // `onlyterm --start-conf` is invoked from. `dunce::canonicalize`
        // (rather than `Path::canonicalize`) matters here on Windows:
        // `std::fs::canonicalize` returns the `\\?\`-prefixed verbatim
        // form, which `cmd.exe` (the default shell) refuses as a starting
        // directory -- it prints "CMD.EXE was started with the above path
        // as the current directory. UNC paths are not supported. Defaulting
        // to Windows directory." and silently starts somewhere else
        // entirely, which is exactly the failure a smoke test of this
        // feature turned up. `dunce::canonicalize` resolves the same real
        // path but strips that prefix whenever the plain (non-verbatim)
        // form is representable, which it always is for the ordinary local
        // paths a startup layout's `root_dir` points at.
        let base_dir = dunce::canonicalize(path)
            .map_err(|err| {
                anyhow::anyhow!(
                    "Error resolving the directory of {}: {}",
                    path.display(),
                    err
                )
            })?
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        layout.root_dir = resolve_relative_root_dir(layout.root_dir, &base_dir);
        for tab in &mut layout.tabs {
            tab.root_dir = resolve_relative_root_dir(tab.root_dir.take(), &base_dir);
        }

        Ok(layout)
    }
}

/// Joins a relative `root_dir` onto `base_dir` (the layout file's own
/// directory); an absolute `root_dir` (or none at all) passes through
/// unchanged.
fn resolve_relative_root_dir(root_dir: Option<PathBuf>, base_dir: &Path) -> Option<PathBuf> {
    root_dir.map(|dir| {
        if dir.is_relative() {
            base_dir.join(dir)
        } else {
            dir
        }
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parses_global_and_per_tab_vars_commands_titles() {
        let doc = r#"
vars: {
    GLOBAL_VAR: global-value
    SHARED: from-global
}

commands: [
    echo global-one
]

tabs: [
    {
        title: tab1
        vars: {
            SHARED: from-tab1
            TAB1_ONLY: tab1-value
        }
        commands: [
            echo tab1-hello
        ]
    }
    {
        title: tab2
        commands: [
            echo tab2-hello
        ]
    }
]
"#;
        let dir = std::env::temp_dir();
        let path = dir.join("onlyterm-start-conf-test-parses.ktav");
        std::fs::write(&path, doc).unwrap();

        let layout = StartupLayout::load(&path).expect("layout should parse");
        std::fs::remove_file(&path).ok();

        assert_eq!(
            layout.vars.get("GLOBAL_VAR").map(String::as_str),
            Some("global-value")
        );
        assert_eq!(layout.commands, vec!["echo global-one".to_string()]);
        assert_eq!(layout.tabs.len(), 2);

        let tab1 = &layout.tabs[0];
        assert_eq!(tab1.title.as_deref(), Some("tab1"));
        assert_eq!(
            tab1.vars.get("SHARED").map(String::as_str),
            Some("from-tab1")
        );
        assert_eq!(
            tab1.vars.get("TAB1_ONLY").map(String::as_str),
            Some("tab1-value")
        );
        assert_eq!(tab1.commands, vec!["echo tab1-hello".to_string()]);

        let tab2 = &layout.tabs[1];
        assert_eq!(tab2.title.as_deref(), Some("tab2"));
        assert!(tab2.vars.is_empty());
        assert_eq!(tab2.commands, vec!["echo tab2-hello".to_string()]);
    }

    #[test]
    fn global_root_dir_relative_path_resolves_against_layout_file_directory() {
        let dir = std::env::temp_dir().join("onlyterm-start-conf-test-root-dir-global");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("layout.ktav");
        std::fs::write(&path, "root_dir: sub/project\ntabs: [ { title: t1 } ]\n").unwrap();

        let layout = StartupLayout::load(&path).expect("layout should parse");
        let expected = dunce::canonicalize(&dir)
            .unwrap()
            .join("sub")
            .join("project");
        assert!(
            !expected.as_os_str().to_string_lossy().starts_with(r"\\?\"),
            "test setup bug: expected path should already be the non-verbatim \
             form, got {:?}",
            expected
        );
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(layout.root_dir, Some(expected));
        assert_eq!(
            layout.tabs[0].root_dir, None,
            "a tab that doesn't set its own root_dir should stay None here -- \
             falling back to the layout-wide root_dir happens later, at \
             spawn time, not by baking it into every tab"
        );
    }

    #[test]
    fn per_tab_root_dir_resolves_independently_of_the_global_one() {
        let dir = std::env::temp_dir().join("onlyterm-start-conf-test-root-dir-per-tab");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("layout.ktav");
        std::fs::write(
            &path,
            r#"
root_dir: global_dir
tabs: [
    {
        title: t1
        root_dir: tab_dir
    }
    {
        title: t2
    }
]
"#,
        )
        .unwrap();

        let layout = StartupLayout::load(&path).expect("layout should parse");
        let base = dunce::canonicalize(&dir).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert!(
            !base.as_os_str().to_string_lossy().starts_with(r"\\?\"),
            "test setup bug: expected base path should already be the \
             non-verbatim form, got {:?}",
            base
        );

        assert_eq!(layout.root_dir, Some(base.join("global_dir")));
        assert_eq!(layout.tabs[0].root_dir, Some(base.join("tab_dir")));
        assert_eq!(layout.tabs[1].root_dir, None);
    }

    /// Regression test for a bug a smoke test of `--start-conf` turned up:
    /// resolving a relative `root_dir` via plain `Path::canonicalize` (the
    /// original implementation) produces Windows' `\\?\`-prefixed verbatim
    /// path form. `cmd.exe` (the default Windows shell) doesn't accept that
    /// form as a starting directory -- it prints "CMD.EXE was started with
    /// the above path as the current directory. UNC paths are not
    /// supported. Defaulting to Windows directory." and starts somewhere
    /// else entirely, silently defeating `root_dir`. `StartupLayout::load`
    /// now resolves relative `root_dir` via `dunce::canonicalize` instead,
    /// specifically to avoid producing this prefix.
    #[test]
    #[cfg(windows)]
    fn resolved_root_dir_never_carries_the_windows_verbatim_path_prefix() {
        let dir = std::env::temp_dir().join("onlyterm-start-conf-test-root-dir-no-unc-prefix");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("layout.ktav");
        std::fs::write(
            &path,
            "root_dir: sub\ntabs: [ { title: t1, root_dir: sub } ]\n",
        )
        .unwrap();

        let layout = StartupLayout::load(&path).expect("layout should parse");
        std::fs::remove_dir_all(&dir).ok();

        for resolved in [&layout.root_dir, &layout.tabs[0].root_dir] {
            let resolved = resolved.as_ref().expect("root_dir should have resolved");
            assert!(
                !resolved.as_os_str().to_string_lossy().starts_with(r"\\?\"),
                "root_dir must not carry the \\\\?\\ verbatim-path prefix -- \
                 cmd.exe rejects it as a starting directory; got {:?}",
                resolved
            );
        }
    }

    #[test]
    fn absolute_root_dir_passes_through_unchanged() {
        let dir = std::env::temp_dir().join("onlyterm-start-conf-test-root-dir-absolute");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("layout.ktav");
        // Unquoted Windows drive-letter paths are unambiguous in ktav: the
        // pair separator is only ever the first `:` that is followed by
        // whitespace (spec 0.6.0 5.3), and `C:/...`'s colon is immediately
        // followed by `/`, not whitespace -- so it's just part of the
        // value, the same way ktav already tolerates bare `http://...`
        // URLs without quoting.
        std::fs::write(
            &path,
            "root_dir: C:/some/absolute/dir\ntabs: [ { title: t1 } ]\n",
        )
        .unwrap();

        let layout = StartupLayout::load(&path).expect("layout should parse");
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(layout.root_dir, Some(PathBuf::from("C:/some/absolute/dir")));
    }

    #[test]
    fn empty_tabs_is_rejected() {
        let doc = "tabs: []\n";
        let dir = std::env::temp_dir();
        let path = dir.join("onlyterm-start-conf-test-empty.ktav");
        std::fs::write(&path, doc).unwrap();

        let err = StartupLayout::load(&path).unwrap_err();
        std::fs::remove_file(&path).ok();

        assert!(
            err.to_string().contains("no tabs defined"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn missing_tabs_is_rejected() {
        let doc = "vars: { FOO: bar }\n";
        let dir = std::env::temp_dir();
        let path = dir.join("onlyterm-start-conf-test-missing.ktav");
        std::fs::write(&path, doc).unwrap();

        let err = StartupLayout::load(&path).unwrap_err();
        std::fs::remove_file(&path).ok();

        assert!(
            err.to_string().contains("no tabs defined"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn non_object_top_level_is_rejected() {
        let doc = "[ echo hello ]\n";
        let dir = std::env::temp_dir();
        let path = dir.join("onlyterm-start-conf-test-array.ktav");
        std::fs::write(&path, doc).unwrap();

        let err = StartupLayout::load(&path).unwrap_err();
        std::fs::remove_file(&path).ok();

        assert!(
            err.to_string().contains("top-level ktav object"),
            "unexpected error: {}",
            err
        );
    }

    /// Loads a layout from an inline document, using a caller-chosen file
    /// name so parallel tests never race on the same temp path.
    fn load_doc(name: &str, doc: &str) -> anyhow::Result<StartupLayout> {
        let path = std::env::temp_dir().join(format!("onlyterm-start-conf-{name}.ktav"));
        std::fs::write(&path, doc).unwrap();
        let result = StartupLayout::load(&path);
        std::fs::remove_file(&path).ok();
        result
    }

    #[test]
    fn shell_priority_and_admin_parse_in_their_documented_spelling() {
        let layout = load_doc(
            "tab-options",
            r#"
tabs: [
    {
        shell: powershell
        priority: above_normal
        admin: true
    }
]
"#,
        )
        .expect("layout should parse");

        let options = layout.tab_options(&layout.tabs[0]);
        assert_eq!(options.shell, Some(Shell::Powershell));
        assert_eq!(options.priority, ProcessPriority::AboveNormal);
        assert!(options.admin);
    }

    /// Layout-wide values apply to a tab that says nothing, exactly like
    /// `root_dir` and `vars` already do.
    #[test]
    fn a_tab_inherits_the_layouts_options() {
        let layout = load_doc(
            "tab-options-inherit",
            r#"
shell: bash
priority: high
admin: true

tabs: [
    {
        title: inherits-everything
    }
]
"#,
        )
        .expect("layout should parse");

        let options = layout.tab_options(&layout.tabs[0]);
        assert_eq!(options.shell, Some(Shell::Bash));
        assert_eq!(options.priority, ProcessPriority::High);
        assert!(options.admin);
    }

    /// ...and a tab that does say something wins over the layout, including
    /// when it turns an inherited `admin: true` back off.
    #[test]
    fn a_tabs_own_options_override_the_layouts() {
        let layout = load_doc(
            "tab-options-override",
            r#"
shell: bash
priority: high
admin: true

tabs: [
    {
        shell: cmd
        priority: idle
        admin: false
    }
]
"#,
        )
        .expect("layout should parse");

        let options = layout.tab_options(&layout.tabs[0]);
        assert_eq!(options.shell, Some(Shell::Cmd));
        assert_eq!(options.priority, ProcessPriority::Idle);
        assert!(
            !options.admin,
            "a tab must be able to opt back out of an inherited admin: true"
        );
    }

    /// Saying nothing anywhere has to keep behaving exactly as it did before
    /// these three settings existed: default_prog, normal priority, no
    /// elevation.
    #[test]
    fn a_layout_that_sets_nothing_keeps_the_previous_defaults() {
        let layout = load_doc("tab-options-absent", "tabs: [ { title: plain } ]\n")
            .expect("layout should parse");

        let options = layout.tab_options(&layout.tabs[0]);
        assert_eq!(options.shell, None);
        assert_eq!(options.priority, ProcessPriority::Normal);
        assert!(!options.admin);
    }

    #[test]
    fn a_misspelled_shell_is_rejected_rather_than_ignored() {
        let err = load_doc("tab-options-bad-shell", "tabs: [ { shell: fish } ]\n")
            .expect_err("an unknown shell must not be silently ignored");
        assert!(
            err.to_string().contains("fish"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn unknown_field_is_rejected() {
        let doc = "tabs: [ { title: t1 } ]\nbogus_field: 1\n";
        let dir = std::env::temp_dir();
        let path = dir.join("onlyterm-start-conf-test-unknown.ktav");
        std::fs::write(&path, doc).unwrap();

        let err = StartupLayout::load(&path).unwrap_err();
        std::fs::remove_file(&path).ok();

        assert!(
            err.to_string().contains("bogus_field") || err.to_string().contains("unknown"),
            "unexpected error: {}",
            err
        );
    }
}
