use super::*;
use codec::*;

#[test]
fn test_pdu_policy_unrestricted_allows_all() {
    let policy = PduPolicy::Unrestricted;

    // Test a few representative PDU types - all should be allowed
    assert!(policy.is_allowed(&codec::Pdu::Ping(Ping {})));

    // The most dangerous PDU should be allowed in unrestricted mode
    assert!(policy.is_allowed(&codec::Pdu::SpawnV2(codec::SpawnV2 {
        domain: config::keyassignment::SpawnTabDomain::CurrentPaneDomain,
        window_id: None,
        command: None,
        command_dir: None,
        size: Default::default(),
        workspace: "".to_string(),
        attach: false,
    })));

    // Test a layout manipulation PDU should also be allowed
    assert!(policy.is_allowed(&codec::Pdu::SplitPane(codec::SplitPane {
        pane_id: 0,
        split_request: Default::default(),
        command: None,
        command_dir: None,
        domain: config::keyassignment::SpawnTabDomain::CurrentPaneDomain,
        move_pane_id: None,
    })));
}

#[test]
fn test_pdu_policy_elevated_allowlist_allows_only_allowed_pdus() {
    let policy = PduPolicy::ElevatedSinglePaneAllowList;

    // Allowed PDUs - these should all return true

    // GetCodecVersion/SetClientId: unconditional parts of every client
    // attach handshake (see Client::verify_version_compat) -- rejecting
    // either of these breaks attaching to the elevated channel at all.
    assert!(policy.is_allowed(&codec::Pdu::GetCodecVersion(GetCodecVersion {})));
    assert!(policy.is_allowed(&codec::Pdu::SetClientId(SetClientId {
        client_id: mux::client::ClientId::default(),
        is_proxy: false,
    })));
    assert!(policy.is_allowed(&codec::Pdu::Ping(Ping {})));
    assert!(policy.is_allowed(&codec::Pdu::ListPanes(ListPanes {})));
    assert!(
        policy.is_allowed(&codec::Pdu::GetPaneRenderChanges(GetPaneRenderChanges {
            pane_id: 0,
        }))
    );
    assert!(policy.is_allowed(&codec::Pdu::GetLines(GetLines {
        pane_id: 0,
        lines: vec![],
    })));
    assert!(policy.is_allowed(&codec::Pdu::WriteToPane(WriteToPane {
        pane_id: 0,
        data: vec![],
    })));
    assert!(policy.is_allowed(&codec::Pdu::SendPaste(SendPaste {
        pane_id: 0,
        data: "".to_string(),
    })));
    assert!(policy.is_allowed(&codec::Pdu::Resize(Resize {
        pane_id: 0,
        containing_tab_id: 0,
        size: Default::default(),
    })));
    assert!(policy.is_allowed(&codec::Pdu::KillPane(KillPane { pane_id: 0 })));

    // SendKeyDown/SendMouseEvent: the actual client->server input path
    // ClientPane uses for ordinary keyboard/mouse input, not a
    // server->client notification -- an earlier version of this allow-list
    // rejected these on that mistaken assumption, which broke ALL keyboard
    // typing (though not paste, which uses a different PDU) in an elevated
    // tab. See is_allowed's doc comment for the live-confirmed symptom.
    assert!(policy.is_allowed(&codec::Pdu::SendKeyDown(SendKeyDown {
        pane_id: 0,
        event: termwiz::input::KeyEvent {
            key: termwiz::input::KeyCode::Char('a'),
            modifiers: termwiz::input::Modifiers::NONE,
        },
        input_serial: codec::InputSerial::now(),
    })));
    assert!(
        policy.is_allowed(&codec::Pdu::SendMouseEvent(SendMouseEvent {
            pane_id: 0,
            event: wezterm_term::input::MouseEvent {
                kind: wezterm_term::input::MouseEventKind::Press,
                x: 0,
                y: 0,
                x_pixel_offset: 0,
                y_pixel_offset: 0,
                button: wezterm_term::input::MouseButton::Left,
                modifiers: wezterm_term::input::KeyModifiers::NONE,
            },
        }))
    );
}

