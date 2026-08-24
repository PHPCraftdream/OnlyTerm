mod per_pane;
mod spawn;

#[cfg(test)]
mod tests;

use anyhow::{anyhow, Context};
use codec::*;
use config::keyassignment::RotationDirection;
use config::TermConfig;
use mux::client::ClientId;
use mux::pane::PaneId;
use mux::tab::{NotifyMux, TabId};
use mux::{Mux, MuxNotification};
use onlyterm_term::terminal::Alert;
use onlyterm_term::StableRowIndex;
use promise::spawn::spawn_into_main_thread;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use per_pane::{maybe_push_pane_changes, PerPane};
use spawn::{schedule_domain_spawn_v2, schedule_move_pane, schedule_split_pane};

/// Policy restricting which PDU types a SessionHandler will process.
///
/// This is a security boundary: the elevated single-pane WebSocket rendezvous
/// transport (`onlyterm-elevated-transport`) uses `ElevatedSinglePaneAllowList`
/// to ensure that any process able to reach the rendezvous channel cannot
/// spawn arbitrary elevated processes or otherwise escape the single-pane
/// sandbox. Windows Terminal's maintainers explicitly declined to ship this
/// feature without such a restriction ("any other unelevated application could
/// send input to the Terminal's HWND" / reach the IPC channel). See the
/// `onlyterm-elevated-transport` module doc for full design context.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PduPolicy {
    /// Unrestricted: all PDUs are processed (default for daemon-mode Unix-domain
    /// sockets, where the only clients are those who successfully connected to a
    /// user-owned socket path).
    Unrestricted,

    /// Elevated single-pane mode: only a minimal allow-list of PDUs is processed.
    /// Rejects any PDU that could spawn processes, modify window/tab structure,
    /// or otherwise escape the single-pane sandbox. Used exclusively for the
    /// WebSocket rendezvous channel where the elevated child connects back to
    /// the (non-elevated) GUI.
    ElevatedSinglePaneAllowList,
}

