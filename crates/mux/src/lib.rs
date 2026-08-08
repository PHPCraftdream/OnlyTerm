#![warn(clippy::undocumented_unsafe_blocks)]
use crate::client::{ClientId, ClientInfo};
use crate::pane::{CachePolicy, Pane, PaneId};
use crate::ssh_agent::AgentProxy;
use crate::tab::{NotifyMux, SplitRequest, Tab, TabId};
use crate::window::{Window, WindowId};
use anyhow::{anyhow, Context, Error};
use config::keyassignment::{RotationDirection, SpawnTabDomain};
use config::{configuration, ExitBehavior, GuiPosition};
use domain::{Domain, DomainId, DomainState, SplitSource};
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
use wezterm_term::{Clipboard, ClipboardSelection, DownloadHandler, TerminalSize};

pub mod activity;
pub mod client;
pub mod connui;
pub mod domain;
pub mod localpane;
pub mod pane;
mod pty_reader;
pub mod renderable;
pub mod ssh_agent;
pub mod tab;
pub mod termwiztermtab;
#[cfg(test)]
mod test;
pub mod tmux;
pub mod tmux_commands;
mod tmux_pty;
pub mod window;

use crate::activity::Activity;
pub(crate) use pty_reader::read_from_pane_pty;
#[cfg(test)]
pub(crate) use pty_reader::{hold_timeout_from, parse_buffered_data};

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
        alert: wezterm_term::Alert,
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
    default_domain: RwLock<Option<Arc<dyn Domain>>>,
    domains: RwLock<HashMap<DomainId, Arc<dyn Domain>>>,
    domains_by_name: RwLock<HashMap<String, Arc<dyn Domain>>>,
    subscribers: RwLock<HashMap<usize, MuxSubscriber>>,
    banner: RwLock<Option<String>>,
    clients: RwLock<HashMap<ClientId, ClientInfo>>,
    identity: RwLock<Option<Arc<ClientId>>>,
    num_panes_by_workspace: RwLock<HashMap<String, usize>>,
    main_thread_id: std::thread::ThreadId,
    agent: Option<AgentProxy>,
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

pub struct MuxWindowBuilder {
    window_id: WindowId,
    activity: Option<Activity>,
    notified: bool,
}