#[test]
fn test_pdu_policy_elevated_allowlist_rejects_dangerous_pdus() {
    let policy = PduPolicy::ElevatedSinglePaneAllowList;

    // Rejected PDUs - these should all return false
    // The most dangerous: arbitrary process spawn
    assert!(!policy.is_allowed(&codec::Pdu::SpawnV2(codec::SpawnV2 {
        domain: config::keyassignment::SpawnTabDomain::CurrentPaneDomain,
        window_id: None,
        command: None,
        command_dir: None,
        size: Default::default(),
        workspace: "".to_string(),
        attach: false,
    })));

    // Pane layout manipulation - breaks single-pane contract
    assert!(!policy.is_allowed(&codec::Pdu::SplitPane(codec::SplitPane {
        pane_id: 0,
        split_request: Default::default(),
        command: None,
        command_dir: None,
        domain: config::keyassignment::SpawnTabDomain::CurrentPaneDomain,
        move_pane_id: None,
    })));
    assert!(
        !policy.is_allowed(&codec::Pdu::MovePaneToNewTab(codec::MovePaneToNewTab {
            pane_id: 0,
            window_id: None,
            workspace_for_new_window: None,
        }))
    );
    assert!(
        !policy.is_allowed(&codec::Pdu::SetPaneZoomed(SetPaneZoomed {
            pane_id: 0,
            containing_tab_id: 0,
            zoomed: false,
        }))
    );
    assert!(!policy.is_allowed(&codec::Pdu::RotatePanes(RotatePanes {
        pane_id: 0,
        direction: config::keyassignment::RotationDirection::Clockwise,
    })));
    assert!(!policy.is_allowed(&codec::Pdu::SwapActivePaneWithIndex(
        SwapActivePaneWithIndex {
            active_pane_id: 0,
            with_pane_index: 0,
            keep_focus: false,
        }
    )));
    assert!(
        !policy.is_allowed(&codec::Pdu::ActivatePaneDirection(ActivatePaneDirection {
            pane_id: 0,
            direction: config::keyassignment::PaneDirection::Up,
        }))
    );
    assert!(
        !policy.is_allowed(&codec::Pdu::AdjustPaneSize(AdjustPaneSize {
            pane_id: 0,
            direction: config::keyassignment::PaneDirection::Up,
            amount: 0,
        }))
    );

    // Workspace and window management
    assert!(
        !policy.is_allowed(&codec::Pdu::SetWindowWorkspace(SetWindowWorkspace {
            window_id: 0,
            workspace: "".to_string(),
        }))
    );
    assert!(
        !policy.is_allowed(&codec::Pdu::RenameWorkspace(RenameWorkspace {
            old_workspace: "".to_string(),
            new_workspace: "".to_string(),
        }))
    );

    // Client identity management: GetClientList enumerates ALL connected
    // clients (information disclosure beyond this session's own scope) and
    // is still rejected. SetClientId only writes this session's own
    // identity and is unconditionally sent by verify_version_compat as
    // part of every attach handshake -- it is now allow-listed (see
    // test_pdu_policy_elevated_allowlist_allows_only_allowed_pdus).
    assert!(!policy.is_allowed(&codec::Pdu::GetClientList(GetClientList)));

    // Scrollback manipulation
    assert!(!policy.is_allowed(&codec::Pdu::EraseScrollbackRequest(
        EraseScrollbackRequest {
            pane_id: 0,
            erase_mode: config::keyassignment::ScrollbackEraseMode::ScrollbackOnly,
        }
    )));
    assert!(!policy.is_allowed(&codec::Pdu::SearchScrollbackRequest(
        SearchScrollbackRequest {
            pane_id: 0,
            pattern: mux::pane::Pattern::CaseSensitiveString("".to_string()),
            range: 0..0,
            limit: None,
        }
    )));

    // Credential query. GetCodecVersion is NOT rejected -- see
    // test_pdu_policy_elevated_allowlist_allows_only_allowed_pdus.
    assert!(!policy.is_allowed(&codec::Pdu::GetTlsCreds(GetTlsCreds {})));

    // Metadata and state mutation
    assert!(
        !policy.is_allowed(&codec::Pdu::WindowTitleChanged(WindowTitleChanged {
            window_id: 0,
            title: "".to_string(),
        }))
    );
    assert!(
        !policy.is_allowed(&codec::Pdu::TabTitleChanged(TabTitleChanged {
            tab_id: 0,
            title: "".to_string(),
        }))
    );
    assert!(!policy.is_allowed(&codec::Pdu::SetPalette(SetPalette {
        pane_id: 0,
        palette: Default::default(),
    })));
}

/// A `Pane` whose only interesting property is the keyboard encoding it
/// reports; everything else is held constant so that `compute_changes` sees
/// no other reason to consider the pane changed.
struct FixedPane {
    keyboard_encoding: parking_lot::Mutex<termwiz::input::KeyboardEncoding>,
    writer: parking_lot::Mutex<Vec<u8>>,
}

impl FixedPane {
    fn new() -> Self {
        Self {
            keyboard_encoding: parking_lot::Mutex::new(termwiz::input::KeyboardEncoding::Xterm),
            writer: parking_lot::Mutex::new(vec![]),
        }
    }
}

impl mux::pane::Pane for FixedPane {
    fn pane_id(&self) -> mux::pane::PaneId {
        1
    }

    fn get_keyboard_encoding(&self) -> termwiz::input::KeyboardEncoding {
        *self.keyboard_encoding.lock()
    }

    fn get_cursor_position(&self) -> mux::renderable::StableCursorPosition {
        mux::renderable::StableCursorPosition::default()
    }

    fn get_current_seqno(&self) -> termwiz::surface::SequenceNo {
        1
    }

    fn get_changed_since(
        &self,
        _lines: std::ops::Range<StableRowIndex>,
        _seqno: termwiz::surface::SequenceNo,
    ) -> rangeset::RangeSet<StableRowIndex> {
        rangeset::RangeSet::new()
    }