impl PduPolicy {
    /// Returns true if the given PDU type is allowed under this policy.
    ///
    /// For `ElevatedSinglePaneAllowList`, this is a very conservative allow-list:
    /// only read-only observation PDUs, direct user input to the existing pane,
    /// and pane lifecycle termination are permitted. Anything that could spawn
    /// a process, create/modify windows/tabs, or otherwise escape the single-pane
    /// sandbox is rejected.
    pub fn is_allowed(&self, pdu: &codec::Pdu) -> bool {
        match self {
            PduPolicy::Unrestricted => true,
            PduPolicy::ElevatedSinglePaneAllowList => {
                // Allow-list: only these PDUs are permitted over the elevated
                // rendezvous channel. The reasoning for each:
                //
                // - GetCodecVersion: read-only protocol version handshake --
                //   sent unconditionally by every client (elevated or not) as
                //   the very first step of `Client::verify_version_compat`,
                //   before anything else can happen. Confirmed live: without
                //   this, EVERY elevated attach fails immediately with
                //   "unexpected response Ok(ErrorResponse(...))", since the
                //   client has no fallback for a rejected version check.
                // - SetClientId: client identity bookkeeping (hostname/pid,
                //   for `onlyterm cli list-clients`-style display) -- also
                //   sent unconditionally right after a successful version
                //   check, by `verify_version_compat` itself. No privileged
                //   effect, purely descriptive metadata.
                // - Ping: protocol health check, no side effects.
                // - ListPanes: read-only observation of pane tree.
                // - GetPaneRenderChanges: read-only pane state fetch for rendering.
                // - GetLines: read-only scrollback/history fetch.
                // - WriteToPane: direct user input (keystrokes) to the existing pane.
                // - SendPaste: paste operation into the existing pane.
                // - SendKeyDown/SendMouseEvent: THE actual client->server input path a
                //   ClientPane uses for ordinary keyboard/mouse input (see
                //   ClientPane::key_down/mouse_event) -- this is direct user input to
                //   the existing pane, exactly like WriteToPane/SendPaste, not a
                //   server->client notification. An earlier version of this allow-list
                //   rejected these on the mistaken assumption that they only ever flow
                //   the other way; confirmed live that rejecting SendKeyDown breaks ALL
                //   keyboard typing in an elevated tab (SendPaste still worked, which is
                //   what made this reachable at all -- paste uses a different PDU).
                // - Resize: terminal geometry change, no privilege escalation.
                // - KillPane: terminate the single pane (normal exit path).
                // - SetPalette: the GUI pushes the user's configured colour
                //   scheme down to the pane's terminal. Confirmed live via
                //   this very allow-list's own rejection log: without it the
                //   elevated pane keeps the *default* palette while the GUI
                //   renders with the user's scheme, which showed up as
                //   pasted text on a grey background with a foreground so
                //   close to it that the text was only readable once
                //   selected. Purely cosmetic state -- it cannot spawn,
                //   read, or escalate anything.
                // - SetFocusedPane/WindowTitleChanged/TabTitleChanged:
                //   presentation bookkeeping the GUI sends for the session it
                //   already owns (which pane has focus, what the title is).
                //   Also seen rejected in that same log. No privileged
                //   effect; rejecting them only desynchronises the UI.
                //
                // Explicitly rejected (not exhaustive, but the most dangerous):
                // - SpawnV2: arbitrary process spawn with elevated privileges (the
                //   exact attack surface this allow-list defends against).
                // - SplitPane: would create additional panes, breaking the
                //   "single-pane" contract.
                // - MovePaneToNewTab: would create additional tabs/windows.
                // - SetWindowWorkspace/RenameWorkspace: workspace management.
                // - GetClientList: enumerates all connected clients (info
                //   disclosure beyond this single elevated session's scope).
                // - EraseScrollbackRequest/SearchScrollbackRequest: scrollback
                //   manipulation (data loss/exfiltration).
                // - SetPaneZoomed/GetPaneDirection/ActivatePaneDirection/
                //   SwapActivePaneWithIndex/RotatePanes/AdjustPaneSize: pane
                //   layout manipulation.
                // - GetPaneRenderableDimensions/GetImageCell: rendering internals.
                // - GetTlsCreds: credential query.
                matches!(
                    pdu,
                    codec::Pdu::GetCodecVersion(_)
                        | codec::Pdu::SetClientId(_)
                        | codec::Pdu::Ping(_)
                        | codec::Pdu::ListPanes(_)
                        | codec::Pdu::SendKeyDown(_)
                        | codec::Pdu::SendMouseEvent(_)
                        | codec::Pdu::GetPaneRenderChanges(_)
                        | codec::Pdu::GetLines(_)
                        | codec::Pdu::WriteToPane(_)
                        | codec::Pdu::SendPaste(_)
                        | codec::Pdu::Resize(_)
                        | codec::Pdu::KillPane(_)
                        | codec::Pdu::SetPalette(_)
                        | codec::Pdu::SetFocusedPane(_)
                        | codec::Pdu::WindowTitleChanged(_)
                        | codec::Pdu::TabTitleChanged(_)
                )
            }
        }
    }
}

#[derive(Clone)]
pub struct PduSender {
    func: Arc<dyn Fn(DecodedPdu) -> anyhow::Result<()> + Send + Sync>,
}

impl PduSender {
    pub fn send(&self, pdu: DecodedPdu) -> anyhow::Result<()> {
        (self.func)(pdu)
    }

    pub fn new<T>(f: T) -> Self
    where
        T: Fn(DecodedPdu) -> anyhow::Result<()> + Send + Sync + 'static,
    {
        Self { func: Arc::new(f) }
    }
}

pub struct SessionHandler {
    to_write_tx: PduSender,
    per_pane: HashMap<TabId, Arc<Mutex<PerPane>>>,
    client_id: Option<Arc<ClientId>>,
    proxy_client_id: Option<ClientId>,
    policy: PduPolicy,
}

impl Drop for SessionHandler {
    fn drop(&mut self) {
        if let Some(client_id) = self.client_id.take() {
            let mux = Mux::get();
            mux.unregister_client(&client_id);
        }
    }
}

