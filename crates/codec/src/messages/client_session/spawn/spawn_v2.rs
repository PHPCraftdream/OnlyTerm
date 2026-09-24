use mux::window::WindowId;
use onlyterm_term::TerminalSize;
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct SpawnV2 {
    pub domain: config::keyassignment::SpawnTabDomain,
    /// If None, create a new window for this new tab
    pub window_id: Option<WindowId>,
    pub command: Option<CommandBuilder>,
    pub command_dir: Option<String>,
    pub size: TerminalSize,
    pub workspace: String,
    /// If true, attach to the domain and reuse existing panes
    /// rather than unconditionally spawning a new tab.
    pub attach: bool,
}
