use super::ClientDomain;
use crate::pane::ClientPane;
use ::onlyterm_config::keyassignment::SpawnTabDomain;
use anyhow::anyhow;
use async_trait::async_trait;
use onlyterm_codec::{SpawnV2, SplitPane};
use onlyterm_mux::connui::{ConnectionUI, ConnectionUIParams};
use onlyterm_mux::domain::{Domain, DomainId, DomainState, SplitSource};
use onlyterm_mux::pane::{Pane, PaneId};
use onlyterm_mux::tab::{SplitRequest, Tab, TabId};
use onlyterm_mux::window::WindowId;
use onlyterm_mux::Mux;
use onlyterm_term::TerminalSize;
use portable_pty::CommandBuilder;
use std::future::Future;
use std::sync::Arc;

fn split_command_dir(
    elevated_host: bool,
    has_explicit_command: bool,
    command_dir: Option<String>,
) -> Option<String> {
    if elevated_host && !has_explicit_command {
        None
    } else {
        command_dir
    }
}

fn split_response_matches_target(
    source_tab_id: TabId,
    source_pane_id: PaneId,
    result_tab_id: TabId,
    result_pane_id: PaneId,
) -> bool {
    source_tab_id == result_tab_id && source_pane_id != result_pane_id
}

fn split_response_valid_for_source(
    source_tab_id: TabId,
    source_pane_id: PaneId,
    result_tab_id: TabId,
    result_pane_id: PaneId,
    spawned_here: bool,
    already_known: bool,
) -> bool {
    if source_tab_id != result_tab_id {
        return false;
    }
    if spawned_here {
        split_response_matches_target(source_tab_id, source_pane_id, result_tab_id, result_pane_id)
            && !already_known
    } else {
        true
    }
}

async fn rollback_split<K, KF, R, RF>(
    kill: K,
    resync: R,
) -> (anyhow::Result<()>, anyhow::Result<()>)
where
    K: FnOnce() -> KF,
    KF: Future<Output = anyhow::Result<()>>,
    R: FnOnce() -> RF,
    RF: Future<Output = anyhow::Result<()>>,
{
    let kill_result = kill().await;
    let resync_result = resync().await;
    (kill_result, resync_result)
}

impl ClientDomain {
    fn still_attached_to(&self, inner: &Arc<super::ClientInner>) -> bool {
        self.inner()
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, inner))
    }

    /// cancel-safe: no; the caller must finish compensation after a remote split.
    async fn rollback_remote_split(
        &self,
        inner: &Arc<super::ClientInner>,
        remote_pane_id: PaneId,
        spawned_here: bool,
        cause: anyhow::Error,
    ) -> anyhow::Error {
        let (kill_result, resync_result) = rollback_split(
            || async {
                if spawned_here {
                    inner
                        .client
                        .kill_pane(onlyterm_codec::KillPane {
                            pane_id: remote_pane_id,
                        })
                        .await?;
                }
                Ok(())
            },
            || async {
                if !self.still_attached_to(inner) {
                    anyhow::bail!("domain detached before split resync");
                }
                self.resync().await?;
                if !self.still_attached_to(inner) {
                    anyhow::bail!("domain detached during split resync");
                }
                Ok(())
            },
        )
        .await;

        if resync_result.is_err() && self.still_attached_to(inner) {
            self.perform_detach();
        }
        let mut error = cause;
        if let Err(kill_error) = kill_result {
            error = error.context(format!(
                "remote pane {} cleanup failed: {kill_error:#}",
                remote_pane_id
            ));
        }
        if let Err(resync_error) = resync_result {
            error = error.context(format!("split resync failed: {resync_error:#}"));
        }
        error
    }
}

#[async_trait(?Send)]
impl Domain for ClientDomain {
    fn domain_id(&self) -> DomainId {
        self.local_domain_id
    }

    fn domain_name(&self) -> &str {
        self.config.name()
    }

    async fn domain_label(&self) -> String {
        self.label.to_string()
    }

