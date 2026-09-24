use super::*;

impl Mux {
    pub fn set_banner(&self, banner: Option<String>) {
        *self.banner.write() = banner;
    }

    pub fn resolve_spawn_tab_domain(
        &self,
        // TODO: disambiguate with TabId
        pane_id: Option<PaneId>,
        domain: &onlyterm_config::keyassignment::SpawnTabDomain,
    ) -> anyhow::Result<Arc<dyn Domain>> {
        let domain = match domain {
            SpawnTabDomain::DefaultDomain => self.default_domain(),
            SpawnTabDomain::CurrentPaneDomain => match pane_id {
                Some(pane_id) => {
                    let (pane_domain_id, _window_id, _tab_id) = self
                        .resolve_pane_id(pane_id)
                        .ok_or_else(|| anyhow!("pane_id {} invalid", pane_id))?;
                    let pane_domain = self
                        .get_domain(pane_domain_id)
                        .expect("resolve_pane_id to give valid domain_id");
                    // See `Domain::spawnable`'s doc comment: a single-pane
                    // hosting-process domain cannot host a second pane, so
                    // "follow the current pane's domain" (what a plain new-
                    // tab click/keybinding asks for) falls back to the
                    // default domain instead of resolving to a domain that
                    // would just fail (or, for the elevated case, be
                    // correctly rejected by that channel's PDU allow-list).
                    if pane_domain.spawnable() {
                        pane_domain
                    } else {
                        self.default_domain()
                    }
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

    /// Resolves the effective cwd for a newly spawned tab/pane.
    ///
    /// Task #469: when `command_dir` is already given, or there's no
    /// candidate pane, this returns synchronously without ever touching
    /// `pane.get_current_working_dir` -- that's the cheap, common path and
    /// it stays exactly as inexpensive as before. Only the fallback lookup
    /// (`pane.get_current_working_dir(policy)`) is `async`, and only its
    /// `CachePolicy::FetchImmediate` case is offloaded via `smol::unblock`:
    /// on Windows, when the shell hasn't reported OSC 7 (true for
    /// `cmd.exe`/stock PowerShell), that call falls through to
    /// `divine_foreground_process` -> `LocalProcessInfo::with_root_pid`,
    /// a full-system `CreateToolhelp32Snapshot` walk that opens every
    /// process on the machine. Both call sites (`spawn_tab_or_window`,
    /// `split_pane`) run on the GUI thread's `onlyterm_promise::spawn` executor, so
    /// running that snapshot inline here would stall the message loop on
    /// every new tab/split. `CachePolicy::AllowStale` never reaches this
    /// helper today (both call sites always pass `FetchImmediate`), but is
    /// left running inline if it ever does, since it's the cheap,
    /// non-blocking cache-hit/stale-return path.
    async fn resolve_cwd(
        &self,
        command_dir: Option<String>,
        pane: Option<Arc<dyn Pane>>,
        target_domain: DomainId,
        policy: CachePolicy,
    ) -> Option<String> {
        if command_dir.is_some() {
            return command_dir;
        }

        let pane = match pane {
            Some(pane) if pane.domain_id() == target_domain => pane,
            _ => return None,
        };

        let url = if policy == CachePolicy::FetchImmediate {
            smol::unblock(move || pane.get_current_working_dir(policy)).await
        } else {
            pane.get_current_working_dir(policy)
        };

        url.and_then(|url| {
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
        })
    }

    pub async fn split_pane(
        &self,
        // TODO: disambiguate with TabId
        pane_id: PaneId,
        request: SplitRequest,
        source: SplitSource,
        domain: onlyterm_config::keyassignment::SpawnTabDomain,
    ) -> anyhow::Result<(Arc<dyn Pane>, TerminalSize)> {
        let (pane_domain_id, window_id, tab_id) = self
            .resolve_pane_id(pane_id)
            .ok_or_else(|| anyhow!("pane_id {} invalid", pane_id))?;

        // Splitting follows the pane's domain even when that domain cannot host a new tab.
        let domain = match &domain {
            SpawnTabDomain::CurrentPaneDomain => {
                self.get_domain(pane_domain_id).ok_or_else(|| {
                    anyhow!("domain {} of pane {} not found", pane_domain_id, pane_id)
                })?
            }
            _ => self
                .resolve_spawn_tab_domain(Some(pane_id), &domain)
                .context("resolve_spawn_tab_domain")?,
        };

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
                command_dir: self
                    .resolve_cwd(
                        command_dir,
                        Some(Arc::clone(&current_pane)),
                        domain.domain_id(),
                        CachePolicy::FetchImmediate,
                    )
                    .await,
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
            // A window that exists but holds no tab is not a broken state: the
            // `--choose-tab` startup mode opens exactly that -- a window whose
            // only content is the New Tab Options dialog -- and this call is
            // what creates its first tab. Geometry and terminal config are
            // normally inherited from the active tab; with no tab to inherit
            // from, fall back to what the caller passed, which is the same
            // thing the new-window branch below does.
            match window.get_active() {
                Some(tab) => {
                    let pane = tab.get_active_pane().ok_or_else(|| {
                        anyhow!("active tab in window {} has no panes", window_id)
                    })?;
                    term_config = pane.get_config();
                    let size = tab.get_size();
                    (window_id, size)
                }
                None => {
                    term_config = None;
                    (window_id, size)
                }
            }
        } else {
            term_config = None;
            window_builder = self.new_empty_window(Some(workspace_for_new_window), window_position);
            (*window_builder, size)
        };

        if domain.state() == DomainState::Detached {
            domain.attach(Some(window_id)).await?;
        }

        let cwd = self
            .resolve_cwd(
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
            )
            .await;

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