impl SessionHandler {
    pub fn new(to_write_tx: PduSender, policy: PduPolicy) -> Self {
        Self {
            to_write_tx,
            per_pane: HashMap::new(),
            client_id: None,
            proxy_client_id: None,
            policy,
        }
    }

    pub(crate) fn per_pane(&mut self, pane_id: PaneId) -> Arc<Mutex<PerPane>> {
        Arc::clone(
            self.per_pane
                .entry(pane_id)
                .or_insert_with(|| Arc::new(Mutex::new(PerPane::default()))),
        )
    }

    pub fn schedule_pane_push(&mut self, pane_id: PaneId) {
        let sender = self.to_write_tx.clone();
        let per_pane = self.per_pane(pane_id);
        spawn_into_main_thread(async move {
            let mux = Mux::get();
            let pane = mux
                .get_pane(pane_id)
                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
            maybe_push_pane_changes(&pane, sender, per_pane)?;
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    pub fn process_one(&mut self, decoded: DecodedPdu) {
        let start = Instant::now();
        let sender = self.to_write_tx.clone();
        let serial = decoded.serial;

        if let Some(client_id) = &self.client_id {
            if decoded.pdu.is_user_input() {
                Mux::get().client_had_input(client_id);
            }
        }

        // Policy check: reject disallowed PDUs for ElevatedSinglePaneAllowList.
        // This is a security boundary: any process able to reach the elevated
        // rendezvous channel must not be able to spawn arbitrary elevated
        // processes or otherwise escape the single-pane sandbox.
        if !self.policy.is_allowed(&decoded.pdu) {
            let pdu_name = decoded.pdu.pdu_name();
            log::warn!(
                "PDU {} (serial {}) rejected by policy {:?}",
                pdu_name,
                serial,
                self.policy
            );
            sender
                .send(DecodedPdu {
                    pdu: codec::Pdu::ErrorResponse(codec::ErrorResponse {
                        reason: format!(
                            "PDU {} not permitted over the elevated single-pane rendezvous channel",
                            pdu_name
                        ),
                    }),
                    serial,
                })
                .ok();
            return;
        }

        let send_response = move |result: anyhow::Result<Pdu>| {
            let pdu = match result {
                Ok(pdu) => pdu,
                Err(err) => Pdu::ErrorResponse(ErrorResponse {
                    reason: format!("Error: {err:#}"),
                }),
            };
            log::trace!("{} processing time {:?}", serial, start.elapsed());
            sender.send(DecodedPdu { pdu, serial }).ok();
        };

        fn catch<F, SND>(f: F, send_response: SND)
        where
            F: FnOnce() -> anyhow::Result<Pdu>,
            SND: Fn(anyhow::Result<Pdu>),
        {
            send_response(f());
        }

        match decoded.pdu {
            Pdu::Ping(Ping {}) => send_response(Ok(Pdu::Pong(Pong {}))),
            Pdu::SetWindowWorkspace(SetWindowWorkspace {
                window_id,
                workspace,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let mut window = mux
                                .get_window_mut(window_id)
                                .ok_or_else(|| anyhow!("window {} is invalid", window_id))?;
                            window.set_workspace(&workspace);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::SetClientId(SetClientId {
                mut client_id,
                is_proxy,
            }) => {
                if is_proxy {
                    if self.proxy_client_id.is_none() {
                        // Copy proxy identity, but don't assign it to the mux;
                        // we'll use it to annotate the actual clients own
                        // identity when they send it
                        self.proxy_client_id.replace(client_id);
                    }
                } else {
                    // If this session is a proxy, override the incoming id with
                    // the proxy information so that it is clear what is going
                    // on from the `onlyterm cli list-clients` information
                    if let Some(proxy_id) = &self.proxy_client_id {
                        client_id.ssh_auth_sock = proxy_id.ssh_auth_sock.clone();
                        // Note that this `via proxy pid` string is coupled
                        // with the logic in mux/src/ssh_agent
                        client_id.hostname =
                            format!("{} (via proxy pid {})", client_id.hostname, proxy_id.pid);
                    }

                    let client_id = Arc::new(client_id);
                    self.client_id.replace(client_id.clone());
                    spawn_into_main_thread(async move {
                        let mux = Mux::get();
                        mux.register_client(client_id);
                    })
                    .detach();
                }
                send_response(Ok(Pdu::UnitResponse(UnitResponse {})))
            }
            Pdu::SetFocusedPane(SetFocusedPane { pane_id }) => {
                let client_id = self.client_id.clone();
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let _identity = mux.with_identity(client_id);

                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow::anyhow!("pane {pane_id} not found"))?;

                            let (_domain_id, window_id, tab_id) = mux
                                .resolve_pane_id(pane_id)
                                .ok_or_else(|| anyhow::anyhow!("pane {pane_id} not found"))?;
                            {
                                let mut window =
                                    mux.get_window_mut(window_id).ok_or_else(|| {
                                        anyhow::anyhow!("window {window_id} not found")
                                    })?;
                                let tab_idx = window.idx_by_id(tab_id).ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "tab {tab_id} isn't really in window {window_id}!?"
                                    )
                                })?;
                                window.save_and_then_set_active(tab_idx);
                            }
                            let tab = mux
                                .get_tab(tab_id)
                                .ok_or_else(|| anyhow::anyhow!("tab {tab_id} not found"))?;
                            // We emit our own PaneFocused below; suppress the
                            // duplicate advise_focus_change would emit.
                            // See <https://github.com/wezterm/wezterm/issues/4390>
                            tab.set_active_pane_with_notify(&pane, NotifyMux::No);

                            mux.record_focus_for_current_identity(pane_id);
                            mux.notify(mux::MuxNotification::PaneFocused(pane_id));

                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::GetClientList(GetClientList) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let clients = mux.iter_clients();
                            Ok(Pdu::GetClientListResponse(GetClientListResponse {
                                clients,
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::ListPanes(ListPanes {}) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let mut tabs = vec![];
                            let mut tab_titles = vec![];
                            let mut window_titles = HashMap::new();
                            for window_id in mux.iter_windows().into_iter() {
                                let window = mux.get_window(window_id).unwrap();
                                window_titles.insert(window_id, window.get_title().to_string());
                                for tab in window.iter() {
                                    tabs.push(tab.codec_pane_tree());
                                    tab_titles.push(tab.get_title());
                                }
                            }
                            log::trace!("ListPanes {tabs:#?} {tab_titles:?}");
                            Ok(Pdu::ListPanesResponse(ListPanesResponse {
                                tabs,
                                tab_titles,
                                window_titles,
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::RenameWorkspace(RenameWorkspace {
                old_workspace,
                new_workspace,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            mux.rename_workspace(&old_workspace, &new_workspace);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    );
                })
                .detach();
            }

            Pdu::WriteToPane(WriteToPane { pane_id, data }) => {
                let sender = self.to_write_tx.clone();
                let per_pane = self.per_pane(pane_id);
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.writer().write_all(&data)?;
                            maybe_push_pane_changes(&pane, sender, per_pane)?;
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    );
                })
                .detach();
            }
            Pdu::EraseScrollbackRequest(EraseScrollbackRequest {
                pane_id,
                erase_mode,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.erase_scrollback(erase_mode);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    );
                })
                .detach();
            }
            Pdu::KillPane(KillPane { pane_id }) => {
                let sender = self.to_write_tx.clone();
                let per_pane = self.per_pane(pane_id);
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.kill();
                            mux.remove_pane(pane_id);
                            maybe_push_pane_changes(&pane, sender, per_pane)?;
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    );
                })
                .detach();
            }
            Pdu::SendPaste(SendPaste { pane_id, data }) => {
                let sender = self.to_write_tx.clone();
                let per_pane = self.per_pane(pane_id);
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.send_paste(&data)?;
                            maybe_push_pane_changes(&pane, sender, per_pane)?;
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::SearchScrollbackRequest(SearchScrollbackRequest {
                pane_id,
                pattern,
                range,
                limit,
            }) => {
                use mux::pane::Pattern;

                async fn do_search(
                    pane_id: TabId,
                    pattern: Pattern,
                    range: std::ops::Range<StableRowIndex>,
                    limit: Option<u32>,
                ) -> anyhow::Result<Pdu> {
                    let mux = Mux::get();
                    let pane = mux
                        .get_pane(pane_id)
                        .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;

                    pane.search(pattern, range, limit).await.map(|results| {
                        Pdu::SearchScrollbackResponse(SearchScrollbackResponse { results })
                    })
                }

                spawn_into_main_thread(async move {
                    promise::spawn::spawn(async move {
                        let result = do_search(pane_id, pattern, range, limit).await;
                        send_response(result);
                    })
                    .detach();
                })
                .detach();
            }

            Pdu::SetPaneZoomed(SetPaneZoomed {
                containing_tab_id,
                pane_id,
                zoomed,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let tab = mux
                                .get_tab(containing_tab_id)
                                .ok_or_else(|| anyhow!("no such tab {}", containing_tab_id))?;
                            match tab.get_zoomed_pane() {
                                Some(p) => {
                                    let is_zoomed = p.pane_id() == pane_id;
                                    if is_zoomed != zoomed {
                                        tab.set_zoomed(false);
                                        if zoomed {
                                            tab.set_active_pane(&pane);
                                            tab.set_zoomed(zoomed);
                                        }
                                    }
                                }
                                None => {
                                    if zoomed {
                                        tab.set_active_pane(&pane);
                                        tab.set_zoomed(zoomed);
                                    }
                                }
                            }
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::GetPaneDirection(GetPaneDirection { pane_id, direction }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let (_domain_id, _window_id, tab_id) = mux
                                .resolve_pane_id(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let tab = mux
                                .get_tab(tab_id)
                                .ok_or_else(|| anyhow!("no such tab {}", tab_id))?;
                            let panes = tab.iter_panes_ignoring_zoom();
                            let pane_id = tab
                                .get_pane_direction(direction, true)
                                .map(|pane_index| panes[pane_index].pane.pane_id());

                            Ok(Pdu::GetPaneDirectionResponse(GetPaneDirectionResponse {
                                pane_id,
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::ActivatePaneDirection(ActivatePaneDirection { pane_id, direction }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let (_domain_id, _window_id, tab_id) = mux
                                .resolve_pane_id(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let tab = mux
                                .get_tab(tab_id)
                                .ok_or_else(|| anyhow!("no such tab {}", tab_id))?;
                            tab.activate_pane_direction(direction);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::SwapActivePaneWithIndex(SwapActivePaneWithIndex {
                active_pane_id,
                with_pane_index,
                keep_focus,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let (_domain_id, _window_id, tab_id) = mux
                                .resolve_pane_id(active_pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", active_pane_id))?;
                            let tab = mux
                                .get_tab(tab_id)
                                .ok_or_else(|| anyhow!("no such tab {}", tab_id))?;
                            tab.local_swap_active_with_index(with_pane_index, keep_focus);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::RotatePanes(RotatePanes { pane_id, direction }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let (_domain_id, _window_id, tab_id) = mux
                                .resolve_pane_id(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let tab = mux
                                .get_tab(tab_id)
                                .ok_or_else(|| anyhow!("no such tab {}", tab_id))?;

                            match direction {
                                RotationDirection::Clockwise => tab.local_rotate_clockwise(),
                                RotationDirection::CounterClockwise => {
                                    tab.local_rotate_counter_clockwise()
                                }
                            };

                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::Resize(Resize {
                containing_tab_id,
                pane_id,
                size,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.resize(size)?;
                            let tab = mux
                                .get_tab(containing_tab_id)
                                .ok_or_else(|| anyhow!("no such tab {}", containing_tab_id))?;
                            tab.rebuild_splits_sizes_from_contained_panes();
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::SendKeyDown(SendKeyDown {
                pane_id,
                event,
                input_serial,
            }) => {
                let sender = self.to_write_tx.clone();
                let per_pane = self.per_pane(pane_id);
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.key_down(event.key, event.modifiers)?;

                            // For a key press, we want to always send back the
                            // cursor position so that the predictive echo doesn't
                            // leave the cursor in the wrong place
                            let mut per_pane = per_pane.lock().unwrap();
                            if let Some(resp) = per_pane.compute_changes(&pane, Some(input_serial))
                            {
                                sender.send(DecodedPdu {
                                    pdu: Pdu::GetPaneRenderChangesResponse(resp),
                                    serial: 0,
                                })?;
                            }
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::SendMouseEvent(SendMouseEvent { pane_id, event }) => {
                let sender = self.to_write_tx.clone();
                let per_pane = self.per_pane(pane_id);
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            pane.mouse_event(event)?;
                            maybe_push_pane_changes(&pane, sender, per_pane)?;
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::SpawnV2(spawn) => {
                let client_id = self.client_id.clone();
                spawn_into_main_thread(async move {
                    schedule_domain_spawn_v2(spawn, send_response, client_id);
                })
                .detach();
            }

            Pdu::SplitPane(split) => {
                let client_id = self.client_id.clone();
                spawn_into_main_thread(async move {
                    schedule_split_pane(split, send_response, client_id);
                })
                .detach();
            }

            Pdu::MovePaneToNewTab(request) => {
                let client_id = self.client_id.clone();
                spawn_into_main_thread(async move {
                    schedule_move_pane(request, send_response, client_id);
                })
                .detach();
            }

            Pdu::GetPaneRenderableDimensions(GetPaneRenderableDimensions { pane_id }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let cursor_position = pane.get_cursor_position();
                            let dimensions = pane.get_dimensions();
                            Ok(Pdu::GetPaneRenderableDimensionsResponse(
                                GetPaneRenderableDimensionsResponse {
                                    pane_id,
                                    cursor_position,
                                    dimensions,
                                },
                            ))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::GetPaneRenderChanges(GetPaneRenderChanges { pane_id, .. }) => {
                let sender = self.to_write_tx.clone();
                let per_pane = self.per_pane(pane_id);
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let is_alive = match mux.get_pane(pane_id) {
                                Some(pane) => {
                                    maybe_push_pane_changes(&pane, sender, per_pane)?;
                                    true
                                }
                                None => false,
                            };
                            Ok(Pdu::LivenessResponse(LivenessResponse {
                                pane_id,
                                is_alive,
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::GetLines(GetLines { pane_id, lines }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;
                            let mut lines_and_indices = vec![];

                            for range in lines {
                                let (first_row, lines) = pane.get_lines(range);
                                for (idx, mut line) in lines.into_iter().enumerate() {
                                    let stable_row = first_row + idx as StableRowIndex;
                                    line.compress_for_scrollback();
                                    lines_and_indices.push((stable_row, line));
                                }
                            }
                            Ok(Pdu::GetLinesResponse(GetLinesResponse {
                                pane_id,
                                lines: lines_and_indices.into(),
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::GetImageCell(GetImageCell {
                pane_id,
                line_idx,
                cell_idx,
                data_hash,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let mut data = None;

                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;

                            let (_, lines) = pane.get_lines(line_idx..line_idx + 1);
                            'found_data: for line in lines {
                                if let Some(cell) = line.get_cell(cell_idx) {
                                    if let Some(images) = cell.attrs().images() {
                                        for im in images {
                                            if im.image_data().hash() == data_hash {
                                                data.replace(im.image_data().clone());
                                                break 'found_data;
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(Pdu::GetImageCellResponse(GetImageCellResponse {
                                pane_id,
                                data,
                            }))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::GetCodecVersion(_) => {
                match std::env::current_exe().context("resolving current_exe") {
                    Err(err) => send_response(Err(err)),
                    Ok(executable_path) => {
                        send_response(Ok(Pdu::GetCodecVersionResponse(GetCodecVersionResponse {
                            codec_vers: CODEC_VERSION,
                            version_string: config::onlyterm_version().to_owned(),
                            executable_path,
                            config_file_path: std::env::var_os("ONLYTERM_CONFIG_FILE")
                                .map(Into::into),
                        })))
                    }
                }
            }

            Pdu::GetTlsCreds(_) => {
                send_response(Err(anyhow!(
                    "TLS mux support has been removed; GetTlsCreds is no longer available"
                )));
            }
            Pdu::WindowTitleChanged(WindowTitleChanged { window_id, title }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let mut window = mux
                                .get_window_mut(window_id)
                                .ok_or_else(|| anyhow!("no such window {window_id}"))?;

                            window.set_title(&title);

                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::TabTitleChanged(TabTitleChanged { tab_id, title }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let tab = mux
                                .get_tab(tab_id)
                                .ok_or_else(|| anyhow!("no such tab {tab_id}"))?;

                            tab.set_title(&title);

                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }
            Pdu::SetPalette(SetPalette { pane_id, palette }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let pane = mux
                                .get_pane(pane_id)
                                .ok_or_else(|| anyhow!("no such pane {}", pane_id))?;

                            match pane.get_config() {
                                Some(config) => match config.downcast_ref::<TermConfig>() {
                                    Some(tc) => tc.set_client_palette(palette),
                                    None => {
                                        log::error!(
                                            "pane {pane_id} doesn't \
                                            have TermConfig as its config! \
                                            Ignoring client palette update"
                                        );
                                    }
                                },
                                None => {
                                    let config = TermConfig::new();
                                    config.set_client_palette(palette);
                                    pane.set_config(Arc::new(config));
                                }
                            }

                            mux.notify(MuxNotification::Alert {
                                pane_id,
                                alert: Alert::PaletteChanged,
                            });

                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::AdjustPaneSize(AdjustPaneSize {
                pane_id,
                direction,
                amount,
            }) => {
                spawn_into_main_thread(async move {
                    catch(
                        move || {
                            let mux = Mux::get();
                            let (_pane_domain_id, _window_id, tab_id) = mux
                                .resolve_pane_id(pane_id)
                                .ok_or_else(|| anyhow!("pane_id {} invalid", pane_id))?;

                            let tab = match mux.get_tab(tab_id) {
                                Some(tab) => tab,
                                None => {
                                    return Err(anyhow!(
                                        "Failed to retrieve tab with ID {}",
                                        tab_id
                                    ))
                                }
                            };

                            tab.adjust_pane_size(direction, amount);
                            Ok(Pdu::UnitResponse(UnitResponse {}))
                        },
                        send_response,
                    )
                })
                .detach();
            }

            Pdu::Invalid { .. } => send_response(Err(anyhow!("invalid PDU {:?}", decoded.pdu))),
            Pdu::Pong { .. }
            | Pdu::ListPanesResponse { .. }
            | Pdu::SetClipboard { .. }
            | Pdu::NotifyAlert { .. }
            | Pdu::SpawnResponse { .. }
            | Pdu::GetPaneRenderChangesResponse { .. }
            | Pdu::UnitResponse { .. }
            | Pdu::LivenessResponse { .. }
            | Pdu::GetPaneDirectionResponse { .. }
            | Pdu::SearchScrollbackResponse { .. }
            | Pdu::GetLinesResponse { .. }
            | Pdu::GetCodecVersionResponse { .. }
            | Pdu::WindowWorkspaceChanged { .. }
            | Pdu::GetTlsCredsResponse { .. }
            | Pdu::GetClientListResponse { .. }
            | Pdu::PaneRemoved { .. }
            | Pdu::PaneFocused { .. }
            | Pdu::TabReflowed { .. }
            | Pdu::GetImageCellResponse { .. }
            | Pdu::MovePaneToNewTabResponse { .. }
            | Pdu::TabAddedToWindow { .. }
            | Pdu::GetPaneRenderableDimensionsResponse { .. }
            | Pdu::ErrorResponse { .. } => {
                send_response(Err(anyhow!("expected a request, got {:?}", decoded.pdu)))
            }
        }
    }
}
