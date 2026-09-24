use crate::client::Client;
use crate::pane::ClientPane;
use anyhow::{anyhow, bail};
use onlyterm_codec::ListPanesResponse;
use onlyterm_mux::connui::{ConnectionUI, ConnectionUIParams};
use onlyterm_mux::domain::{alloc_domain_id, Domain, DomainId, DomainState};
use onlyterm_mux::pane::Pane;
use onlyterm_mux::tab::{Tab, TabId};
use onlyterm_mux::window::WindowId;
use onlyterm_mux::{Mux, MuxNotification};
use onlyterm_promise::spawn::spawn_into_new_thread;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

mod config;
mod inner;
mod mux_domain;

pub use config::ClientDomainConfig;
pub use inner::ClientInner;

pub struct ClientDomain {
    config: ClientDomainConfig,
    label: String,
    inner: Mutex<Option<Arc<ClientInner>>>,
    local_domain_id: DomainId,
    /// Dedicated per-tab hosts cannot create another tab, but may split their
    /// existing tab into multiple panes.
    spawnable: bool,
    elevated_host: bool,
}

pub struct PreparedAttach {
    domain_id: DomainId,
    client: Client,
    panes: ListPanesResponse,
}

async fn update_remote_workspace(
    local_domain_id: DomainId,
    pdu: onlyterm_codec::SetWindowWorkspace,
) -> anyhow::Result<()> {
    let inner = ClientDomain::get_client_inner_for_domain(local_domain_id)?;
    inner.client.set_window_workspace(pdu).await?;
    Ok(())
}

fn mux_notify_client_domain(local_domain_id: DomainId, notif: MuxNotification) -> bool {
    let mux = Mux::get();
    let domain = match mux.get_domain(local_domain_id) {
        Some(domain) => domain,
        None => return false,
    };
    let client_domain = match domain.downcast_ref::<ClientDomain>() {
        Some(c) => c,
        None => return false,
    };

    match notif {
        MuxNotification::ActiveWorkspaceChanged(_client_id) => {
            // TODO: advice remote host of interesting workspaces
        }
        MuxNotification::WorkspaceRenamed {
            old_workspace,
            new_workspace,
        } => {
            if let Some(inner) = client_domain.inner() {
                let workspaces = Mux::get().iter_workspaces();
                if workspaces.contains(&old_workspace) {
                    onlyterm_promise::spawn::spawn(async move {
                        inner
                            .client
                            .rename_workspace(onlyterm_codec::RenameWorkspace {
                                old_workspace,
                                new_workspace,
                            })
                            .await
                    })
                    .detach();
                }
            }
        }
        MuxNotification::WindowWorkspaceChanged(window_id) => {
            // Mux::get_window() may trigger a borrow error if called
            // immediately; defer the bulk of this work.
            // <https://github.com/wezterm/wezterm/issues/2638>
            onlyterm_promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::get();
                let domain = match mux.get_domain(local_domain_id) {
                    Some(domain) => domain,
                    None => return,
                };
                let domain = match domain.downcast_ref::<ClientDomain>() {
                    Some(domain) => domain,
                    None => return,
                };
                if let Some(remote_window_id) = domain.local_to_remote_window_id(window_id) {
                    if let Some(workspace) = mux
                        .get_window(window_id)
                        .map(|w| w.get_workspace().to_string())
                    {
                        onlyterm_promise::spawn::spawn_into_main_thread(async move {
                            let request = onlyterm_codec::SetWindowWorkspace {
                                window_id: remote_window_id,
                                workspace,
                            };
                            let _ = update_remote_workspace(local_domain_id, request).await;
                        })
                        .detach();
                    }
                } else {
                    log::debug!(
                        "local window id {window_id} has no known remote window \
                        id while reconciling a local WindowWorkspaceChanged event"
                    );
                }
            })
            .detach();
        }
        MuxNotification::TabTitleChanged { tab_id, title } => {
            if let Some(remote_tab_id) = client_domain.local_to_remote_tab_id(tab_id) {
                if let Some(inner) = client_domain.inner() {
                    onlyterm_promise::spawn::spawn(async move {
                        inner
                            .client
                            .set_tab_title(onlyterm_codec::TabTitleChanged {
                                tab_id: remote_tab_id,
                                title,
                            })
                            .await
                    })
                    .detach();
                }
            }
        }
        MuxNotification::WindowTitleChanged {
            window_id,
            title: _,
        } => {
            if let Some(remote_window_id) = client_domain.local_to_remote_window_id(window_id) {
                if let Some(inner) = client_domain.inner() {
                    onlyterm_promise::spawn::spawn_into_main_thread(async move {
                        // De-bounce the title propagation.
                        // There is a bit of a race condition with these async
                        // updates that can trigger a cycle of WindowTitleChanged
                        // PDUs being exchanged between client and server if the
                        // title is changed twice in quick succession.
                        // To avoid that, here on the client, we wait a second
                        // and then report the now-current name of the window, rather
                        // than propagating the title encoded in the MuxNotification.
                        smol::Timer::after(std::time::Duration::from_secs(1)).await;
                        if let Some(mux) = Mux::try_get() {
                            let title = mux
                                .get_window(window_id)
                                .map(|win| win.get_title().to_string());
                            if let Some(title) = title {
                                inner
                                    .client
                                    .set_window_title(onlyterm_codec::WindowTitleChanged {
                                        window_id: remote_window_id,
                                        title,
                                    })
                                    .await?;
                            }
                        }
                        anyhow::Result::<()>::Ok(())
                    })
                    .detach();
                }
            }
        }
        _ => {}
    }
    true
}

