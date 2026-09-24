use crate::client::{ClientId, ClientInfo};
use crate::domain::{Domain, DomainId, DomainState, SplitSource};
use crate::pane::{CachePolicy, Pane, PaneId};
use crate::ssh_agent::AgentProxy;
use crate::tab::{NotifyMux, SplitRequest, Tab, TabId};
use crate::window::{Window, WindowId};
use anyhow::{anyhow, Context, Error};
use onlyterm_config::keyassignment::{RotationDirection, SpawnTabDomain};
use onlyterm_config::{configuration, GuiPosition};
use onlyterm_term::{Clipboard, ClipboardSelection, DownloadHandler, TerminalSize};
use parking_lot::{
    MappedRwLockReadGuard, MappedRwLockWriteGuard, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
use percent_encoding::percent_decode_str;
use portable_pty::{CommandBuilder, ExitStatus, PtySize};
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use thiserror::*;

use crate::activity::Activity;

mod domains;
mod notifications;
mod panes;
mod spawn;
mod state;
mod windows;

pub const DEFAULT_WORKSPACE: &str = "default";

#[derive(Clone, Debug)]
pub enum MuxNotification {
    PaneOutput(PaneId),
    PaneAdded(PaneId),
    PaneRemoved(PaneId),
    WindowCreated(WindowId),
    WindowRemoved(WindowId),
    WindowInvalidated(WindowId),
    WindowWorkspaceChanged(WindowId),
    ActiveWorkspaceChanged(Arc<ClientId>),
    Alert {
        pane_id: PaneId,
        alert: onlyterm_term::Alert,
    },
    Empty,
    AssignClipboard {
        pane_id: PaneId,
        selection: ClipboardSelection,
        clipboard: Option<String>,
    },
    SaveToDownloads {
        name: Option<String>,
        data: Arc<Vec<u8>>,
    },
    TabAddedToWindow {
        tab_id: TabId,
        window_id: WindowId,
    },
    PaneFocused(PaneId),
    TabReflowed(TabId),
    TabTitleChanged {
        tab_id: TabId,
        title: String,
    },
    WindowTitleChanged {
        window_id: WindowId,
        title: String,
    },
    WorkspaceRenamed {
        old_workspace: String,
        new_workspace: String,
    },
}

static LAST_SUBSCRIBER_ID: AtomicUsize = AtomicUsize::new(0);

type MuxSubscriber = Arc<dyn Fn(MuxNotification) -> bool + Send + Sync>;

/// Per-pane state for coalescing PaneOutput notifications.
/// Tracks: (is_delivery_in_flight, has_more_output_pending)
type PaneOutputState = (bool, bool, usize);

pub struct Mux {
    tabs: RwLock<HashMap<TabId, Arc<Tab>>>,
    panes: RwLock<HashMap<PaneId, Arc<dyn Pane>>>,
    windows: RwLock<HashMap<WindowId, Window>>,
    pub(crate) default_domain: RwLock<Option<Arc<dyn Domain>>>,
    pub(crate) domains: RwLock<HashMap<DomainId, Arc<dyn Domain>>>,
    pub(crate) domains_by_name: RwLock<HashMap<String, Arc<dyn Domain>>>,
    subscribers: RwLock<HashMap<usize, MuxSubscriber>>,
    banner: RwLock<Option<String>>,
    clients: RwLock<HashMap<ClientId, ClientInfo>>,
    identity: RwLock<Option<Arc<ClientId>>>,
    num_panes_by_workspace: RwLock<HashMap<String, usize>>,
    main_thread_id: std::thread::ThreadId,
    pub(crate) agent: Option<AgentProxy>,
    /// Coalescing state for `MuxNotification::PaneOutput`: maps pane id
    /// to (in_flight, has_more_output, coalesce_count). When new output arrives:
    ///
    /// - If in_flight is false, we schedule delivery and set it to true
    /// - If in_flight is true, we set has_more_output=true and increment coalesce_count
    /// - When delivery completes, we check if coalesce_count increased (meaning output
    ///   arrived during the callback) vs if it was already set before (pre-drain coalescing).
    pane_output_notify_state: Mutex<HashMap<PaneId, PaneOutputState>>,
}

lazy_static::lazy_static! {
    static ref MUX: Mutex<Option<Arc<Mux>>> = Mutex::new(None);
}

/// How many consecutive `PaneOutput` rounds `Mux::notify` will redeliver
/// synchronously (in a loop, not via recursion) before yielding back to the
/// event loop. A pane with sustained, continuous output can keep
/// `coalesce_count` incrementing indefinitely; without a cap, delivering
/// every round from the same call would either recurse without bound (the
/// original bug) or spin the calling thread forever, starving every other
/// pane/window of a chance to run. See `Mux::notify`'s reschedule loop.
pub(crate) const PANE_OUTPUT_MAX_SYNC_ROUNDS: usize = 64;

pub struct MuxWindowBuilder {
    pub(crate) window_id: WindowId,
    pub(crate) activity: Option<Activity>,
    pub(crate) notified: bool,
}

impl MuxWindowBuilder {
    pub(crate) fn notify(&mut self) {
        if self.notified {
            return;
        }
        self.notified = true;
        let activity = self.activity.take().unwrap();
        let window_id = self.window_id;
        let mux = Mux::get();
        if mux.is_main_thread() {
            // If we're already on the mux thread, just send the notification
            // immediately.
            // This is super important for Wayland; if we push it to the
            // spawn queue below then the extra milliseconds of delay
            // causes it to get confused and shutdown the connection!?
            mux.notify(MuxNotification::WindowCreated(window_id));
        } else {
            onlyterm_promise::spawn::spawn_into_main_thread(async move {
                if let Some(mux) = Mux::try_get() {
                    mux.notify(MuxNotification::WindowCreated(window_id));
                    drop(activity);
                }
            })
            .detach();
        }
    }
}

impl Drop for MuxWindowBuilder {
    fn drop(&mut self) {
        self.notify();
    }
}

impl std::ops::Deref for MuxWindowBuilder {
    type Target = WindowId;

    fn deref(&self) -> &WindowId {
        &self.window_id
    }
}

pub struct IdentityHolder {
    prior: Option<Arc<ClientId>>,
}

impl Drop for IdentityHolder {
    fn drop(&mut self) {
        if let Some(mux) = Mux::try_get() {
            mux.replace_identity(self.prior.take());
        }
    }
}

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum SessionTerminated {
    #[error("Process exited: {:?}", status)]
    ProcessStatus { status: ExitStatus },
    #[error("Error: {:?}", err)]
    Error { err: Error },
    #[error("Window Closed")]
    WindowClosed,
}

pub(crate) fn terminal_size_to_pty_size(size: TerminalSize) -> anyhow::Result<PtySize> {
    Ok(PtySize {
        rows: size.rows.try_into()?,
        cols: size.cols.try_into()?,
        pixel_height: size.pixel_height.try_into()?,
        pixel_width: size.pixel_width.try_into()?,
    })
}

struct MuxClipboard {
    pane_id: PaneId,
}

impl Clipboard for MuxClipboard {
    fn set_contents(
        &self,
        selection: ClipboardSelection,
        clipboard: Option<String>,
    ) -> anyhow::Result<()> {
        let mux =
            Mux::try_get().ok_or_else(|| anyhow::anyhow!("MuxClipboard::set_contents: no Mux?"))?;
        mux.notify(MuxNotification::AssignClipboard {
            pane_id: self.pane_id,
            selection,
            clipboard,
        });
        Ok(())
    }
}

struct MuxDownloader {}

impl onlyterm_term::DownloadHandler for MuxDownloader {
    fn save_to_downloads(&self, name: Option<String>, data: Vec<u8>) {
        if let Some(mux) = Mux::try_get() {
            mux.notify(MuxNotification::SaveToDownloads {
                name,
                data: Arc::new(data),
            });
        }
    }
}

#[cfg(test)]
mod captrack_integration {
    //! Proof-of-wiring test for `captrack` (task #151). This does not touch
    //! any production code path: it only demonstrates that the `captrack`
    //! dependency resolves and its `t*!` macros compile/run in both the
    //! default (telemetry disabled, zero-overhead bare constructor) and
    //! `telemetry`-featured (tracked wrapper) configurations. See
    //! CONTRIBUTING.md ("Collection-capacity telemetry with captrack") for
    //! how to enable telemetry and dump stats.

    use captrack::tvec;

    #[test]
    fn tvec_demo_resolves_and_behaves_like_vec() {
        let mut v = tvec!("mux/demo/example", 8);
        v.push(1);
        v.push(2);
        v.push(3);
        assert_eq!(&v[..], &[1, 2, 3]);
    }
}
