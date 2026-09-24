use config::keyassignment::*;
use std::borrow::Cow;
use window::{KeyCode, Modifiers};
use KeyAssignment::*;

/// Describes an argument/parameter/context that is required
/// in order for the command to have meaning.
/// The intent is for this to be used when filtering the items
/// that should be shown in eg: a context menu.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
// "Active" prefix is meaningful: these target the active pane/tab/window,
// distinguishing them from non-active command targets in filtering/menus.
#[allow(clippy::enum_variant_names)]
pub enum ArgType {
    /// Operates on the active pane
    ActivePane,
    /// Operates on the active tab
    ActiveTab,
    /// Operates on the active window
    ActiveWindow,
}

/// `CommandDef` defines a command in the UI.
pub struct CommandDef {
    /// Brief description
    pub brief: Cow<'static, str>,
    /// A longer, more detailed, description
    pub doc: Cow<'static, str>,
    /// The key assignments associated with this command.
    pub keys: Vec<(Modifiers, String)>,
    /// The argument types/context in which this command is valid.
    pub args: &'static [ArgType],
    /// Where to place the command in a menubar
    pub menubar: &'static [&'static str],
    pub icon: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct ExpandedCommand {
    pub brief: Cow<'static, str>,
    pub doc: Cow<'static, str>,
    pub action: KeyAssignment,
    pub keys: Vec<(Modifiers, KeyCode)>,
    pub menubar: &'static [&'static str],
    pub icon: Option<Cow<'static, str>>,
}

impl std::fmt::Debug for CommandDef {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.debug_struct("CommandDef")
            .field("brief", &self.brief)
            .field("doc", &self.doc)
            .field("keys", &self.keys)
            .field("args", &self.args)
            .finish()
    }
}

mod command_def;
mod derive;

pub use derive::derive_command_from_key_assignment;
