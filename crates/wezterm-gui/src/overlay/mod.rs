use crate::termwindow::TermWindow;
use mux::pane::Pane;
use mux::tab::{Tab, TabId};
use mux::termwiztermtab::{allocate, TermWizTerminal};
use std::pin::Pin;
use std::sync::Arc;
use wezterm_term::TerminalConfiguration;

pub mod confirm;
pub mod copy;
pub mod debug;
pub mod launcher;
pub mod prompt;
pub mod quickselect;
pub mod selector;
pub mod version;

pub use copy::{CopyModeParams, CopyOverlay};
pub use debug::show_debug_overlay;
pub use launcher::{launcher, LauncherArgs, LauncherFlags};
pub use quickselect::QuickSelectOverlay;
pub use version::show_version_overlay;

// Async-over-sync bridge return type used in exactly one place; a type alias
// would add indirection without any reuse benefit.
#[allow(clippy::type_complexity)]
pub fn start_overlay<T, F>(
    term_window: &TermWindow,
    tab: &Arc<Tab>,
    func: F,
) -> anyhow::Result<(
    Arc<dyn Pane>,
    Pin<Box<dyn std::future::Future<Output = anyhow::Result<T>>>>,
)>
where
    T: Send + 'static,
    F: Send + 'static + FnOnce(TabId, TermWizTerminal) -> anyhow::Result<T>,
{
    let tab_id = tab.tab_id();
    let tab_size = tab.get_size();
    let term_config: Arc<dyn TerminalConfiguration + Send + Sync> =
        Arc::new(config::TermConfig::with_config(term_window.config.clone()));
    let (tw_term, tw_tab) = allocate(tab_size, term_config)?;

    let window = term_window.window.clone().unwrap();

    let overlay_pane_id = tw_tab.pane_id();

    let future = promise::spawn::spawn_into_new_thread(move || {
        let res = func(tab_id, tw_term);
        TermWindow::schedule_cancel_overlay(window, tab_id, Some(overlay_pane_id));
        res
    });

    Ok((tw_tab, Box::pin(future)))
}
