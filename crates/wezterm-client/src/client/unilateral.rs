use crate::domain::ClientDomain;
use crate::pane::ClientPane;
use anyhow::{anyhow, bail};
use codec::*;
use mux::domain::DomainId;
use mux::pane::PaneId;
use mux::Mux;

fn process_unilateral_inner(pane_id: PaneId, local_domain_id: DomainId, decoded: DecodedPdu) {
    promise::spawn::spawn(async move {
        process_unilateral_inner_async(pane_id, local_domain_id, decoded).await?;
        Ok::<(), anyhow::Error>(())
    })
    .detach();
}

async fn process_unilateral_inner_async(
    pane_id: PaneId,
    local_domain_id: DomainId,
    decoded: DecodedPdu,
) -> anyhow::Result<()> {
    let mux = match Mux::try_get() {
        Some(mux) => mux,
        None => {
            // This can happen for some client scenarios; it is ok to ignore it.
            return Ok(());
        }
    };

    let client_domain = mux
        .get_domain(local_domain_id)
        .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
    let client_domain = client_domain
        .downcast_ref::<ClientDomain>()
        .ok_or_else(|| anyhow!("domain {} is not a ClientDomain instance", local_domain_id))?;

    // If we get a push for a pane that we don't yet know about,
    // it means that some other client has manipulated the mux
    // topology; we need to re-sync.
    let local_pane_id = match client_domain.remote_to_local_pane_id(pane_id) {
        Some(p) => p,
        None => {
            log::debug!("got {decoded:?}, pane not found locally, resync");
            client_domain.resync().await?;
            client_domain
                .remote_to_local_pane_id(pane_id)
                .ok_or_else(|| {
                    anyhow!("remote pane id {} does not have a local pane id", pane_id)
                })?
        }
    };

    let pane = match mux.get_pane(local_pane_id) {
        Some(p) => p,
        None => {
            log::debug!("got {decoded:?}, but local pane {local_pane_id} no longer exists; resync");
            client_domain.resync().await?;

            let local_pane_id =
                client_domain
                    .remote_to_local_pane_id(pane_id)
                    .ok_or_else(|| {
                        anyhow!("remote pane id {} does not have a local pane id", pane_id)
                    })?;

            mux.get_pane(local_pane_id)
                .ok_or_else(|| anyhow!("local pane {local_pane_id} not found"))?
        }
    };
    let client_pane = pane.downcast_ref::<ClientPane>().ok_or_else(|| {
        log::error!(
            "received unilateral PDU for pane {} which is \
                     not an instance of ClientPane: {:?}",
            local_pane_id,
            decoded.pdu
        );
        anyhow!(
            "received unilateral PDU for pane {} which is \
                     not an instance of ClientPane: {:?}",
            local_pane_id,
            decoded.pdu
        )
    })?;
    client_pane.process_unilateral(decoded.pdu).await
}

pub(super) fn process_unilateral(
    local_domain_id: Option<DomainId>,
    decoded: DecodedPdu,
) -> anyhow::Result<()> {
    let local_domain_id = match local_domain_id {
        Some(id) => id,
        None => {
            // FIXME: We currently get a bunch of these; we'll need
            // to do something to advise the server when we want them.
            // For now, we just ignore them.
            log::trace!(
                "client doesn't have a real local domain, \
                 so unilateral message cannot be processed by it"
            );
            return Ok(());
        }
    };
    match &decoded.pdu {
        Pdu::WindowWorkspaceChanged(WindowWorkspaceChanged {
            window_id,
            workspace,
        }) => {
            let window_id = *window_id;
            let workspace = workspace.to_string();
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::try_get().ok_or_else(|| anyhow!("no more mux"))?;
                let client_domain = mux
                    .get_domain(local_domain_id)
                    .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
                let client_domain =
                    client_domain
                        .downcast_ref::<ClientDomain>()
                        .ok_or_else(|| {
                            anyhow!("domain {} is not a ClientDomain instance", local_domain_id)
                        })?;

                let local_window_id = client_domain
                    .remote_to_local_window_id(window_id)
                    .ok_or_else(|| anyhow!("no local window for remote window id {}", window_id))?;
                if let Some(mut window) = mux.get_window_mut(local_window_id) {
                    window.set_workspace(&workspace);
                }

                anyhow::Result::<()>::Ok(())
            })
            .detach();

            return Ok(());
        }
        Pdu::WindowTitleChanged(WindowTitleChanged { window_id, title }) => {
            let title = title.to_string();
            let window_id = *window_id;
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::try_get().ok_or_else(|| anyhow!("no more mux"))?;
                let client_domain = mux
                    .get_domain(local_domain_id)
                    .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
                let client_domain =
                    client_domain
                        .downcast_ref::<ClientDomain>()
                        .ok_or_else(|| {
                            anyhow!("domain {} is not a ClientDomain instance", local_domain_id)
                        })?;

                client_domain.process_remote_window_title_change(window_id, title);
                anyhow::Result::<()>::Ok(())
            })
            .detach();
            return Ok(());
        }
        Pdu::RenameWorkspace(RenameWorkspace {
            old_workspace,
            new_workspace,
        }) => {
            let old_workspace = old_workspace.to_string();
            let new_workspace = new_workspace.to_string();
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::try_get().ok_or_else(|| anyhow!("no more mux"))?;
                log::debug!("got a rename {old_workspace} -> {new_workspace}");
                mux.rename_workspace(&old_workspace, &new_workspace);
                anyhow::Result::<()>::Ok(())
            })
            .detach();
            return Ok(());
        }
        Pdu::TabTitleChanged(TabTitleChanged { tab_id, title }) => {
            let title = title.to_string();
            let tab_id = *tab_id;
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::try_get().ok_or_else(|| anyhow!("no more mux"))?;
                let client_domain = mux
                    .get_domain(local_domain_id)
                    .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
                let client_domain =
                    client_domain
                        .downcast_ref::<ClientDomain>()
                        .ok_or_else(|| {
                            anyhow!("domain {} is not a ClientDomain instance", local_domain_id)
                        })?;

                client_domain.process_remote_tab_title_change(tab_id, title);
                anyhow::Result::<()>::Ok(())
            })
            .detach();
            return Ok(());
        }
        Pdu::TabReflowed(_) | Pdu::TabAddedToWindow(_) => {
            log::trace!("resync due to {:?}", decoded.pdu);
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::try_get().ok_or_else(|| anyhow!("no more mux"))?;
                let client_domain = mux
                    .get_domain(local_domain_id)
                    .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
                let client_domain =
                    client_domain
                        .downcast_ref::<ClientDomain>()
                        .ok_or_else(|| {
                            anyhow!("domain {} is not a ClientDomain instance", local_domain_id)
                        })?;

                client_domain.resync().await
            })
            .detach();

            return Ok(());
        }
        _ => {}
    }

    if let Some(pane_id) = decoded.pdu.pane_id() {
        promise::spawn::spawn_into_main_thread(async move {
            process_unilateral_inner(pane_id, local_domain_id, decoded)
        })
        .detach();
    } else {
        bail!("don't know how to handle {:?}", decoded);
    }
    Ok(())
}