    async fn spawn_pane(
        &self,
        _size: TerminalSize,
        _command: Option<CommandBuilder>,
        _command_dir: Option<String>,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        anyhow::bail!("spawn_pane not implemented for ClientDomain")
    }

    /// Forward the request to the remote; we need to translate the local ids
    /// to those that match the remote for the request, resync the changed
    /// structure, and then translate the results back to local
    async fn remote_move_pane_to_new_tab(
        &self,
        pane_id: PaneId,
        window_id: Option<WindowId>,
        workspace_for_new_window: Option<String>,
    ) -> anyhow::Result<Option<(Arc<Tab>, WindowId)>> {
        let inner = self
            .inner()
            .ok_or_else(|| anyhow!("domain is not attached"))?;

        let local_pane = Mux::get()
            .get_pane(pane_id)
            .ok_or_else(|| anyhow!("pane_id {} is invalid", pane_id))?;
        let pane = local_pane
            .downcast_ref::<ClientPane>()
            .ok_or_else(|| anyhow!("pane_id {} is not a ClientPane", pane_id))?;

        let remote_window_id =
            window_id.and_then(|local_window| self.local_to_remote_window_id(local_window));

        let result = inner
            .client
            .move_pane_to_new_tab(onlyterm_codec::MovePaneToNewTab {
                pane_id: pane.remote_pane_id,
                window_id: remote_window_id,
                workspace_for_new_window,
            })
            .await?;

        self.resync().await?;

        let local_tab_id = inner
            .remote_to_local_tab_id(result.tab_id)
            .ok_or_else(|| anyhow!("remote tab {} didn't resolve after resync", result.tab_id))?;

        let local_win_id = self
            .remote_to_local_window_id(result.window_id)
            .ok_or_else(|| {
                anyhow!(
                    "remote window {} didn't resolve after resync",
                    result.window_id
                )
            })?;

        let tab = Mux::get()
            .get_tab(local_tab_id)
            .ok_or_else(|| anyhow!("local tab {local_tab_id} is invalid"))?;

        Ok(Some((tab, local_win_id)))
    }

    async fn remote_rotate_panes(
        &self,
        pane_id: PaneId,
        direction: ::onlyterm_config::keyassignment::RotationDirection,
    ) -> anyhow::Result<bool> {
        let inner = self
            .inner()
            .ok_or_else(|| anyhow!("domain is not attached"))?;

        let local_pane = Mux::get()
            .get_pane(pane_id)
            .ok_or_else(|| anyhow!("pane_id {} is invalid", pane_id))?;
        let pane = local_pane
            .downcast_ref::<ClientPane>()
            .ok_or_else(|| anyhow!("pane_id {} is not a ClientPane", pane_id))?;

        inner
            .client
            .rotate_panes(onlyterm_codec::RotatePanes {
                pane_id: pane.remote_pane_id,
                direction,
            })
            .await?;

        self.resync().await?;
        Ok(true)
    }

    async fn remote_swap_active_pane_with_index(
        &self,
        active_pane_id: PaneId,
        with_pane_index: usize,
        keep_focus: bool,
    ) -> anyhow::Result<bool> {
        let inner = self
            .inner()
            .ok_or_else(|| anyhow!("domain is not attached"))?;

        let local_pane = Mux::get()
            .get_pane(active_pane_id)
            .ok_or_else(|| anyhow!("pane_id {} is invalid", active_pane_id))?;
        let pane = local_pane
            .downcast_ref::<ClientPane>()
            .ok_or_else(|| anyhow!("pane_id {} is not a ClientPane", active_pane_id))?;

        inner
            .client
            .swap_active_pane_with_index(onlyterm_codec::SwapActivePaneWithIndex {
                active_pane_id: pane.remote_pane_id,
                with_pane_index,
                keep_focus,
            })
            .await?;

        self.resync().await?;
        Ok(true)
    }

