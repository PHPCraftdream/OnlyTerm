fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        use anyhow::Context as _;
        use std::io::Write;
        use std::path::Path;
        let repo_dir = std::env::current_dir()
            .ok()
            .and_then(|cwd| {
                cwd.parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf())
            })
            .unwrap();
        // Derive the actual target/<profile> directory from OUT_DIR
        // (<target_dir>/<profile>/build/<pkg>-<hash>/out) rather than
        // assuming `<repo_dir>/target/<profile>`, since CARGO_TARGET_DIR
        // (env var or .cargo/config.toml) can point the real target dir
        // elsewhere.
        let out_dir = std::env::var("OUT_DIR").unwrap();
        let exe_output_dir = Path::new(&out_dir)
            .ancestors()
            .nth(3)
            .expect("OUT_DIR should be nested 3 levels under target/<profile>")
            .to_path_buf();
        let windows_dir = repo_dir.join("assets").join("windows");

        let conhost_dir = windows_dir.join("conhost");
        for name in &["conpty.dll", "OpenConsole.exe"] {
            let dest_name = exe_output_dir.join(name);
            let src_name = conhost_dir.join(name);

            if !dest_name.exists() {
                std::fs::copy(&src_name, &dest_name)
                    .context(format!(
                        "copy {} -> {}",
                        src_name.display(),
                        dest_name.display()
                    ))
                    .unwrap();
            }
        }

        // If a file named `.tag` is present, we'll take its contents for the
        // version number that we report in onlyterm -h.
        let mut ci_tag = String::new();
        if let Ok(tag) = std::fs::read("../../.tag") {
            if let Ok(s) = String::from_utf8(tag) {
                ci_tag = s.trim().to_string();
                println!("cargo:rerun-if-changed=../../.tag");
            }
        }
        // Mirrors `onlyterm-version/build.rs`'s derivation (duplicated, not
        // shared: build scripts compile and run independently per crate,
        // same as the `.tag`-file handling just above already is). Prefer
        // the nearest reachable `v*` tag, e.g. `v0.0.14-alpha` (or
        // `v0.0.14-alpha-3-g1234567` for commits past the tag), so this
        // resource's version matches what `onlyterm -h`/`onlyterm_version()`
        // reports; fall back to the plain date-hash form when no tag is
        // reachable at all. Without this, a local build with no `.tag` file
        // fell straight to the date-hash form even when a perfectly good
        // release tag existed, so this resource's version and
        // `onlyterm_version()`'s could disagree.
        let ci_tag = if !ci_tag.is_empty() {
            ci_tag
        } else {
            std::process::Command::new("git")
                .args(["describe", "--tags", "--always", "--dirty=-dirty"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_default()
        };
        let commit_hash = std::process::Command::new("git")
            .args(["rev-parse", "--short=8", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string());
        let commit_count: u16 = std::process::Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8_lossy(&output.stdout).trim().parse().ok())
            // A `FILEVERSION` component is a 16-bit `WORD`; the commit count
            // is used as a monotonically increasing build number there, so
            // saturate rather than silently wrap once history passes 65535
            // commits (not close today, but wrapping back to a *smaller*
            // build number would be actively misleading, not just wrong).
            .unwrap_or(u16::MAX);
        let build_date = std::process::Command::new("git")
            .args(["show", "-s", "--format=%cd", "--date=format:%Y-%m-%d"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string());

        let version = if ci_tag.is_empty() {
            let mut cmd = std::process::Command::new("git");
            cmd.args([
                "-c",
                "core.abbrev=8",
                "show",
                "-s",
                "--format=%cd-%h",
                "--date=format:%Y%m%d-%H%M%S",
            ]);
            if let Ok(output) = cmd.output() {
                if output.status.success() {
                    String::from_utf8_lossy(&output.stdout).trim().to_owned()
                } else {
                    "UNKNOWN".to_owned()
                }
            } else {
                "UNKNOWN".to_owned()
            }
        } else {
            ci_tag
        };
        // The human-facing version string embedded in `StringFileInfo`
        // below: the tag/describe form plus the build date, commit hash and
        // ordinal commit number -- the last two of which plain `git
        // describe` output doesn't always carry (a checkout sitting exactly
        // on a release tag has no `-g<hash>` suffix at all, and never carries
        // a commit count).
        let version_with_provenance =
            format!("{version} ({build_date}, commit #{commit_count}, {commit_hash})");
        // `FILEVERSION`/`PRODUCTVERSION` are the raw 4x`WORD` numeric fields
        // -- what Windows Error Reporting's crash dialog and the "version:"
        // line in Application/APPCRASH event log entries actually read, as
        // opposed to the `StringFileInfo` values above (which is what
        // Explorer's file-properties dialog shows). Those were hardcoded to
        // `1,0,0,0` and so every crash report from every build looked
        // identical regardless of what was actually running. Parsed from
        // `version`'s leading `major.minor.patch` (after an optional `v`),
        // stopping at the first character that isn't a digit or a `.` --
        // handles both the bare `v0.0.14-alpha` form and the
        // `v0.0.14-alpha-3-g1234567` form `git describe` produces for
        // commits past the tag. The 4th component is the commit count, a
        // monotonically increasing build number that ordinary
        // major.minor.patch has no room for.
        fn parse_major_minor_patch(version: &str) -> (u16, u16, u16) {
            let s = version.strip_prefix('v').unwrap_or(version);
            let mut parts = [0u16; 3];
            let mut idx = 0;
            let mut cur = String::new();
            for ch in s.chars() {
                if ch.is_ascii_digit() {
                    cur.push(ch);
                } else if ch == '.' && idx < 2 {
                    parts[idx] = cur.parse().unwrap_or(0);
                    cur.clear();
                    idx += 1;
                } else {
                    break;
                }
            }
            parts[idx] = cur.parse().unwrap_or(0);
            (parts[0], parts[1], parts[2])
        }
        let (ver_major, ver_minor, ver_patch) = parse_major_minor_patch(&version);

        let rcfile_name = Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("resource.rc");
        let mut rcfile = std::fs::File::create(&rcfile_name).unwrap();
        println!("cargo:rerun-if-changed=../../assets/windows/terminal.ico");
        write!(
            rcfile,
            r#"
#include <winres.h>
// This ID is coupled with code in window/src/os/windows/window.rs
#define IDI_ICON 0x101
1 RT_MANIFEST "{win}\\manifest.manifest"
IDI_ICON ICON "{win}\\terminal.ico"
VS_VERSION_INFO VERSIONINFO
FILEVERSION     {ver_major},{ver_minor},{ver_patch},{commit_count}
PRODUCTVERSION  {ver_major},{ver_minor},{ver_patch},{commit_count}
FILEFLAGSMASK   VS_FFI_FILEFLAGSMASK
FILEFLAGS       0
FILEOS          VOS__WINDOWS32
FILETYPE        VFT_APP
FILESUBTYPE     VFT2_UNKNOWN
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904E4"
        BEGIN
            VALUE "CompanyName",      "Wez Furlong\0"
            VALUE "FileDescription",  "OnlyTerm - Terminal Emulator\0"
            VALUE "FileVersion",      "{version_with_provenance}\0"
            VALUE "LegalCopyright",   "Wez Furlong, MIT licensed\0"
            VALUE "InternalName",     "\0"
            VALUE "OriginalFilename", "\0"
            VALUE "ProductName",      "OnlyTerm\0"
            VALUE "ProductVersion",   "{version_with_provenance}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1252
    END
END
"#,
            win = windows_dir.display().to_string().replace("\\", "\\\\"),
            version_with_provenance = version_with_provenance,
        )
        .unwrap();
        drop(rcfile);

        // Obtain MSVC environment so that the rc compiler can find the right headers.
        // https://github.com/nabijaczleweli/rust-embed-resource/issues/11#issuecomment-603655972
        let target = std::env::var("TARGET").unwrap();
        if let Some(tool) = cc::windows_registry::find_tool(target.as_str(), "cl.exe") {
            for (key, value) in tool.env() {
                std::env::set_var(key, value);
            }
        }
        embed_resource::compile(rcfile_name);
    }
}
