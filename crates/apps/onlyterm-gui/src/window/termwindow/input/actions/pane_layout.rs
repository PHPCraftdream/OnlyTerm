use super::split_plan::{execute_three_way, plan_three_way_split};
use super::*;
use crate::termwindow::pane_layout_menu::{can_close_pane, PaneLayoutChoice};
use onlyterm_client::pane::ClientPane;
use onlyterm_config::keyassignment::SpawnTabDomain;
use onlyterm_mux::activity::Activity;
use onlyterm_mux::domain::SplitSource;

async fn split_once(
    mux: Arc<Mux>,
    pane_id: PaneId,
    request: SplitRequest,
    term_config: Arc<TermConfig>,
) -> anyhow::Result<PaneId> {
    let (pane, _) = mux
        .split_pane(
            pane_id,
            request,
            SplitSource::Spawn {
                command: None,
                command_dir: None,
            },
            SpawnTabDomain::CurrentPaneDomain,
        )
        .await?;
    pane.set_config(term_config);
    Ok(pane.pane_id())
}

async fn rollback_pane(mux: Arc<Mux>, pane_id: PaneId) -> anyhow::Result<()> {
    let Some(pane) = mux.get_pane(pane_id) else {
        return Ok(());
    };
    if let Some(client_pane) = pane.downcast_ref::<ClientPane>() {
        client_pane.kill_remote_and_wait().await?;
        client_pane.ignore_next_kill();
    }
    mux.remove_pane(pane_id);
    Ok(())
}

impl TermWindow {
    pub(in crate::termwindow) fn perform_pane_layout_choice(&mut self, choice: PaneLayoutChoice) {
        if choice == PaneLayoutChoice::CloseMenu {
            self.cancel_modal();
            return;
        }

        let mux = Mux::get();
        let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
            self.cancel_modal();
            return;
        };
        if choice == PaneLayoutChoice::ClosePane {
            if can_close_pane(tab.iter_panes_ignoring_zoom().len()) {
                self.cancel_modal();
                self.close_current_pane(false);
            }
            return;
        }

        let Some(active_pane) = tab.get_active_pane() else {
            return;
        };
        let pane_id = active_pane.pane_id();
        let Some(position) = tab
            .iter_panes_ignoring_zoom()
            .into_iter()
            .find(|position| position.pane.pane_id() == pane_id)
        else {
            return;
        };
        let (direction, three_way) = match choice {
            PaneLayoutChoice::SplitHorizontal => (SplitDirection::Horizontal, false),
            PaneLayoutChoice::SplitVertical => (SplitDirection::Vertical, false),
            PaneLayoutChoice::SplitThreeHorizontal => (SplitDirection::Horizontal, true),
            PaneLayoutChoice::SplitThreeVertical => (SplitDirection::Vertical, true),
            PaneLayoutChoice::ClosePane | PaneLayoutChoice::CloseMenu => return,
        };
        let total = match direction {
            SplitDirection::Horizontal => position.width,
            SplitDirection::Vertical => position.height,
        };
        let requests = if three_way {
            plan_three_way_split(total, direction)
        } else if total >= 3 {
            Some([
                SplitRequest {
                    direction,
                    target_is_second: true,
                    top_level: false,
                    size: MuxSplitSize::Percent(50),
                },
                SplitRequest::default(),
            ])
        } else {
            None
        };
        let Some(requests) = requests else {
            onlyterm_toast_notification::persistent_toast_notification(
                "OnlyTerm",
                "Pane layout: not enough cells to split this pane",
            );
            return;
        };

        let tab_id = tab.tab_id();
        let term_config = Arc::new(TermConfig::with_config(self.config.clone()));
        let activity = Activity::new();
        self.cancel_modal();
        onlyterm_promise::spawn::spawn(async move {
            let _activity = activity;
            let result = if three_way {
                let split_mux = Arc::clone(&mux);
                let rollback_mux = Arc::clone(&mux);
                let split_config = Arc::clone(&term_config);
                execute_three_way(
                    pane_id,
                    requests,
                    move |target, request| {
                        split_once(
                            Arc::clone(&split_mux),
                            target,
                            request,
                            Arc::clone(&split_config),
                        )
                    },
                    move |first_new| rollback_pane(Arc::clone(&rollback_mux), first_new),
                )
                .await
            } else {
                split_once(
                    Arc::clone(&mux),
                    pane_id,
                    requests[0],
                    Arc::clone(&term_config),
                )
                .await
            };
            match result {
                Ok(new_pane_id) => {
                    if mux.resolve_pane_id(new_pane_id).map(|(_, _, id)| id) == Some(tab_id) {
                        if let (Some(tab), Some(pane)) =
                            (mux.get_tab(tab_id), mux.get_pane(new_pane_id))
                        {
                            tab.set_active_pane(&pane);
                        }
                    }
                }
                Err(error) => {
                    let message = format!("Pane layout: {error:#}");
                    log::error!("{}", message);
                    onlyterm_toast_notification::persistent_toast_notification(
                        "OnlyTerm", &message,
                    );
                }
            }
        })
        .detach();
    }
}