    async fn spawn(
        &self,
        size: TerminalSize,
        command: Option<CommandBuilder>,
        command_dir: Option<String>,
        window: WindowId,
    ) -> anyhow::Result<Arc<Tab>> {
        let inner = self
            .inner()
            .ok_or_else(|| anyhow!("domain is not attached"))?;

        let workspace = Mux::get().active_workspace();

        let result = inner
            .client
            .spawn_v2(SpawnV2 {
                domain: SpawnTabDomain::DefaultDomain,
                window_id: inner.local_to_remote_window(window),
                size,
                command,
                command_dir,
                workspace,
                attach: false,
            })
            .await?;

        inner.record_remote_to_local_window_mapping(result.window_id, window);

        let pane: Arc<dyn Pane> = Arc::new(ClientPane::new(
            &inner,
            result.tab_id,
            result.pane_id,
            size,
            "onlyterm",
        ));
        let tab = Arc::new(Tab::new(&size));
        tab.assign_pane(&pane);
        inner.remove_old_tab_mapping(result.tab_id);
        inner.record_remote_to_local_tab_mapping(result.tab_id, tab.tab_id());

        let mux = Mux::get();
        mux.add_tab_and_active_pane(&tab)?;
        mux.add_tab_to_window(&tab, window)?;

        Ok(tab)
    }

    async fn split_pane(
        &self,
        source: SplitSource,
        tab_id: TabId,
        pane_id: PaneId,
        split_request: SplitRequest,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        let inner = self
            .inner()
            .ok_or_else(|| anyhow!("domain is not attached"))?;

        let mux = Mux::get();

        let tab = mux
            .get_tab(tab_id)
            .ok_or_else(|| anyhow!("tab_id {} is invalid", tab_id))?;
        let local_pane = mux
            .get_pane(pane_id)
            .ok_or_else(|| anyhow!("pane_id {} is invalid", pane_id))?;
        let pane = local_pane
            .downcast_ref::<ClientPane>()
            .ok_or_else(|| anyhow!("pane_id {} is not a ClientPane", pane_id))?;

        let (command, command_dir, move_pane_id) = match source {
            SplitSource::Spawn {
                command,
                command_dir,
            } => (command, command_dir, None),
            SplitSource::MovePane(move_pane_id) => (None, None, Some(move_pane_id)),
        };
        let spawned_here = move_pane_id.is_none();
        let command_dir = split_command_dir(self.elevated_host, command.is_some(), command_dir);

        let result = inner
            .client
            .split_pane(SplitPane {
                domain: SpawnTabDomain::CurrentPaneDomain,
                pane_id: pane.remote_pane_id,
                split_request,
                command,
                command_dir,
                move_pane_id,
            })
            .await?;

        if !split_response_valid_for_source(
            pane.remote_tab_id,
            pane.remote_pane_id,
            result.tab_id,
            result.pane_id,
            spawned_here,
            inner.remote_to_local_pane_id(result.pane_id).is_some(),
        ) {
            let error = anyhow!(
                "remote split returned pane {} in tab {} for pane {} in tab {}",
                result.pane_id,
                result.tab_id,
                pane.remote_pane_id,
                pane.remote_tab_id
            );
            if !self.still_attached_to(&inner) {
                return Err(error.context("domain detached before split resync"));
            }
            if let Err(sync_error) = self.resync().await {
                if self.still_attached_to(&inner) {
                    self.perform_detach();
                }
                return Err(error.context(format!("resync failed: {sync_error:#}")));
            }
            if !self.still_attached_to(&inner) {
                return Err(error.context("domain detached during split resync"));
            }
            return Err(error);
        }

        let remote_new_pane_id = result.pane_id;
        let tab_is_current = mux
            .get_tab(tab_id)
            .is_some_and(|current| Arc::ptr_eq(&current, &tab));
        let source_is_current = mux.resolve_pane_id(pane_id).map(|(_, _, id)| id) == Some(tab_id);
        let pane_index = tab
            .iter_panes_ignoring_zoom()
            .iter()
            .find(|position| position.pane.pane_id() == pane_id)
            .map(|position| position.index);
        let pane_index = match (tab_is_current, source_is_current, pane_index) {
            (true, true, Some(index)) => index,
            _ => {
                let error = anyhow!(
                    "pane {} disappeared while splitting tab {}",
                    pane_id,
                    tab_id
                );
                return Err(self
                    .rollback_remote_split(&inner, remote_new_pane_id, spawned_here, error)
                    .await);
            }
        };
        if result.size.cols == 0 || result.size.rows == 0 {
            let error = anyhow!("remote split returned a zero-sized pane");
            return Err(self
                .rollback_remote_split(&inner, remote_new_pane_id, spawned_here, error)
                .await);
        }

        let new_pane = Arc::new(ClientPane::new(
            &inner,
            result.tab_id,
            result.pane_id,
            result.size,
            "onlyterm",
        ));
        let pane: Arc<dyn Pane> = new_pane.clone();
        if let Err(error) = tab.split_and_insert(pane_index, split_request, Arc::clone(&pane)) {
            return Err(self
                .rollback_remote_split(&inner, remote_new_pane_id, spawned_here, error)
                .await);
        }

        if let Err(error) = mux.add_pane(&pane) {
            tab.remove_pane(new_pane.pane_id());
            if mux.get_pane(new_pane.pane_id()).is_some() {
                new_pane.ignore_next_kill();
                mux.remove_pane(new_pane.pane_id());
            }
            return Err(self
                .rollback_remote_split(&inner, remote_new_pane_id, spawned_here, error)
                .await);
        }

        Ok(pane)
    }

