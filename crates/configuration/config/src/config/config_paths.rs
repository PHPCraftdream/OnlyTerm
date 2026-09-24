use std::path::PathBuf;

pub(super) struct PathPossibility {
    pub(super) path: PathBuf,
    pub(super) is_required: bool,
}

impl PathPossibility {
    pub(super) fn required(path: PathBuf) -> PathPossibility {
        PathPossibility {
            path,
            is_required: true,
        }
    }

    pub(super) fn optional(path: PathBuf) -> PathPossibility {
        PathPossibility {
            path,
            is_required: false,
        }
    }
}

pub(crate) fn compute_cache_dir() -> anyhow::Result<PathBuf> {
    if let Some(runtime) = dirs_next::cache_dir() {
        return Ok(runtime.join("onlyterm"));
    }

    Ok(crate::HOME_DIR.join(".local/share/onlyterm"))
}

pub(crate) fn compute_data_dir() -> anyhow::Result<PathBuf> {
    if let Some(runtime) = dirs_next::data_dir() {
        return Ok(runtime.join("onlyterm"));
    }

    Ok(crate::HOME_DIR.join(".local/share/onlyterm"))
}

pub(crate) fn compute_runtime_dir() -> anyhow::Result<PathBuf> {
    if let Some(runtime) = dirs_next::runtime_dir() {
        return Ok(runtime.join("onlyterm"));
    }

    Ok(crate::HOME_DIR.join(".local/share/onlyterm"))
}

/// Distinguishes "no `.ktav` config exists at this candidate path, but a
/// legacy `.rhai`/`.lua` sibling does" from any other load error (I/O error,
/// a `.ktav` file that exists but fails to parse, etc). `load_with_overrides`
/// downcasts to this type so that a legacy sibling found next to an
/// earlier-searched candidate doesn't prevent it from continuing on to a
/// later candidate that might have a genuine, loadable `.ktav` config (task
/// #298 / bug F9): this case is deferred and only surfaced as a hard error if
/// no valid `.ktav` config is found anywhere in the whole search order.
#[derive(Debug)]
pub(super) struct LegacyScriptSiblingError {
    pub(super) script_path: PathBuf,
    pub(super) expected_path: PathBuf,
}

impl std::fmt::Display for LegacyScriptSiblingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Found a legacy scripted configuration file at {} but \
             scripted configs (rhai/Lua) are no longer supported: \
             the config-scripting engine has been removed from \
             onlyterm's live config-loading path in favor of the \
             static `ktav` format. Please migrate {} to the ktav \
             format and save it as {}. See the migration guide for \
             details.",
            self.script_path.display(),
            self.script_path.display(),
            self.expected_path.display()
        )
    }
}

impl std::error::Error for LegacyScriptSiblingError {}
