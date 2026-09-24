#![warn(clippy::undocumented_unsafe_blocks)]

#[path = "client/mod.rs"]
mod client_modules;
pub use client_modules::{activity, client, connui, ssh_agent};
mod model;
pub use model::{domain, localpane, pane, tab};
pub use tab::termwiztermtab;
#[cfg(test)]
mod test;
pub mod tmux;
pub use tmux::commands as tmux_commands;
#[path = "window/mod.rs"]
mod window_modules;
pub use window_modules::{renderable, window};

mod mux;
pub(crate) use client::ClientId;
#[cfg(test)]
pub(crate) use config::configuration;
pub(crate) use config::ExitBehavior;
pub(crate) use domain::{Domain, DomainId};
pub(crate) use mux::terminal_size_to_pty_size;
#[cfg(test)]
pub(crate) use mux::PANE_OUTPUT_MAX_SYNC_ROUNDS;
pub use mux::*;
pub(crate) use pane::{Pane, PaneId};
pub(crate) use ssh_agent::AgentProxy;
pub(crate) use tab::{Tab, TabId};
pub(crate) use window::WindowId;
pub(crate) use window_modules::pty_reader::read_from_pane_pty;
#[cfg(test)]
pub(crate) use window_modules::pty_reader::{hold_timeout_from, parse_buffered_data};
