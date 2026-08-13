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

    // Client identity management
    assert!(!policy.is_allowed(&codec::Pdu::SetClientId(SetClientId {
        client_id: mux::client::ClientId::default(),
        is_proxy: false,
    })));
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

    // Version and credential queries
    assert!(!policy.is_allowed(&codec::Pdu::GetCodecVersion(GetCodecVersion {})));
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