    async fn attach(&self, window_id: Option<WindowId>) -> anyhow::Result<()> {
        let ui = ConnectionUI::with_params(ConnectionUIParams {
            window_id,
            ..Default::default()
        });
        ui.title("onlyterm: Connecting...");
        self.attach_impl(window_id, ui, true).await
    }

    fn detachable(&self) -> bool {
        true
    }

    fn detach(&self) -> anyhow::Result<()> {
        self.perform_detach();
        Ok(())
    }

    fn state(&self) -> DomainState {
        if self.inner.lock().unwrap().is_some() {
            DomainState::Attached
        } else {
            DomainState::Detached
        }
    }

    fn spawnable(&self) -> bool {
        self.spawnable
    }
}

#[cfg(test)]
mod tests {
    use super::{
        rollback_split, split_command_dir, split_response_matches_target,
        split_response_valid_for_source,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn elevated_default_split_does_not_forward_client_resolved_cwd() {
        assert_eq!(
            split_command_dir(true, false, Some("client supplied cwd".to_string())),
            None
        );
    }

    #[test]
    fn ordinary_and_explicit_splits_keep_their_requested_cwd() {
        let cwd = Some("requested cwd".to_string());
        assert_eq!(split_command_dir(false, false, cwd.clone()), cwd);
        assert_eq!(split_command_dir(true, true, cwd.clone()), cwd);
    }

    #[test]
    fn remote_split_response_must_stay_in_the_source_tab_and_create_a_new_pane() {
        assert!(split_response_matches_target(7, 42, 7, 43));
        assert!(!split_response_matches_target(7, 42, 8, 43));
        assert!(!split_response_matches_target(7, 42, 7, 42));
    }

    #[test]
    fn moving_a_known_pane_still_rejects_a_foreign_result_tab() {
        assert!(split_response_valid_for_source(7, 42, 7, 42, false, true));
        assert!(!split_response_valid_for_source(7, 42, 8, 42, false, true));
        assert!(split_response_valid_for_source(7, 42, 7, 43, true, false));
        assert!(!split_response_valid_for_source(7, 42, 7, 43, true, true));
    }

    #[test]
    fn rollback_resyncs_even_when_remote_kill_fails() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let kill_calls = Rc::clone(&calls);
        let resync_calls = Rc::clone(&calls);
        let (kill, resync) = onlyterm_promise::spawn::block_on(rollback_split(
            move || async move {
                kill_calls.borrow_mut().push("kill");
                anyhow::bail!("remote kill failed")
            },
            move || async move {
                resync_calls.borrow_mut().push("resync");
                Ok(())
            },
        ));
        assert!(kill.is_err());
        assert!(resync.is_ok());
        assert_eq!(*calls.borrow(), ["kill", "resync"]);
    }
}