impl MuxWindowBuilder {
    fn notify(&mut self) {
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
            promise::spawn::spawn_into_main_thread(async move {
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

impl Mux {
    pub fn new(default_domain: Option<Arc<dyn Domain>>) -> Self {
        let mut domains = HashMap::new();
        let mut domains_by_name = HashMap::new();
        if let Some(default_domain) = default_domain.as_ref() {
            domains.insert(default_domain.domain_id(), Arc::clone(default_domain));

            domains_by_name.insert(
                default_domain.domain_name().to_string(),
                Arc::clone(default_domain),
            );
        }

        let agent = if config::configuration().mux_enable_ssh_agent {
            Some(AgentProxy::new())
        } else {
            None
        };

        Self {
            tabs: RwLock::new(HashMap::new()),
            panes: RwLock::new(HashMap::new()),
            windows: RwLock::new(HashMap::new()),
            default_domain: RwLock::new(default_domain),
            domains_by_name: RwLock::new(domains_by_name),
            domains: RwLock::new(domains),
            subscribers: RwLock::new(HashMap::new()),
            banner: RwLock::new(None),
            clients: RwLock::new(HashMap::new()),
            identity: RwLock::new(None),
            num_panes_by_workspace: RwLock::new(HashMap::new()),
            main_thread_id: std::thread::current().id(),
            agent,
            pane_output_notify_state: Mutex::new(HashMap::new()),
        }
    }

    fn get_default_workspace(&self) -> String {
        let config = configuration();
        config
            .default_workspace
            .as_deref()
            .unwrap_or(DEFAULT_WORKSPACE)
            .to_string()
    }

    pub fn is_main_thread(&self) -> bool {
        std::thread::current().id() == self.main_thread_id
    }

    fn recompute_pane_count(&self) {
        let mut count = HashMap::new();
        for window in self.windows.read().values() {
            let workspace = window.get_workspace();
            for tab in window.iter() {
                *count.entry(workspace.to_string()).or_insert(0) += match tab.count_panes() {
                    Some(n) => n,
                    None => {
                        // Busy: abort this and we'll retry later
                        return;
                    }
                };
            }
        }
        *self.num_panes_by_workspace.write() = count;
    }

    pub fn client_had_input(&self, client_id: &ClientId) {
        if let Some(info) = self.clients.write().get_mut(client_id) {
            info.update_last_input();
        }
        if let Some(agent) = &self.agent {
            agent.update_target();
        }
    }

    pub fn record_input_for_current_identity(&self) {
        if let Some(ident) = self.identity.read().as_ref() {
            self.client_had_input(ident);
        }
    }

    pub fn record_focus_for_current_identity(&self, pane_id: PaneId) {
        if let Some(ident) = self.identity.read().as_ref() {
            self.record_focus_for_client(ident, pane_id);
        }
    }

    pub fn resolve_focused_pane(
        &self,
        client_id: &ClientId,
    ) -> Option<(DomainId, WindowId, TabId, PaneId)> {
        let pane_id = self.clients.read().get(client_id)?.focused_pane_id?;
        let (domain, window, tab) = self.resolve_pane_id(pane_id)?;
        Some((domain, window, tab, pane_id))
    }

    pub fn record_focus_for_client(&self, client_id: &ClientId, pane_id: PaneId) {
        let mut prior = None;
        if let Some(info) = self.clients.write().get_mut(client_id) {
            prior = info.focused_pane_id;
            info.update_focused_pane(pane_id);
        }

        if prior == Some(pane_id) {
            return;
        }
        // Synthesize focus events
        if let Some(prior_id) = prior {
            if let Some(pane) = self.get_pane(prior_id) {
                pane.focus_changed(false);
            }
        }
        if let Some(pane) = self.get_pane(pane_id) {
            pane.focus_changed(true);
        }
    }

    /// Called by PaneFocused event handlers to reconcile a remote
    /// pane focus event and apply its effects locally
    pub fn focus_pane_and_containing_tab(&self, pane_id: PaneId) -> anyhow::Result<()> {
        let pane = self
            .get_pane(pane_id)
            .ok_or_else(|| anyhow::anyhow!("pane {pane_id} not found"))?;

        let (_domain, window_id, tab_id) = self
            .resolve_pane_id(pane_id)
            .ok_or_else(|| anyhow::anyhow!("can't find {pane_id} in the mux"))?;

        // Focus/activate the containing tab within its window
        {
            let mut win = self
                .get_window_mut(window_id)
                .ok_or_else(|| anyhow::anyhow!("window_id {window_id} not found"))?;
            let tab_idx = win
                .idx_by_id(tab_id)
                .ok_or_else(|| anyhow::anyhow!("tab {tab_id} not in {window_id}"))?;
            win.save_and_then_set_active(tab_idx);
        }

        // Focus/activate the pane locally
        let tab = self
            .get_tab(tab_id)
            .ok_or_else(|| anyhow::anyhow!("tab {tab_id} not found"))?;

        // Reconcile local focus to a change already decided elsewhere; must not
        // emit PaneFocused, or we'd re-broadcast (and, for client panes, echo it
        // back to the server). See <https://github.com/wezterm/wezterm/issues/4390>
        tab.set_active_pane_with_notify(&pane, NotifyMux::No);

        Ok(())
    }

    pub fn register_client(&self, client_id: Arc<ClientId>) {
        self.clients
            .write()
            .insert((*client_id).clone(), ClientInfo::new(client_id));
    }

    pub fn iter_clients(&self) -> Vec<ClientInfo> {
        self.clients.read().values().cloned().collect()
    }

    /// Returns a list of the unique workspace names known to the mux.
    /// This is taken from all known windows.
    pub fn iter_workspaces(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .windows
            .read()
            .values()
            .map(|w| w.get_workspace().to_string())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Generate a new unique workspace name
    pub fn generate_workspace_name(&self) -> String {
        let used = self.iter_workspaces();
        for candidate in names::Generator::default() {
            if !used.contains(&candidate) {
                return candidate;
            }
        }
        unreachable!();
    }

    /// Returns the effective active workspace name
    pub fn active_workspace(&self) -> String {
        self.identity
            .read()
            .as_ref()
            .and_then(|ident| {
                self.clients
                    .read()
                    .get(ident)
                    .and_then(|info| info.active_workspace.clone())
            })
            .unwrap_or_else(|| self.get_default_workspace())
    }

    /// Returns the effective active workspace name for a given client
    pub fn active_workspace_for_client(&self, ident: &Arc<ClientId>) -> String {
        self.clients
            .read()
            .get(ident)
            .and_then(|info| info.active_workspace.clone())
            .unwrap_or_else(|| self.get_default_workspace())
    }

    pub fn set_active_workspace_for_client(&self, ident: &Arc<ClientId>, workspace: &str) {
        let mut clients = self.clients.write();
        if let Some(info) = clients.get_mut(ident) {
            info.active_workspace.replace(workspace.to_string());
            self.notify(MuxNotification::ActiveWorkspaceChanged(ident.clone()));
        }
    }

    /// Assigns the active workspace name for the current identity
    pub fn set_active_workspace(&self, workspace: &str) {
        if let Some(ident) = self.identity.read().clone() {
            self.set_active_workspace_for_client(&ident, workspace);
        }
    }

    pub fn rename_workspace(&self, old_workspace: &str, new_workspace: &str) {
        if old_workspace == new_workspace {
            return;
        }
        self.notify(MuxNotification::WorkspaceRenamed {
            old_workspace: old_workspace.to_string(),
            new_workspace: new_workspace.to_string(),
        });

        for window in self.windows.write().values_mut() {
            if window.get_workspace() == old_workspace {
                window.set_workspace(new_workspace);
            }
        }
        self.recompute_pane_count();
        for client in self.clients.write().values_mut() {
            if client.active_workspace.as_deref() == Some(old_workspace) {
                client.active_workspace.replace(new_workspace.to_string());
                self.notify(MuxNotification::ActiveWorkspaceChanged(
                    client.client_id.clone(),
                ));
            }
        }
    }

    /// Overrides the current client identity.
    /// Returns `IdentityHolder` which will restore the prior identity
    /// when it is dropped.
    /// This can be used to change the identity for the duration of a block.
    pub fn with_identity(&self, id: Option<Arc<ClientId>>) -> IdentityHolder {
        let prior = self.replace_identity(id);
        IdentityHolder { prior }
    }

    /// Replace the identity, returning the prior identity
    pub fn replace_identity(&self, id: Option<Arc<ClientId>>) -> Option<Arc<ClientId>> {
        std::mem::replace(&mut *self.identity.write(), id)
    }

    /// Returns the active identity
    pub fn active_identity(&self) -> Option<Arc<ClientId>> {
        self.identity.read().clone()
    }

    pub fn unregister_client(&self, client_id: &ClientId) {
        self.clients.write().remove(client_id);
    }

    pub fn subscribe<F>(&self, subscriber: F)
    where
        F: Fn(MuxNotification) -> bool + 'static + Send + Sync,
    {
        let sub_id = LAST_SUBSCRIBER_ID.fetch_add(1, Ordering::Relaxed);
        self.subscribers
            .write()
            .insert(sub_id, Arc::new(subscriber));
    }

    pub fn notify(&self, notification: MuxNotification) {
        let (pane_id, initial_coalesce_count) =
            if let MuxNotification::PaneOutput(pane_id) = &notification {
                // Capture the coalesce_count BEFORE we start delivering.
                // If it increases during delivery, output arrived during the callback.
                let coalesce_count = {
                    let state = self.pane_output_notify_state.lock();
                    state.get(pane_id).map(|(_, _, count)| *count).unwrap_or(0)
                };
                (Some(*pane_id), coalesce_count)
            } else {
                (None, 0)
            };

        // Snapshot the subscriber ids+callbacks under a short-lived read
        // lock, then release it before invoking any callbacks. Subscriber
        // callbacks are arbitrary code (eg. GUI code that ends up calling
        // back into `Mux::get_window`, or even `Mux::subscribe` /
        // `Mux::notify` again); if we held `subscribers.write()` for the
        // duration of those calls -- as a naive `retain`-based
        // implementation would -- any callback that (transitively) touches
        // `self.subscribers` would deadlock against this non-reentrant
        // `RwLock`, and every other subscriber would be blocked from
        // observing the notification until the slowest callback returns.
        //
        // Note: cloning the `Arc<Fn>` payload (rather than the raw
        // `Box<Fn>`) is what makes a cheap snapshot possible without
        // moving the callbacks out of the map, so a second pass can later
        // reconcile the live set using the same ids.
        let snapshot: Vec<(usize, MuxSubscriber)> = {
            let subscribers = self.subscribers.read();
            subscribers
                .iter()
                .map(|(id, notify)| (*id, Arc::clone(notify)))
                .collect()
        };

        // Invoke the callbacks with no mux lock held at all, recording
        // which subscriber ids asked to be removed (by returning `false`).
        let mut dead = Vec::new();
        for (id, notify) in &snapshot {
            if !notify(notification.clone()) {
                dead.push(*id);
            }
        }

        // Reconcile: drop only the ids that asked to unsubscribe *and*
        // are still present (a concurrent `subscribe` could have reused
        // an id only via `LAST_SUBSCRIBER_ID`, which never repeats, so
        // this is simply "remove if still there").
        if !dead.is_empty() {
            let mut subscribers = self.subscribers.write();
            for id in dead {
                subscribers.remove(&id);
            }
        }

        // After all subscribers have been notified, clear the pending marker
        // for PaneOutput notifications and check if we need to schedule a
        // follow-up delivery. We schedule a follow-up only if coalesce_count
        // increased DURING the delivery (meaning output arrived from within
        // the callback), not if it was already set BEFORE the delivery started.
        if let Some(pane_id) = pane_id {
            let should_reschedule = {
                let mut state = self.pane_output_notify_state.lock();
                if let Some((_, has_more_output, coalesce_count)) = state.get_mut(&pane_id) {
                    // Check if coalesce_count increased during delivery.
                    // If it did, output arrived from within the callback.
                    let coalesce_count_increased = *coalesce_count > initial_coalesce_count;
                    if coalesce_count_increased {
                        // Keep in_flight=true for the new delivery, reset everything else
                        *coalesce_count = 0;
                        *has_more_output = false;
                    } else {
                        // No new output during delivery; clean up
                        state.remove(&pane_id);
                    }
                    coalesce_count_increased
                } else {
                    false
                }
            };

            if should_reschedule {
                // More output arrived while we were delivering; schedule another round.
                // We must call dispatch_notification OUTSIDE the lock to avoid
                // lock-ordering issues (dispatch may acquire other locks).
                metrics::counter!("mux.pane_output.rescheduled").increment(1);
                Self::dispatch_notification(MuxNotification::PaneOutput(pane_id));
            }
        }
    }

    /// Schedules `notification` for delivery on the main thread (or
    /// delivers it synchronously if already there).
    ///
    /// `PaneOutput` gets special-cased to coalesce: `parse_buffered_data`
    /// calls this once per parser flush (coalesce delay
    /// `mux_output_parser_coalesce_delay_ms`, default 3ms), which under
    /// sustained pty output can mean hundreds of calls per second per
    /// pane. Each call that reaches the `spawn_into_main_thread` branch
    /// below costs a hop through the main-thread spawn queue. To bound
    /// the queue depth, we track pending delivery per pane and only
    /// spawn when no delivery is already in-flight. When a delivery
    /// completes, we check if more output arrived and schedule exactly
    /// one more delivery if needed. `mux_pane_output_event` always
    /// inspects current pane state (not a snapshot carried by the
    /// notification), so the single delivery that does go through still
    /// reflects whatever is the latest state by the time it runs --
    /// coalescing only elides redundant wakeups, it never drops data.
    pub fn notify_from_any_thread(notification: MuxNotification) {
        if let MuxNotification::PaneOutput(pane_id) = &notification {
            let mux = match Mux::try_get() {
                Some(mux) => mux,
                None => {
                    return Self::dispatch_notification(notification);
                }
            };

            let should_schedule = {
                let mut state = mux.pane_output_notify_state.lock();
                let entry = state.entry(*pane_id).or_insert((false, false, 0));
                if entry.0 {
                    // Already delivering; just mark that more arrived
                    entry.1 = true;
                    entry.2 += 1; // Track that we coalesced during this delivery
                    metrics::counter!("mux.pane_output.coalesced").increment(1);
                    false
                } else {
                    // Not delivering; schedule delivery now
                    entry.0 = true;
                    entry.2 = 0; // Reset coalesce counter for new delivery
                    metrics::counter!("mux.pane_output.scheduled").increment(1);
                    true
                }
            };

            if !should_schedule {
                return;
            }
        }
        Self::dispatch_notification(notification);
    }

    fn dispatch_notification(notification: MuxNotification) {
        if let Some(mux) = Mux::try_get() {
            if mux.is_main_thread() {
                mux.notify(notification);
                return;
            }
        }
        promise::spawn::spawn_into_main_thread(async {
            if let Some(mux) = Mux::try_get() {
                mux.notify(notification);
            }
        })
        .detach();
    }

    pub fn default_domain(&self) -> Arc<dyn Domain> {
        self.default_domain.read().as_ref().map(Arc::clone).unwrap()
    }

    pub fn set_default_domain(&self, domain: &Arc<dyn Domain>) {
        *self.default_domain.write() = Some(Arc::clone(domain));
    }

    pub fn get_domain(&self, id: DomainId) -> Option<Arc<dyn Domain>> {
        self.domains.read().get(&id).cloned()
    }

    pub fn get_domain_by_name(&self, name: &str) -> Option<Arc<dyn Domain>> {
        self.domains_by_name.read().get(name).cloned()
    }

    pub fn add_domain(&self, domain: &Arc<dyn Domain>) {
        if self.default_domain.read().is_none() {
            *self.default_domain.write() = Some(Arc::clone(domain));
        }
        self.domains
            .write()
            .insert(domain.domain_id(), Arc::clone(domain));
        self.domains_by_name
            .write()
            .insert(domain.domain_name().to_string(), Arc::clone(domain));
    }

    pub fn set_mux(mux: &Arc<Mux>) {
        MUX.lock().replace(Arc::clone(mux));
    }

    pub fn shutdown() {
        MUX.lock().take();
    }

    pub fn get() -> Arc<Mux> {
        Self::try_get().unwrap()
    }

    pub fn try_get() -> Option<Arc<Mux>> {
        MUX.lock().as_ref().map(Arc::clone)
    }

    pub fn get_pane(&self, pane_id: PaneId) -> Option<Arc<dyn Pane>> {
        self.panes.read().get(&pane_id).map(Arc::clone)
    }

    pub fn get_tab(&self, tab_id: TabId) -> Option<Arc<Tab>> {
        self.tabs.read().get(&tab_id).map(Arc::clone)
    }

    pub fn add_pane(&self, pane: &Arc<dyn Pane>) -> Result<(), Error> {
        if self.panes.read().contains_key(&pane.pane_id()) {
            return Ok(());
        }

        let clipboard: Arc<dyn Clipboard> = Arc::new(MuxClipboard {
            pane_id: pane.pane_id(),
        });
        pane.set_clipboard(&clipboard);

        let downloader: Arc<dyn DownloadHandler> = Arc::new(MuxDownloader {});
        pane.set_download_handler(&downloader);

        self.panes.write().insert(pane.pane_id(), Arc::clone(pane));
        let pane_id = pane.pane_id();
        if let Some(reader) = pane.reader()? {
            let banner = self.banner.read().clone();
            let pane = Arc::downgrade(pane);
            thread::spawn(move || read_from_pane_pty(pane, banner, reader));
        }
        self.recompute_pane_count();
        self.notify(MuxNotification::PaneAdded(pane_id));
        Ok(())
    }

    pub fn add_tab_no_panes(&self, tab: &Arc<Tab>) {
        self.tabs.write().insert(tab.tab_id(), Arc::clone(tab));
        self.recompute_pane_count();
    }

    pub fn add_tab_and_active_pane(&self, tab: &Arc<Tab>) -> Result<(), Error> {
        self.tabs.write().insert(tab.tab_id(), Arc::clone(tab));
        let pane = tab
            .get_active_pane()
            .ok_or_else(|| anyhow!("tab MUST have an active pane"))?;
        self.add_pane(&pane)
    }

    fn remove_pane_internal(&self, pane_id: PaneId) {
        log::debug!("removing pane {}", pane_id);
        let mut changed = false;
        if let Some(pane) = self.panes.write().remove(&pane_id).clone() {
            log::debug!("killing pane {}", pane_id);
            pane.kill();
            self.notify(MuxNotification::PaneRemoved(pane_id));
            changed = true;
        }

        if changed {
            self.recompute_pane_count();
        }
    }

    fn remove_tab_internal(&self, tab_id: TabId) -> Option<Arc<Tab>> {
        log::debug!("remove_tab_internal tab {}", tab_id);

        let tab = self.tabs.write().remove(&tab_id)?;

        if let Some(mut windows) = self.windows.try_write() {
            for w in windows.values_mut() {
                w.remove_by_id(tab_id);
            }
        }

        let mut pane_ids = vec![];
        for pos in tab.iter_panes_ignoring_zoom() {
            pane_ids.push(pos.pane.pane_id());
        }
        log::debug!("panes to remove: {pane_ids:?}");
        for pane_id in pane_ids {
            self.remove_pane_internal(pane_id);
        }
        self.recompute_pane_count();

        Some(tab)
    }

    fn remove_window_internal(&self, window_id: WindowId) {
        log::debug!("remove_window_internal {}", window_id);

        let window = self.windows.write().remove(&window_id);
        if let Some(window) = window {
            // Gather all the domains referenced by this window
            let mut domains_of_window = HashSet::new();
            for tab in window.iter() {
                for pane in tab.iter_panes_ignoring_zoom() {
                    domains_of_window.insert(pane.pane.domain_id());
                }
            }

            for domain_id in domains_of_window {
                if let Some(domain) = self.get_domain(domain_id) {
                    if domain.detachable() {
                        log::info!("detaching domain");
                        if let Err(err) = domain.detach() {
                            log::error!(
                                "while detaching domain {domain_id} {}: {err:#}",
                                domain.domain_name()
                            );
                        }
                    }
                }
            }

            for tab in window.iter() {
                self.remove_tab_internal(tab.tab_id());
            }
            self.notify(MuxNotification::WindowRemoved(window_id));
        }
        self.recompute_pane_count();
    }

    pub fn remove_pane(&self, pane_id: PaneId) {
        self.remove_pane_internal(pane_id);
        self.prune_dead_windows();
    }

    pub fn remove_tab(&self, tab_id: TabId) -> Option<Arc<Tab>> {
        let tab = self.remove_tab_internal(tab_id);
        self.prune_dead_windows();
        tab
    }

    pub fn prune_dead_windows(&self) {
        if Activity::count() > 0 {
            log::trace!("prune_dead_windows: Activity::count={}", Activity::count());
            return;
        }
        let live_tab_ids: Vec<TabId> = self.tabs.read().keys().cloned().collect();
        let mut dead_windows = vec![];
        let dead_tab_ids: Vec<TabId>;

        {
            let mut windows = match self.windows.try_write() {
                Some(w) => w,
                None => {
                    // It's ok if our caller already locked it; we can prune later.
                    log::trace!("prune_dead_windows: self.windows already borrowed");
                    return;
                }
            };
            for (window_id, win) in windows.iter_mut() {
                win.prune_dead_tabs(&live_tab_ids);
                if win.is_empty() {
                    log::trace!("prune_dead_windows: window is now empty");
                    dead_windows.push(*window_id);
                }
            }

            dead_tab_ids = self
                .tabs
                .read()
                .iter()
                .filter_map(|(&id, tab)| if tab.is_dead() { Some(id) } else { None })
                .collect();
        }

        for tab_id in dead_tab_ids {
            log::trace!("tab {} is dead", tab_id);
            self.remove_tab_internal(tab_id);
        }

        for window_id in dead_windows {
            log::trace!("window {} is dead", window_id);
            self.remove_window_internal(window_id);
        }

        if self.is_empty() {
            log::trace!("prune_dead_windows: is_empty, send MuxNotification::Empty");
            self.notify(MuxNotification::Empty);
        } else {
            log::trace!("prune_dead_windows: not empty");
        }
    }

    pub fn kill_window(&self, window_id: WindowId) {
        self.remove_window_internal(window_id);
        self.prune_dead_windows();
    }

    pub fn get_window(&self, window_id: WindowId) -> Option<MappedRwLockReadGuard<'_, Window>> {
        if !self.windows.read().contains_key(&window_id) {
            return None;
        }
        Some(RwLockReadGuard::map(self.windows.read(), |windows| {
            windows.get(&window_id).unwrap()
        }))
    }

    pub fn get_window_mut(
        &self,
        window_id: WindowId,
    ) -> Option<MappedRwLockWriteGuard<'_, Window>> {
        if !self.windows.read().contains_key(&window_id) {
            return None;
        }
        Some(RwLockWriteGuard::map(self.windows.write(), |windows| {
            windows.get_mut(&window_id).unwrap()
        }))
    }

    pub fn get_active_tab_for_window(&self, window_id: WindowId) -> Option<Arc<Tab>> {
        let window = self.get_window(window_id)?;
        window.get_active().map(Arc::clone)
    }

    pub fn new_empty_window(
        &self,
        workspace: Option<String>,
        position: Option<GuiPosition>,
    ) -> MuxWindowBuilder {
        let window = Window::new(workspace, position);
        let window_id = window.window_id();
        self.windows.write().insert(window_id, window);
        MuxWindowBuilder {
            window_id,
            activity: Some(Activity::new()),
            notified: false,
        }
    }

    pub fn add_tab_to_window(&self, tab: &Arc<Tab>, window_id: WindowId) -> anyhow::Result<()> {
        let tab_id = tab.tab_id();
        {
            let mut window = self
                .get_window_mut(window_id)
                .ok_or_else(|| anyhow!("add_tab_to_window: no such window_id {}", window_id))?;
            window.push(tab);
        }
        self.recompute_pane_count();
        self.notify(MuxNotification::TabAddedToWindow { tab_id, window_id });
        Ok(())
    }

    /// Returns the ID of the window containing the given tab ID, if any.
    pub fn window_containing_tab(&self, tab_id: TabId) -> Option<WindowId> {
        for w in self.windows.read().values() {
            for t in w.iter() {
                if t.tab_id() == tab_id {
                    return Some(w.window_id());
                }
            }
        }
        None
    }

    pub fn is_empty(&self) -> bool {
        self.panes.read().is_empty()
    }

    pub fn is_workspace_empty(&self, workspace: &str) -> bool {
        *self
            .num_panes_by_workspace
            .read()
            .get(workspace)
            .unwrap_or(&0)
            == 0
    }

    pub fn is_active_workspace_empty(&self) -> bool {
        let workspace = self.active_workspace();
        self.is_workspace_empty(&workspace)
    }

    pub fn iter_panes(&self) -> Vec<Arc<dyn Pane>> {
        self.panes.read().values().map(Arc::clone).collect()
    }

    pub fn iter_windows_in_workspace(&self, workspace: &str) -> Vec<WindowId> {
        let mut windows: Vec<WindowId> = self
            .windows
            .read()
            .iter()
            .filter_map(|(k, w)| {
                if w.get_workspace() == workspace {
                    Some(k)
                } else {
                    None
                }
            })
            .cloned()
            .collect();
        windows.sort();
        windows
    }

    pub fn iter_windows(&self) -> Vec<WindowId> {
        self.windows.read().keys().cloned().collect()
    }

    pub fn iter_domains(&self) -> Vec<Arc<dyn Domain>> {
        self.domains.read().values().cloned().collect()
    }

    pub fn resolve_pane_id(&self, pane_id: PaneId) -> Option<(DomainId, WindowId, TabId)> {
        let mut ids = None;
        for tab in self.tabs.read().values() {
            for p in tab.iter_panes_ignoring_zoom() {
                if p.pane.pane_id() == pane_id {
                    ids = Some((tab.tab_id(), p.pane.domain_id()));
                    break;
                }
            }
        }
        let (tab_id, domain_id) = ids?;
        let window_id = self.window_containing_tab(tab_id)?;
        Some((domain_id, window_id, tab_id))
    }

    pub fn domain_was_detached(&self, domain: DomainId) {
        let mut dead_panes = vec![];
        for pane in self.panes.read().values() {
            if pane.domain_id() == domain {
                dead_panes.push(pane.pane_id());
            }
        }

        {
            let mut windows = self.windows.write();
            for win in windows.values_mut() {
                for tab in win.iter() {
                    tab.kill_panes_in_domain(domain);
                }
            }
        }

        log::info!("domain detached panes: {:?}", dead_panes);
        for pane_id in dead_panes {
            self.remove_pane_internal(pane_id);
        }

        self.prune_dead_windows();
    }

    /// Test-only probe: returns `true` if `self.windows` is currently free
    /// to acquire for write (i.e. not already held on this thread), `false`
    /// otherwise. Used by regression tests to prove that no code path
    /// synchronously calls back into pane teardown (e.g. `resize`/`kill`)
    /// while still holding `windows` for write, which would deadlock a
    /// non-reentrant lock. See `domain_was_detached`.
    #[cfg(test)]
    pub(crate) fn probe_windows_try_write(&self) -> bool {
        self.windows.try_write().is_some()
    }

    /// Test-only helper to register a `Window` built outside of
    /// `new_empty_window` (which requires a live `Activity`/config
    /// machinery not needed by unit tests).
    #[cfg(test)]
    pub(crate) fn insert_window_for_test(&self, window_id: WindowId, window: Window) {
        self.windows.write().insert(window_id, window);
        self.recompute_pane_count();
    }

    /// Test-only probe: returns `true` if `self.subscribers` is currently
    /// free to acquire for write. Used to prove that `Mux::notify` does
    /// not hold the subscribers lock while invoking callbacks -- a
    /// subscriber callback that (transitively) calls back into
    /// `Mux::subscribe`/`Mux::notify` would deadlock a non-reentrant lock
    /// otherwise.
    #[cfg(test)]
    pub(crate) fn probe_subscribers_try_write(&self) -> bool {
        self.subscribers.try_write().is_some()
    }

    /// Test-only accessor for the `PaneOutput` coalescing marker set, so
    /// regression tests can assert on pending/cleared state directly
    /// rather than only inferring it from callback invocation counts.
    #[cfg(test)]
    pub(crate) fn pane_output_notify_is_pending(&self, pane_id: PaneId) -> bool {
        self.pane_output_notify_state.lock().contains_key(&pane_id)
    }

    pub fn set_banner(&self, banner: Option<String>) {
        *self.banner.write() = banner;
    }

    pub fn resolve_spawn_tab_domain(
        &self,
        // TODO: disambiguate with TabId
        pane_id: Option<PaneId>,
        domain: &config::keyassignment::SpawnTabDomain,
    ) -> anyhow::Result<Arc<dyn Domain>> {
        let domain = match domain {
            SpawnTabDomain::DefaultDomain => self.default_domain(),
            SpawnTabDomain::CurrentPaneDomain => match pane_id {
                Some(pane_id) => {
                    let (pane_domain_id, _window_id, _tab_id) = self
                        .resolve_pane_id(pane_id)
                        .ok_or_else(|| anyhow!("pane_id {} invalid", pane_id))?;
                    self.get_domain(pane_domain_id)
                        .expect("resolve_pane_id to give valid domain_id")
                }
                None => self.default_domain(),
            },
            SpawnTabDomain::DomainId(domain_id) => self
                .get_domain(*domain_id)
                .ok_or_else(|| anyhow!("domain id {} is invalid", domain_id))?,
            SpawnTabDomain::DomainName(name) => self.get_domain_by_name(name).ok_or_else(|| {
                let names: Vec<String> = self
                    .domains_by_name
                    .read()
                    .keys()
                    .map(|name| format!("\"{name}\""))
                    .collect();
                anyhow!(
                    "domain name \"{name}\" is invalid. Possible names are {}.",
                    names.join(", ")
                )
            })?,
        };
        Ok(domain)
    }

    fn resolve_cwd(
        &self,
        command_dir: Option<String>,
        pane: Option<Arc<dyn Pane>>,
        target_domain: DomainId,
        policy: CachePolicy,
    ) -> Option<String> {
        command_dir.or_else(|| {
            match pane {
                Some(pane) if pane.domain_id() == target_domain => pane
                    .get_current_working_dir(policy)
                    .and_then(|url| {
                        percent_decode_str(url.path())
                            .decode_utf8()
                            .ok()
                            .map(|path| path.into_owned())
                    })
                    .map(|path| {
                        // On Windows the file URI can produce a path like:
                        // `/C:\Users` which is valid in a file URI, but the leading slash
                        // is not liked by the windows file APIs, so we strip it off here.
                        let bytes = path.as_bytes();
                        if bytes.len() > 2 && bytes[0] == b'/' && bytes[2] == b':' {
                            path[1..].to_owned()
                        } else {
                            path
                        }
                    }),
                _ => None,
            }
        })
    }

    pub async fn split_pane(
        &self,
        // TODO: disambiguate with TabId
        pane_id: PaneId,
        request: SplitRequest,
        source: SplitSource,
        domain: config::keyassignment::SpawnTabDomain,
    ) -> anyhow::Result<(Arc<dyn Pane>, TerminalSize)> {
        let (_pane_domain_id, window_id, tab_id) = self
            .resolve_pane_id(pane_id)
            .ok_or_else(|| anyhow!("pane_id {} invalid", pane_id))?;

        let domain = self
            .resolve_spawn_tab_domain(Some(pane_id), &domain)
            .context("resolve_spawn_tab_domain")?;

        if domain.state() == DomainState::Detached {
            domain.attach(Some(window_id)).await?;
        }

        let current_pane = self
            .get_pane(pane_id)
            .ok_or_else(|| anyhow!("pane_id {} is invalid", pane_id))?;
        let term_config = current_pane.get_config();

        let source = match source {
            SplitSource::Spawn {
                command,
                command_dir,
            } => SplitSource::Spawn {
                command,
                command_dir: self.resolve_cwd(
                    command_dir,
                    Some(Arc::clone(&current_pane)),
                    domain.domain_id(),
                    CachePolicy::FetchImmediate,
                ),
            },
            other => other,
        };

        let pane = domain.split_pane(source, tab_id, pane_id, request).await?;
        if let Some(config) = term_config {
            pane.set_config(config);
        }

        // FIXME: clipboard

        let dims = pane.get_dimensions();

        let size = TerminalSize {
            cols: dims.cols,
            rows: dims.viewport_rows,
            pixel_height: 0, // FIXME: split pane pixel dimensions
            pixel_width: 0,
            dpi: dims.dpi,
        };

        Ok((pane, size))
    }

    pub async fn move_pane_to_new_tab(
        &self,
        pane_id: PaneId,
        window_id: Option<WindowId>,
        workspace_for_new_window: Option<String>,
    ) -> anyhow::Result<(Arc<Tab>, WindowId)> {
        let (domain_id, _src_window, src_tab) = self
            .resolve_pane_id(pane_id)
            .ok_or_else(|| anyhow::anyhow!("pane {} not found", pane_id))?;

        let domain = self
            .get_domain(domain_id)
            .ok_or_else(|| anyhow::anyhow!("domain {domain_id} of pane {pane_id} not found"))?;

        if let Some((tab, window_id)) = domain
            .remote_move_pane_to_new_tab(pane_id, window_id, workspace_for_new_window.clone())
            .await?
        {
            return Ok((tab, window_id));
        }

        let src_tab = match self.get_tab(src_tab) {
            Some(t) => t,
            None => anyhow::bail!("Invalid tab id {}", src_tab),
        };

        let window_builder;
        let (window_id, size) = if let Some(window_id) = window_id {
            let window = self
                .get_window_mut(window_id)
                .ok_or_else(|| anyhow!("window_id {} not found on this server", window_id))?;
            let tab = window
                .get_active()
                .ok_or_else(|| anyhow!("window {} has no tabs", window_id))?;
            let size = tab.get_size();

            (window_id, size)
        } else {
            window_builder = self.new_empty_window(workspace_for_new_window, None);
            (*window_builder, src_tab.get_size())
        };

        let pane = src_tab
            .remove_pane(pane_id)
            .ok_or_else(|| anyhow::anyhow!("pane {} wasn't in its containing tab!?", pane_id))?;

        let tab = Arc::new(Tab::new(&size));
        tab.assign_pane(&pane);
        pane.resize(size)?;
        self.add_tab_and_active_pane(&tab)?;
        self.add_tab_to_window(&tab, window_id)?;

        if src_tab.is_dead() {
            self.remove_tab(src_tab.tab_id());
        }

        Ok((tab, window_id))
    }

    pub async fn rotate_panes(
        &self,
        tab_id: TabId,
        direction: RotationDirection,
    ) -> anyhow::Result<()> {
        let tab = match self.get_tab(tab_id) {
            Some(tab) => tab,
            None => anyhow::bail!("Invalid tab id {}", tab_id),
        };

        // This makes the assumption that a tab contains only panes from a single local domain,
        // though that is also an assumption that ClientDomain makes when syncing tab panes.
        let tab_panes = tab.iter_panes();
        let pos_pane = match tab_panes.first() {
            Some(pos_pane) => pos_pane,
            None => anyhow::bail!("Tab contains no panes: {}", tab_id),
        };

        let pane_id = pos_pane.pane.pane_id();
        let domain_id = pos_pane.pane.domain_id();

        let domain = self
            .get_domain(domain_id)
            .ok_or_else(|| anyhow::anyhow!("domain {domain_id} of tab {tab_id} not found"))?;

        if domain.remote_rotate_panes(pane_id, direction).await? {
            return Ok(());
        }

        match direction {
            RotationDirection::Clockwise => tab.local_rotate_clockwise(),
            RotationDirection::CounterClockwise => tab.local_rotate_counter_clockwise(),
        }
        Ok(())
    }

    pub async fn swap_active_pane_with_index(
        &self,
        active_pane_id: PaneId,
        with_pane_index: usize,
        keep_focus: bool,
    ) -> anyhow::Result<()> {
        let (domain_id, _window_id, tab_id) = self
            .resolve_pane_id(active_pane_id)
            .ok_or_else(|| anyhow::anyhow!("pane {} not found", active_pane_id))?;

        let domain = self.get_domain(domain_id).ok_or_else(|| {
            anyhow::anyhow!("domain {domain_id} of pane {active_pane_id} not found")
        })?;

        if domain
            .remote_swap_active_pane_with_index(active_pane_id, with_pane_index, keep_focus)
            .await?
        {
            return Ok(());
        }

        let tab = match self.get_tab(tab_id) {
            Some(tab) => tab,
            None => anyhow::bail!("Invalid tab id {}", tab_id),
        };

        tab.local_swap_active_with_index(with_pane_index, keep_focus);
        Ok(())
    }
    #[allow(clippy::too_many_arguments)] // public async API; reordering/merging params would break callers across the workspace
    pub async fn spawn_tab_or_window(
        &self,
        window_id: Option<WindowId>,
        domain: SpawnTabDomain,
        command: Option<CommandBuilder>,
        command_dir: Option<String>,
        size: TerminalSize,
        current_pane_id: Option<PaneId>,
        workspace_for_new_window: String,
        window_position: Option<GuiPosition>,
    ) -> anyhow::Result<(Arc<Tab>, Arc<dyn Pane>, WindowId)> {
        let domain = self
            .resolve_spawn_tab_domain(current_pane_id, &domain)
            .context("resolve_spawn_tab_domain")?;

        let window_builder;
        let term_config;

        let (window_id, size) = if let Some(window_id) = window_id {
            let window = self
                .get_window_mut(window_id)
                .ok_or_else(|| anyhow!("window_id {} not found on this server", window_id))?;
            let tab = window
                .get_active()
                .ok_or_else(|| anyhow!("window {} has no tabs", window_id))?;
            let pane = tab
                .get_active_pane()
                .ok_or_else(|| anyhow!("active tab in window {} has no panes", window_id))?;
            term_config = pane.get_config();

            let size = tab.get_size();

            (window_id, size)
        } else {
            term_config = None;
            window_builder = self.new_empty_window(Some(workspace_for_new_window), window_position);
            (*window_builder, size)
        };

        if domain.state() == DomainState::Detached {
            domain.attach(Some(window_id)).await?;
        }

        let cwd = self.resolve_cwd(
            command_dir,
            match current_pane_id {
                Some(id) => {
                    // Only use the cwd from the current pane if the domain
                    // is the same as the one we are spawning into
                    let (current_domain_id, _, _) = self
                        .resolve_pane_id(id)
                        .ok_or_else(|| anyhow!("pane_id {} invalid", id))?;
                    if current_domain_id == domain.domain_id() {
                        self.get_pane(id)
                    } else {
                        None
                    }
                }
                None => None,
            },
            domain.domain_id(),
            CachePolicy::FetchImmediate,
        );

        let tab = domain
            .spawn(size, command.clone(), cwd.clone(), window_id)
            .await
            .with_context(|| {
                format!(
                    "Spawning in domain `{}`: {size:?} command={command:?} cwd={cwd:?}",
                    domain.domain_name()
                )
            })?;

        let pane = tab
            .get_active_pane()
            .ok_or_else(|| anyhow!("missing active pane on tab!?"))?;

        if let Some(config) = term_config {
            pane.set_config(config);
        }

        // FIXME: clipboard?

        let mut window = self
            .get_window_mut(window_id)
            .ok_or_else(|| anyhow!("no such window!?"))?;
        if let Some(idx) = window.idx_by_id(tab.tab_id()) {
            window.save_and_then_set_active(idx);
        }

        Ok((tab, pane, window_id))
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

impl wezterm_term::DownloadHandler for MuxDownloader {
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
