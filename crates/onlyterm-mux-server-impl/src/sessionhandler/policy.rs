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