impl ClientDomain {
    /// Perform the expensive child-process/connection/version/pane-list work
    /// without materializing local tabs. Startup layouts use this to overlap
    /// bounded isolated launches, then commit each prepared connection in
    /// configured order.
    pub async fn prepare_attach(&self) -> anyhow::Result<PreparedAttach> {
        if self.state() == DomainState::Attached {
            anyhow::bail!("domain is already attached");
        }
        let domain_id = self.local_domain_id;
        let config = self.config.clone();
        let ui = ConnectionUI::with_params(Default::default());
        let result = ui
            .async_run_and_log_error({
                let ui = ui.clone();
                async move {
                    let mut cloned_ui = ui.clone();
                    let client = spawn_into_new_thread(move || match &config {
                        ClientDomainConfig::Unix(unix) => Client::new_unix_domain(
                            Some(domain_id),
                            unix,
                            true,
                            &mut cloned_ui,
                            false,
                        ),
                    })
                    .await?;
                    client.verify_version_compat(&ui).await?;
                    let panes = client.list_panes().await?;
                    Ok((client, panes))
                }
            })
            .await;
        ui.close();
        let (client, panes) = result?;
        Ok(PreparedAttach {
            domain_id,
            client,
            panes,
        })
    }

    pub fn commit_prepared_attach(
        &self,
        prepared: PreparedAttach,
        window_id: Option<WindowId>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            prepared.domain_id == self.local_domain_id,
            "prepared connection belongs to another domain"
        );
        Self::finish_attach(
            prepared.domain_id,
            prepared.client,
            prepared.panes,
            window_id,
        )
    }

    pub fn new(config: ClientDomainConfig) -> Self {
        Self::new_impl(config, true, false)
    }

    /// A regular dedicated host process that cannot create another tab.
    pub fn new_single_pane(config: ClientDomainConfig) -> Self {
        Self::new_impl(config, false, false)
    }

    /// An elevated dedicated host process with restricted split requests.
    pub fn new_elevated_single_pane(config: ClientDomainConfig) -> Self {
        Self::new_impl(config, false, true)
    }

    fn new_impl(config: ClientDomainConfig, spawnable: bool, elevated_host: bool) -> Self {
        let local_domain_id = alloc_domain_id();
        let label = config.label();
        Mux::get().subscribe(move |notif| mux_notify_client_domain(local_domain_id, notif));
        Self {
            config,
            label,
            inner: Mutex::new(None),
            local_domain_id,
            spawnable,
            elevated_host,
        }
    }

    fn inner(&self) -> Option<Arc<ClientInner>> {
        self.inner.lock().unwrap().as_ref().map(Arc::clone)
    }

    pub fn connect_automatically(&self) -> bool {
        self.config.connect_automatically()
    }

    pub fn perform_detach(&self) {
        log::info!("detached domain {}", self.local_domain_id);
        self.inner.lock().unwrap().take();
        let mux = Mux::get();

        // Remove the domain from the mux's registration maps. This must be
        // done BEFORE calling domain_was_detached, because that function
        // may drop references to the domain and we want the mux state to
        // be clean for the rest of the teardown.
        if let Some(domain) = mux.get_domain(self.local_domain_id) {
            mux.remove_domain(&domain);
        }

        mux.domain_was_detached(self.local_domain_id);
    }

    pub fn remote_to_local_pane_id(&self, remote_pane_id: TabId) -> Option<TabId> {
        let inner = self.inner()?;
        inner.remote_to_local_pane_id(remote_pane_id)
    }

    pub fn remote_to_local_window_id(&self, remote_window_id: WindowId) -> Option<WindowId> {
        let inner = self.inner()?;
        inner.remote_to_local_window(remote_window_id)
    }

    pub fn local_to_remote_window_id(&self, local_window_id: WindowId) -> Option<WindowId> {
        let inner = self.inner()?;
        inner.local_to_remote_window(local_window_id)
    }

    pub fn local_to_remote_tab_id(&self, local_tab_id: TabId) -> Option<TabId> {
        let inner = self.inner()?;
        inner.local_to_remote_tab(local_tab_id)
    }

    pub fn get_client_inner_for_domain(domain_id: DomainId) -> anyhow::Result<Arc<ClientInner>> {
        let mux = Mux::get();
        let domain = mux
            .get_domain(domain_id)
            .ok_or_else(|| anyhow!("invalid domain id {}", domain_id))?;
        let domain = domain
            .downcast_ref::<Self>()
            .ok_or_else(|| anyhow!("domain {} is not a ClientDomain", domain_id))?;

        if let Some(inner) = domain.inner() {
            Ok(inner)
        } else {
            bail!("domain has no assigned client");
        }
    }

    /// The reader in the mux may have decided to give up on one or
    /// more tabs at the time that a disconnect was detected, and
    /// it's also possible that another client connected and adjusted
    /// the set of tabs since we were connected, so we need to re-sync.
    pub async fn reattach(domain_id: DomainId, ui: ConnectionUI) -> anyhow::Result<()> {
        let inner = Self::get_client_inner_for_domain(domain_id)?;

        let panes = inner.client.list_panes().await?;
        Self::process_pane_list(inner, panes, None)?;

        ui.close();
        Ok(())
    }

    pub async fn resync(&self) -> anyhow::Result<()> {
        if let Some(inner) = self.inner() {
            let panes = inner.client.list_panes().await?;
            Self::process_pane_list(inner, panes, None)?;
        }
        Ok(())
    }

    pub fn process_remote_window_title_change(&self, remote_window_id: WindowId, title: String) {
        if let Some(inner) = self.inner() {
            if let Some(local_window_id) = inner.remote_to_local_window(remote_window_id) {
                if let Some(mut window) = Mux::get().get_window_mut(local_window_id) {
                    window.set_title(&title);
                }
            }
        }
    }

    pub fn process_remote_tab_title_change(&self, remote_tab_id: TabId, title: String) {
        if let Some(inner) = self.inner() {
            if let Some(local_tab_id) = inner.remote_to_local_tab_id(remote_tab_id) {
                if let Some(tab) = Mux::get().get_tab(local_tab_id) {
                    tab.set_title(&title);
                }
            }
        }
    }

    fn process_pane_list(
        inner: Arc<ClientInner>,
        panes: ListPanesResponse,
        mut primary_window_id: Option<WindowId>,
    ) -> anyhow::Result<()> {
        let mux = Mux::get();
        log::debug!(
            "domain {}: ListPanes result {:#?}",
            inner.local_domain_id,
            panes
        );

        // "Mark" the current set of known remote ids, so that we can "Sweep"
        // any unreferenced ids at the bottom, garbage collection style
        let mut remote_windows_to_forget: HashSet<WindowId> = inner
            .remote_to_local_window
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect();
        let mut remote_tabs_to_forget: HashSet<WindowId> = inner
            .remote_to_local_tab
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect();
        let mut remote_panes_to_forget: HashSet<WindowId> = inner
            .remote_to_local_pane
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect();

        for (tabroot, tab_title) in panes.tabs.into_iter().zip(panes.tab_titles.iter()) {
            let root_size = match tabroot.root_size() {
                Some(size) => size,
                None => continue,
            };

            if let Some((remote_window_id, remote_tab_id)) = tabroot.window_and_tab_ids() {
                let tab;

                remote_windows_to_forget.remove(&remote_window_id);
                remote_tabs_to_forget.remove(&remote_tab_id);

                if let Some(tab_id) = inner.remote_to_local_tab_id(remote_tab_id) {
                    match mux.get_tab(tab_id) {
                        Some(t) => tab = t,
                        None => {
                            // We likely decided that we hit EOF on the tab and
                            // removed it from the mux.  Let's add it back, but
                            // with a new id.
                            log::trace!(
                                "we had remote_to_local_tab_id mapping of \
                                 {remote_tab_id} -> {tab_id}, but the local \
                                 tab is not in the mux, make a new tab"
                            );
                            inner.remove_old_tab_mapping(remote_tab_id);
                            tab = Arc::new(Tab::new(&root_size));
                            inner.record_remote_to_local_tab_mapping(remote_tab_id, tab.tab_id());
                            mux.add_tab_no_panes(&tab);
                        }
                    };
                } else {
                    tab = Arc::new(Tab::new(&root_size));
                    mux.add_tab_no_panes(&tab);
                    inner.record_remote_to_local_tab_mapping(remote_tab_id, tab.tab_id());
                }

                tab.set_title(tab_title);

                log::debug!("domain: {} tree: {:#?}", inner.local_domain_id, tabroot);
                let mut workspace = None;
                tab.sync_with_pane_tree(root_size, tabroot, |entry| {
                    workspace.replace(entry.workspace.clone());
                    remote_panes_to_forget.remove(&entry.pane_id);
                    if let Some(pane_id) = inner.remote_to_local_pane_id(entry.pane_id) {
                        match mux.get_pane(pane_id) {
                            Some(pane) => pane,
                            None => {
                                // We likely decided that we hit EOF on the tab and
                                // removed it from the mux.  Let's add it back, but
                                // with a new id.
                                inner.remove_old_pane_mapping(entry.pane_id);
                                let pane: Arc<dyn Pane> = Arc::new(ClientPane::new(
                                    &inner,
                                    entry.tab_id,
                                    entry.pane_id,
                                    entry.size,
                                    &entry.title,
                                ));
                                mux.add_pane(&pane).expect("failed to add pane to mux");
                                pane
                            }
                        }
                    } else {
                        let pane: Arc<dyn Pane> = Arc::new(ClientPane::new(
                            &inner,
                            entry.tab_id,
                            entry.pane_id,
                            entry.size,
                            &entry.title,
                        ));
                        log::debug!(
                            "domain: {} attaching to remote pane {:?} -> local pane_id {}",
                            inner.local_domain_id,
                            entry,
                            pane.pane_id()
                        );
                        mux.add_pane(&pane).expect("failed to add pane to mux");
                        pane
                    }
                });

                if let Some(local_window_id) = inner.remote_to_local_window(remote_window_id) {
                    let mut window = mux
                        .get_window_mut(local_window_id)
                        .expect("no such window!?");
                    log::debug!(
                        "domain: {} adding tab to existing local window {}",
                        inner.local_domain_id,
                        local_window_id
                    );
                    if window.idx_by_id(tab.tab_id()).is_none() {
                        window.push(&tab);
                    }
                    continue;
                }

                if let Some(local_window_id) = primary_window_id {
                    // Verify that the workspace is consistent between the local and remote
                    // windows
                    if Some(
                        mux.get_window(local_window_id)
                            .expect("primary window to be valid")
                            .get_workspace(),
                    ) == workspace.as_deref()
                    {
                        // Yes! We can use this window
                        log::debug!(
                            "adding remote window {} as tab to local window {}",
                            remote_window_id,
                            local_window_id
                        );
                        inner.record_remote_to_local_window_mapping(
                            remote_window_id,
                            local_window_id,
                        );
                        mux.add_tab_to_window(&tab, local_window_id)?;
                        primary_window_id.take();
                        continue;
                    }
                }
                log::debug!(
                    "making new local window for remote {} in workspace {:?}",
                    remote_window_id,
                    workspace
                );
                let position = None;
                let local_window_id = mux.new_empty_window(workspace.take(), position);
                inner.record_remote_to_local_window_mapping(remote_window_id, *local_window_id);
                mux.add_tab_to_window(&tab, *local_window_id)?;
            }
        }

        for (remote_window_id, window_title) in panes.window_titles {
            if let Some(local_window_id) = inner.remote_to_local_window(remote_window_id) {
                let mut window = mux
                    .get_window_mut(local_window_id)
                    .expect("no such window!?");
                window.set_title(&window_title);
            }
        }

        // "Sweep" away our mapping for ids that are no longer present in the
        // latest sync
        log::debug!(
            "after sync, remote_windows_to_forget={remote_windows_to_forget:?}, \
                    remote_tabs_to_forget={remote_tabs_to_forget:?}, \
                    remote_panes_to_forget={remote_panes_to_forget:?}"
        );
        if !remote_windows_to_forget.is_empty() {
            let mut windows = inner.remote_to_local_window.lock().unwrap();
            for w in remote_windows_to_forget {
                windows.remove(&w);
            }
        }
        if !remote_tabs_to_forget.is_empty() {
            let mut tabs = inner.remote_to_local_tab.lock().unwrap();
            for t in remote_tabs_to_forget {
                tabs.remove(&t);
            }
        }
        if !remote_panes_to_forget.is_empty() {
            let mut panes = inner.remote_to_local_pane.lock().unwrap();
            for p in remote_panes_to_forget {
                panes.remove(&p);
            }
        }

        Ok(())
    }

    fn finish_attach(
        domain_id: DomainId,
        client: Client,
        panes: ListPanesResponse,
        primary_window_id: Option<WindowId>,
    ) -> anyhow::Result<()> {
        let mux = Mux::get();
        let domain = mux
            .get_domain(domain_id)
            .ok_or_else(|| anyhow!("invalid domain id {}", domain_id))?;
        let domain = domain
            .downcast_ref::<Self>()
            .ok_or_else(|| anyhow!("domain {} is not a ClientDomain", domain_id))?;
        let threshold = domain.config.local_echo_threshold_ms();
        let overlay_lag_indicator = domain.config.overlay_lag_indicator();

        let inner = Arc::new(ClientInner::new(
            domain_id,
            client,
            threshold,
            overlay_lag_indicator,
        ));
        *domain.inner.lock().unwrap() = Some(Arc::clone(&inner));

        Self::process_pane_list(inner, panes, primary_window_id)?;

        Ok(())
    }

    async fn attach_impl(
        &self,
        window_id: Option<WindowId>,
        ui: ConnectionUI,
        verbose: bool,
    ) -> anyhow::Result<()> {
        if self.state() == DomainState::Attached {
            // Already attached
            return Ok(());
        }

        let domain_id = self.local_domain_id;
        let config = self.config.clone();

        let activity = onlyterm_mux::activity::Activity::new();

        ui.async_run_and_log_error({
            let ui = ui.clone();
            async move {
                let mut cloned_ui = ui.clone();
                let client = spawn_into_new_thread(move || match &config {
                    ClientDomainConfig::Unix(unix) => {
                        let initial = true;
                        let no_auto_start = false;
                        Client::new_unix_domain(
                            Some(domain_id),
                            unix,
                            initial,
                            &mut cloned_ui,
                            no_auto_start,
                        )
                    }
                })
                .await?;

                if verbose {
                    ui.output_str("Checking server version\n");
                }
                client.verify_version_compat(&ui).await?;

                if verbose {
                    ui.output_str("Version check OK!  Requesting pane list...\n");
                }
                let panes = client.list_panes().await?;
                if verbose {
                    ui.output_str(&format!(
                        "Server has {} tabs.  Attaching to local UI...\n",
                        panes.tabs.len()
                    ));
                }
                ClientDomain::finish_attach(domain_id, client, panes, window_id)
            }
        })
        .await
        .map_err(|e| {
            // Always report errors, even in non-verbose mode -- silence
            // should only ever hide routine progress noise, never a real
            // failure.
            ui.output_str(&format!("Error during attach: {:#}\n", e));
            e
        })?;

        if verbose {
            ui.output_str("Attached!\n");
        }
        drop(activity);
        ui.close();
        Ok(())
    }

    /// Same as `attach_impl`, but takes an already-connected UnixStream instead
    /// of dialing out. Used by the elevated-tab path.
    ///
    /// # Cancel-safety
    /// The spawned thread runs `Client::new_with_stream`, which is a cheap
    /// `Async::new` wrap and thread spawn (both infallible). The only `.await`
    /// is on the thread join, which is cancel-safe: dropping the join handle
    /// detaches the thread (§B21), and the thread itself owns its `stream`,
    /// so no resource is lost on cancellation. The later `.await`s are RPC
    /// calls that are idempotent or no-ops to retry.
    async fn attach_impl_stream(
        &self,
        window_id: Option<WindowId>,
        ui: ConnectionUI,
        verbose: bool,
        stream: onlyterm_uds::UnixStream,
    ) -> anyhow::Result<()> {
        if self.state() == DomainState::Attached {
            // Already attached
            return Ok(());
        }

        let domain_id = self.local_domain_id;
        let config = self.config.clone();

        let activity = onlyterm_mux::activity::Activity::new();

        ui.async_run_and_log_error({
            let ui = ui.clone();
            async move {
                let client = spawn_into_new_thread(move || {
                    Client::new_with_stream(Some(domain_id), config.clone(), stream)
                })
                .await?;

                if verbose {
                    ui.output_str("Checking server version\n");
                }
                client.verify_version_compat(&ui).await?;

                if verbose {
                    ui.output_str("Version check OK!  Requesting pane list...\n");
                }
                let panes = client.list_panes().await?;
                if verbose {
                    ui.output_str(&format!(
                        "Server has {} tabs.  Attaching to local UI...\n",
                        panes.tabs.len()
                    ));
                }
                ClientDomain::finish_attach(domain_id, client, panes, window_id)
            }
        })
        .await
        .map_err(|e| {
            // Always report errors, even in non-verbose mode -- silence
            // should only ever hide routine progress noise, never a real
            // failure.
            ui.output_str(&format!("Error during attach: {:#}\n", e));
            e
        })?;

        if verbose {
            ui.output_str("Attached!\n");
        }
        drop(activity);
        ui.close();
        Ok(())
    }

    /// Same as `Domain::attach`, but shows a plain animated spinner in the
    /// placeholder tab instead of `Domain::attach`'s raw connection-progress
    /// text log. Used by the automatic per-tab-process-isolation spawn path
    /// (see onlyterm-gui's spawn_single_pane_tab): the technical log reads as
    /// broken rather than as progress for that automatic flow, and its
    /// multi-second round-trip latency invites impatient repeat clicks that
    /// each spawn their own redundant hosting process -- see spawn.rs's
    /// per-window spawn debounce, which this pairs with. Reuses the same
    /// termwiztermtab-backed placeholder tab `Domain::attach` uses (so the
    /// tab appears immediately and becomes the window's active tab, same as
    /// any other pane), just with quieter, animated content.
    pub async fn attach_with_spinner(&self, window_id: Option<WindowId>) -> anyhow::Result<()> {
        let ui = ConnectionUI::with_params(ConnectionUIParams {
            window_id,
            ..Default::default()
        });

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let ui = ui.clone();
            let stop = Arc::clone(&stop);
            onlyterm_promise::spawn::spawn(async move {
                // Plain ASCII spinner -- Braille Patterns glyphs (the more
                // common choice for spinners) rendered as tofu/missing-glyph
                // boxes in the default font here (confirmed live), so this
                // sticks to characters every monospace font is guaranteed
                // to have.
                const FRAMES: &[char] = &['|', '/', '-', '\\'];
                let mut i = 0usize;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    ui.output(vec![
                        termwiz::surface::Change::ClearScreen(Default::default()),
                        termwiz::surface::Change::CursorPosition {
                            x: termwiz::surface::Position::Absolute(0),
                            y: termwiz::surface::Position::Absolute(0),
                        },
                        termwiz::surface::Change::Text(format!(
                            "{} Opening tab...",
                            FRAMES[i % FRAMES.len()]
                        )),
                    ]);
                    i += 1;
                    smol::Timer::after(std::time::Duration::from_millis(90)).await;
                }
            })
            .detach();
        }

        let result = self.attach_impl(window_id, ui, false).await;
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        result
    }

    /// Same as `attach_with_spinner`, but takes an already-connected UnixStream
    /// instead of dialing out. Used by the elevated-tab path: the stream is already
    /// authenticated via the WebSocket rendezvous handshake in
    /// `crate::elevate::spawn_elevated_single_pane`.
    ///
    /// # Cancel-safety
    /// This function spawns a detached spinner task (safe to drop), then awaits
    /// `attach_impl_stream`. The spinner task's `AtomicBool` is `Relaxed`-ordered:
    /// if cancellation races with the write, the worst outcome is an extra frame
    /// or a missed stop signal, both benign. `attach_impl_stream` itself is
    /// cancel-safe (see its doc comment).
    pub async fn attach_stream_with_spinner(
        &self,
        window_id: Option<WindowId>,
        stream: onlyterm_uds::UnixStream,
    ) -> anyhow::Result<()> {
        let ui = ConnectionUI::with_params(ConnectionUIParams {
            window_id,
            ..Default::default()
        });

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let ui = ui.clone();
            let stop = Arc::clone(&stop);
            onlyterm_promise::spawn::spawn(async move {
                const FRAMES: &[char] = &['|', '/', '-', '\\'];
                let mut i = 0usize;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    ui.output(vec![
                        termwiz::surface::Change::ClearScreen(Default::default()),
                        termwiz::surface::Change::CursorPosition {
                            x: termwiz::surface::Position::Absolute(0),
                            y: termwiz::surface::Position::Absolute(0),
                        },
                        termwiz::surface::Change::Text(format!(
                            "{} Opening tab...",
                            FRAMES[i % FRAMES.len()]
                        )),
                    ]);
                    i += 1;
                    smol::Timer::after(std::time::Duration::from_millis(90)).await;
                }
            })
            .detach();
        }

        let result = self.attach_impl_stream(window_id, ui, false, stream).await;
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        result
    }
}
