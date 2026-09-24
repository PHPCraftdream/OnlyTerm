/// PDU policy for the mux transport. The elevated channel permits only
/// constrained splits inside its hosted tab, never arbitrary commands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PduPolicy {
    /// Unrestricted: all PDUs are processed (default for daemon-mode Unix-domain
    /// sockets, where the only clients are those who successfully connected to a
    /// user-owned socket path).
    Unrestricted,

    /// Elevated hosted-tab mode. The historical variant name is retained for
    /// its WebSocket rendezvous call sites.
    ElevatedSinglePaneAllowList,
}

#[derive(Clone, Copy)]
pub(super) struct HostedPaneLayout {
    pub pane_id: onlyterm_mux::pane::PaneId,
    pub tab_id: onlyterm_mux::tab::TabId,
    pub width: usize,
    pub height: usize,
}

impl PduPolicy {
    pub(super) fn authorize(
        &self,
        pdu: &onlyterm_codec::Pdu,
        mux: &onlyterm_mux::Mux,
        pending_splits: usize,
    ) -> bool {
        if !self.is_allowed(pdu) {
            return false;
        }
        let (PduPolicy::ElevatedSinglePaneAllowList, onlyterm_codec::Pdu::SplitPane(split)) =
            (self, pdu)
        else {
            return true;
        };

        let panes: Option<Vec<_>> = mux
            .iter_panes()
            .into_iter()
            .map(|pane| {
                let pane_id = pane.pane_id();
                let (_, _, tab_id) = mux.resolve_pane_id(pane_id)?;
                let tab = mux.get_tab(tab_id)?;
                let position = tab
                    .iter_panes_ignoring_zoom()
                    .into_iter()
                    .find(|position| position.pane.pane_id() == pane_id)?;
                Some(HostedPaneLayout {
                    pane_id,
                    tab_id,
                    width: position.width,
                    height: position.height,
                })
            })
            .collect();
        panes.is_some_and(|panes| Self::hosted_split_target_allowed(split, &panes, pending_splits))
    }

    pub(super) fn hosted_split_target_allowed(
        split: &onlyterm_codec::SplitPane,
        panes: &[HostedPaneLayout],
        pending_splits: usize,
    ) -> bool {
        const MAX_HOSTED_PANES: usize = 16;

        if pending_splits != 0 || panes.len() >= MAX_HOSTED_PANES {
            return false;
        }
        let Some(target) = panes.iter().find(|pane| pane.pane_id == split.pane_id) else {
            return false;
        };
        if panes.iter().any(|pane| pane.tab_id != target.tab_id) {
            return false;
        }

        let total = match split.split_request.direction {
            onlyterm_mux::tab::SplitDirection::Horizontal => target.width,
            onlyterm_mux::tab::SplitDirection::Vertical => target.height,
        };
        Self::hosted_split_size_fits(total, split.split_request.size)
    }

    pub(super) fn hosted_split_size_fits(total: usize, size: onlyterm_mux::tab::SplitSize) -> bool {
        if total < 3 {
            return false;
        }

        let target = match size {
            onlyterm_mux::tab::SplitSize::Cells(n) => n,
            onlyterm_mux::tab::SplitSize::Percent(n) if (1..100).contains(&n) => {
                let Some(cells) = total.checked_mul(n as usize) else {
                    return false;
                };
                (cells / 100).max(1)
            }
            _ => return false,
        };

        (1..=total - 2).contains(&target)
    }

    /// Checks PDU shape. `authorize` also checks live tab, geometry, and limits.
    pub fn is_allowed(&self, pdu: &onlyterm_codec::Pdu) -> bool {
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
                // - KillPane: terminate a hosted pane.
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
                // - SplitPane with a client-supplied command, directory,
                //   moved pane, or whole-tab layout change.
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
                match pdu {
                    onlyterm_codec::Pdu::SplitPane(split) => {
                        split.command.is_none()
                            && split.command_dir.is_none()
                            && split.move_pane_id.is_none()
                            && matches!(
                                split.domain,
                                onlyterm_config::keyassignment::SpawnTabDomain::CurrentPaneDomain
                            )
                            && !split.split_request.top_level
                            && split.split_request.target_is_second
                            && match split.split_request.size {
                                onlyterm_mux::tab::SplitSize::Cells(n) => n > 0 && n < usize::MAX,
                                onlyterm_mux::tab::SplitSize::Percent(n) => (1..100).contains(&n),
                            }
                    }
                    _ => matches!(
                        pdu,
                        onlyterm_codec::Pdu::GetCodecVersion(_)
                            | onlyterm_codec::Pdu::SetClientId(_)
                            | onlyterm_codec::Pdu::Ping(_)
                            | onlyterm_codec::Pdu::ListPanes(_)
                            | onlyterm_codec::Pdu::SendKeyDown(_)
                            | onlyterm_codec::Pdu::SendMouseEvent(_)
                            | onlyterm_codec::Pdu::GetPaneRenderChanges(_)
                            | onlyterm_codec::Pdu::GetLines(_)
                            | onlyterm_codec::Pdu::WriteToPane(_)
                            | onlyterm_codec::Pdu::SendPaste(_)
                            | onlyterm_codec::Pdu::Resize(_)
                            | onlyterm_codec::Pdu::KillPane(_)
                            | onlyterm_codec::Pdu::SetPalette(_)
                            | onlyterm_codec::Pdu::SetFocusedPane(_)
                            | onlyterm_codec::Pdu::WindowTitleChanged(_)
                            | onlyterm_codec::Pdu::TabTitleChanged(_)
                    ),
                }
            }
        }
    }
}
