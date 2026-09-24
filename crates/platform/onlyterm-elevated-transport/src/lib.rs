//! WebSocket-based rendezvous transport for elevated single-pane tabs.
//!
//! `ShellExecuteExW("runas")` cannot pass an inheritable handle to the
//! elevated child the way ordinary `CreateProcess`-based spawning can --
//! there is no `STARTUPINFO`/handle-inheritance equivalent anywhere in
//! `SHELLEXECUTEINFOW`, and the actual elevated `CreateProcess` call is
//! made by the Application Information service after the UAC consent UI,
//! not by the calling process, so there is no handle table to project into
//! the child in the first place. So the `proxy_command`/anonymous-
//! socketpair transport the non-elevated single-pane path uses
//! (`onlyterm-gui`'s `spawn_single_pane_tab`) cannot be reused for an
//! elevated child.
//!
//! This crate implements the alternative: the GUI (non-elevated) opens a
//! loopback TCP listener and generates a random token
//! (`generate_rendezvous_token`); the elevated child is launched with that
//! port and token as CLI arguments and connects *out* to the GUI
//! (`connect_and_bridge`), authenticating with the token during the
//! WebSocket handshake (`RendezvousListener::accept` on the server side).
//! Once connected, the WebSocket carries the same mux PDU byte stream the
//! proxy_command transport carries, bridged via a background pump thread
//! onto a local `filedescriptor::socketpair()` end that the existing
//! `onlyterm-client`/`onlyterm-mux-server-impl` machinery consumes
//! unchanged on both sides (`Reconnectable` already accepts a
//! pre-connected stream on the client side; `dispatch::process` already
//! accepts any `AsRawDesc` stream on the server side).
//!
//! Both sides of this crate exist together deliberately: it is meant to be
//! the *only* place that understands the WebSocket framing/bridging, so
//! `onlyterm-gui` (the rendezvous server) and `onlyterm-mux-server` (the
//! rendezvous client) each depend on it without depending on each other --
//! `onlyterm-gui` is a binary crate and cannot be a library dependency of
//! anything else.

mod bridge;
mod rendezvous;

pub use rendezvous::{connect_and_bridge, generate_rendezvous_token, RendezvousListener};

#[cfg(test)]
mod tests;
