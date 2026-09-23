use onlyterm_dynamic::{FromDynamic, ToDynamic};
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Windows process priority classes for spawned processes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToDynamic)]
pub enum ProcessPriority {
    /// Idle priority class (0x00000040)
    Idle,
    /// Below normal priority class (0x00004000)
    BelowNormal,
    /// Normal priority class (0x00000020) - default
    #[default]
    Normal,
    /// Above normal priority class (0x00008000)
    AboveNormal,
    /// High priority class (0x00000080)
    High,
    /// Realtime priority class (0x00000100)
    Realtime,
}

impl ProcessPriority {
    pub const ALL: [ProcessPriority; 6] = [
        ProcessPriority::Idle,
        ProcessPriority::BelowNormal,
        ProcessPriority::Normal,
        ProcessPriority::AboveNormal,
        ProcessPriority::High,
        ProcessPriority::Realtime,
    ];

    /// The lower-case, underscore-separated spelling used in `--start-conf`
    /// layouts.
    pub fn name(self) -> &'static str {
        match self {
            ProcessPriority::Idle => "idle",
            ProcessPriority::BelowNormal => "below_normal",
            ProcessPriority::Normal => "normal",
            ProcessPriority::AboveNormal => "above_normal",
            ProcessPriority::High => "high",
            ProcessPriority::Realtime => "realtime",
        }
    }
}

/// Hand-written rather than derived so that spelling is forgiving: matching
/// is case-insensitive and ignores `_`, `-` and spaces. This is strictly
/// more permissive than the derive it replaces, so every config that already
/// said `AboveNormal` keeps working, and `above_normal` -- the spelling
/// `--start-conf` layouts document -- now works too.
impl FromDynamic for ProcessPriority {
    fn from_dynamic(
        value: &onlyterm_dynamic::Value,
        options: onlyterm_dynamic::FromDynamicOptions,
    ) -> Result<Self, onlyterm_dynamic::Error> {
        let s = String::from_dynamic(value, options)?;
        let token = crate::shell::normalize_token(&s);
        Self::ALL
            .iter()
            .copied()
            .find(|p| crate::shell::normalize_token(p.name()) == token)
            .ok_or_else(|| {
                onlyterm_dynamic::Error::Message(format!(
                    "`{s}` is not a valid process priority; use one of {}",
                    Self::ALL
                        .iter()
                        .map(|p| p.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
    }
}

#[cfg(windows)]
impl ProcessPriority {
    /// Convert the enum to the corresponding Win32 priority class constant.
    pub fn to_win32_flag(self) -> u32 {
        match self {
            ProcessPriority::Idle => 0x00000040,
            ProcessPriority::BelowNormal => 0x00004000,
            ProcessPriority::Normal => 0x00000020,
            ProcessPriority::AboveNormal => 0x00008000,
            ProcessPriority::High => 0x00000080,
            ProcessPriority::Realtime => 0x00000100,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn test_priority_to_win32_flag() {
        assert_eq!(ProcessPriority::Idle.to_win32_flag(), 0x00000040);
        assert_eq!(ProcessPriority::BelowNormal.to_win32_flag(), 0x00004000);
        assert_eq!(ProcessPriority::Normal.to_win32_flag(), 0x00000020);
        assert_eq!(ProcessPriority::AboveNormal.to_win32_flag(), 0x00008000);
        assert_eq!(ProcessPriority::High.to_win32_flag(), 0x00000080);
        assert_eq!(ProcessPriority::Realtime.to_win32_flag(), 0x00000100);
    }

    #[test]
    fn test_priority_default() {
        assert_eq!(ProcessPriority::default(), ProcessPriority::Normal);
    }
}

/// When spawning a tab, specify which domain should be used to
/// host/spawn that tab.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, FromDynamic, ToDynamic, Default)]
pub enum SpawnTabDomain {
    /// Use the default domain
    DefaultDomain,
    /// Use the domain from the current tab in the associated window
    #[default]
    CurrentPaneDomain,
    /// Use a specific domain by name
    DomainName(String),
    /// Use a specific domain by id
    DomainId(usize),
}

#[derive(Default, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct SpawnCommand {
    /// Optional descriptive label
    pub label: Option<String>,

    /// Explicit tab title to apply as soon as the tab is spawned.
    /// If omitted, `Config::default_tab_title` is used instead; if that is
    /// also unset, the tab falls back to its cwd basename. Either way this
    /// is a one-shot value applied at spawn time, not a live template --
    /// an explicit rename (F2 / `onlyterm cli set-tab-title`) always
    /// overrides it afterwards.
    pub title: Option<String>,

    /// The command line to use.
    /// If omitted, the default command associated with the
    /// domain will be used instead, which is typically the
    /// shell for the user.
    pub args: Option<Vec<String>>,

    /// Specifies the current working directory for the command.
    /// If omitted, a default will be used; typically that will
    /// be the home directory of the user, but may also be the
    /// current working directory of the onlyterm process when
    /// it was launched, or for some domains it may be some
    /// other location appropriate to the domain.
    pub cwd: Option<PathBuf>,

    /// Specifies a map of environment variables that should be set.
    /// Whether this is used depends on the domain.
    #[dynamic(default)]
    pub set_environment_variables: HashMap<String, String>,

    #[dynamic(default)]
    pub domain: SpawnTabDomain,

    pub position: Option<crate::GuiPosition>,

    /// Process priority class for the spawned process (Windows only).
    /// If omitted, defaults to Normal priority.
    #[dynamic(default)]
    pub priority: Option<ProcessPriority>,
}

impl std::fmt::Debug for SpawnCommand {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self)
    }
}

impl std::fmt::Display for SpawnCommand {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "SpawnCommand")?;
        if let Some(label) = &self.label {
            write!(fmt, " label='{}'", label)?;
        }
        if let Some(title) = &self.title {
            write!(fmt, " title='{}'", title)?;
        }
        write!(fmt, " domain={:?}", self.domain)?;
        if let Some(args) = &self.args {
            write!(fmt, " args={:?}", args)?;
        }
        if let Some(cwd) = &self.cwd {
            write!(fmt, " cwd={}", cwd.display())?;
        }
        if let Some(priority) = &self.priority {
            write!(fmt, " priority={:?}", priority)?;
        }
        for (k, v) in &self.set_environment_variables {
            write!(fmt, " {}={}", k, v)?;
        }
        Ok(())
    }
}

impl SpawnCommand {
    pub fn label_for_palette(&self) -> Option<String> {
        if let Some(label) = &self.label {
            Some(label.to_string())
        } else if let Some(args) = &self.args {
            Some(shlex::try_join(args.iter().map(|s| s.as_str())).ok()?)
        } else {
            None
        }
    }

    pub fn from_command_builder(cmd: &CommandBuilder) -> anyhow::Result<Self> {
        let mut args = vec![];
        let mut set_environment_variables = HashMap::new();
        for arg in cmd.get_argv() {
            args.push(
                arg.to_str()
                    .ok_or_else(|| anyhow::anyhow!("command argument is not utf8"))?
                    .to_string(),
            );
        }
        for (k, v) in cmd.iter_full_env_as_str() {
            set_environment_variables.insert(k.to_string(), v.to_string());
        }
        let cwd = cmd.get_cwd().map(PathBuf::from);
        Ok(Self {
            label: None,
            title: None,
            domain: SpawnTabDomain::DefaultDomain,
            args: if args.is_empty() { None } else { Some(args) },
            set_environment_variables,
            cwd,
            position: None,
            priority: None,
        })
    }
}