    fn get_lines(
        &self,
        lines: std::ops::Range<StableRowIndex>,
    ) -> (StableRowIndex, Vec<termwiz::surface::Line>) {
        let attrs = termwiz::cell::CellAttributes::default();
        let count = (lines.end - lines.start).max(1) as usize;
        (
            lines.start,
            (0..count)
                .map(|_| termwiz::surface::Line::from_text("", &attrs, 1, None))
                .collect(),
        )
    }

    fn with_lines_mut(
        &self,
        lines: std::ops::Range<StableRowIndex>,
        with_lines: &mut dyn mux::pane::WithPaneLines,
    ) {
        mux::pane::impl_with_lines_via_get_lines(self, lines, with_lines);
    }

    fn for_each_logical_line_in_stable_range_mut(
        &self,
        lines: std::ops::Range<StableRowIndex>,
        for_line: &mut dyn mux::pane::ForEachPaneLogicalLine,
    ) {
        mux::pane::impl_for_each_logical_line_via_get_logical_lines(self, lines, for_line);
    }

    fn get_logical_lines(
        &self,
        lines: std::ops::Range<StableRowIndex>,
    ) -> Vec<mux::pane::LogicalLine> {
        mux::pane::impl_get_logical_lines_via_get_lines(self, lines)
    }

    fn get_dimensions(&self) -> mux::renderable::RenderableDimensions {
        mux::renderable::RenderableDimensions {
            cols: 80,
            viewport_rows: 24,
            scrollback_rows: 24,
            physical_top: 0,
            scrollback_top: 0,
            dpi: 96,
            pixel_width: 800,
            pixel_height: 600,
            reverse_video: false,
        }
    }

    fn get_title(&self) -> String {
        "fixed".to_string()
    }

    fn send_paste(&self, _text: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>> {
        Ok(None)
    }

    fn writer(&self) -> parking_lot::MappedMutexGuard<'_, dyn std::io::Write> {
        parking_lot::MutexGuard::map(self.writer.lock(), |w| {
            let w: &mut dyn std::io::Write = w;
            w
        })
    }

    fn resize(&self, _size: wezterm_term::TerminalSize) -> anyhow::Result<()> {
        Ok(())
    }

    fn key_down(
        &self,
        _key: wezterm_term::KeyCode,
        _mods: wezterm_term::KeyModifiers,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn key_up(
        &self,
        _key: wezterm_term::KeyCode,
        _mods: wezterm_term::KeyModifiers,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn mouse_event(&self, _event: wezterm_term::MouseEvent) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_dead(&self) -> bool {
        false
    }

    fn palette(&self) -> wezterm_term::color::ColorPalette {
        wezterm_term::color::ColorPalette::default()
    }

    fn domain_id(&self) -> mux::domain::DomainId {
        0
    }

    fn is_mouse_grabbed(&self) -> bool {
        false
    }

    fn is_alt_screen_active(&self) -> bool {
        false
    }

    fn get_current_working_dir(&self, _policy: mux::pane::CachePolicy) -> Option<url::Url> {
        None
    }
}

/// A change of keyboard encoding must be enough, on its own, to produce a
/// `GetPaneRenderChangesResponse` -- and that response must carry the new
/// encoding.
///
/// This is the server half of the fix for the "Shift+Enter submits instead of
/// inserting a newline in Codex CLI" bug: negotiating win32-input-mode is a
/// DEC private mode set that need not touch the screen, the cursor, the title
/// or the sequence number, so without explicit change detection here the
/// client would keep believing that nothing was negotiated until some
/// unrelated output happened to force a push.
#[test]
fn keyboard_encoding_change_alone_produces_a_push() {
    use termwiz::escape::csi::KittyKeyboardFlags;
    use termwiz::input::KeyboardEncoding;

    let pane = std::sync::Arc::new(FixedPane::new());
    let pane: std::sync::Arc<dyn mux::pane::Pane> = pane.clone();
    let mut per_pane = per_pane::PerPane::default();

    // The first computation always reports, and carries the *current*
    // encoding -- this is what gives a freshly attached client the state of a
    // protocol negotiated before it attached.
    let first = per_pane
        .compute_changes(&pane, None)
        .expect("the initial computation always reports changes");
    assert_eq!(first.keyboard_encoding, KeyboardEncoding::Xterm);

    // Nothing has changed, so nothing should be pushed.
    assert!(
        per_pane.compute_changes(&pane, None).is_none(),
        "an unchanged pane must not produce a push"
    );

    for encoding in [
        KeyboardEncoding::Win32,
        KeyboardEncoding::Kitty(KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES),
        KeyboardEncoding::Xterm,
    ] {
        *pane
            .downcast_ref::<FixedPane>()
            .unwrap()
            .keyboard_encoding
            .lock() = encoding;

        let resp = per_pane
            .compute_changes(&pane, None)
            .unwrap_or_else(|| panic!("changing the encoding to {:?} must produce a push", encoding));
        assert_eq!(
            resp.keyboard_encoding, encoding,
            "the pushed response must carry the new encoding"
        );

        // ...and the change must be latched, so that an unchanged pane goes
        // back to producing no traffic at all.
        assert!(
            per_pane.compute_changes(&pane, None).is_none(),
            "re-reporting the same encoding must not produce a push"
        );
    }
}
