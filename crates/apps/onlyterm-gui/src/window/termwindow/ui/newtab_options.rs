use crate::spawn::SpawnWhere;
use crate::termwindow::box_model::*;
use crate::termwindow::menu_style::MenuStyle;
use crate::termwindow::modal::Modal;
use crate::termwindow::{DimensionContext, NewTabOptionGroup, TermWindow, UIItemType};
use crate::utilsprites::RenderMetrics;
use ::window::{Connection, ConnectionOps};
use onlyterm_config::keyassignment::{KeyAssignment, SpawnCommand, SpawnTabDomain};
use onlyterm_config::{Dimension, ProcessPriority};
use onlyterm_mux::activity::Activity;
use onlyterm_term::{KeyCode, KeyModifiers, MouseEvent};
use std::cell::{Ref, RefCell};
use std::path::PathBuf;
use std::sync::Arc;

/// Re-exported from the config crate so the dialog and `--start-conf`
/// layouts cannot disagree about what argv each shell name means.
use onlyterm_config::shell::{available_shells, AvailableShell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Elevation {
    Normal,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Priority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

/// What happens when the dialog is dismissed (Esc or the close cross).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnCancel {
    /// Ordinary invocation: dismiss and go back to the tab underneath.
    Dismiss,
    /// Startup invocation: there is no tab underneath, so dismissing the
    /// dialog means dismissing the application.
    QuitApplication,
}

impl Elevation {
    const ALL: [Elevation; 2] = [Elevation::Normal, Elevation::Admin];

    fn name(&self) -> &'static str {
        match self {
            Elevation::Normal => "normal",
            Elevation::Admin => "admin",
        }
    }
}

impl Priority {
    const ALL: [Priority; 6] = [
        Priority::Idle,
        Priority::BelowNormal,
        Priority::Normal,
        Priority::AboveNormal,
        Priority::High,
        Priority::Realtime,
    ];

    fn name(&self) -> &'static str {
        match self {
            Priority::Idle => "Idle",
            Priority::BelowNormal => "Below Normal",
            Priority::Normal => "Normal",
            Priority::AboveNormal => "Above Normal",
            Priority::High => "High",
            Priority::Realtime => "Realtime",
        }
    }

    fn to_process_priority(self) -> ProcessPriority {
        match self {
            Priority::Idle => ProcessPriority::Idle,
            Priority::BelowNormal => ProcessPriority::BelowNormal,
            Priority::Normal => ProcessPriority::Normal,
            Priority::AboveNormal => ProcessPriority::AboveNormal,
            Priority::High => ProcessPriority::High,
            Priority::Realtime => ProcessPriority::Realtime,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusItem {
    Shell(usize),
    Elevation(usize),
    Priority(usize),
    Run,
}

pub struct NewTabOptions {
    element: RefCell<Option<Vec<ComputedElement>>>,
    /// The shells this machine actually has, detected once when the dialog
    /// is opened rather than per frame: `compute` re-runs on every focus
    /// move, and detection touches the filesystem and the registry.
    /// Re-detecting per open (rather than once per process) is what lets a
    /// Git for Windows installed mid-session show up without a restart.
    shells: Vec<AvailableShell>,
    /// An index into `shells`, not a `Shell`: which shells exist varies by
    /// machine, so there is no fixed table for a `Shell` to be an index
    /// into. `available_shells` guarantees at least one entry, so index 0 is
    /// always valid.
    selected_shell: RefCell<usize>,
    selected_elevation: RefCell<Elevation>,
    selected_priority: RefCell<Priority>,
    focus: RefCell<FocusItem>,
    /// What happens when the dialog is dismissed (Esc or the close cross).
    on_cancel: OnCancel,
    /// The Activity that keeps the mux alive while the dialog is open.
    /// Only used in startup mode; in ordinary invocation this is None.
    /// RefCell allows run() to take ownership when Run is pressed.
    _activity: RefCell<Option<Activity>>,
    /// Where the spawned tab should start. Set only in startup mode, from the
    /// launch's `--cwd` -- an ordinary invocation has a pane underneath, whose
    /// directory the normal spawn path already inherits.
    cwd: Option<PathBuf>,
}

impl NewTabOptions {
    /// Takes no `&TermWindow`, unlike its sibling `CommandPalette::new(self)`:
    /// it never used one, and without it the defaults below are testable.
    pub fn new() -> Self {
        Self::build(OnCancel::Dismiss, None, None)
    }

    /// The startup (`--choose-tab`) form: dismissing ends the process, the
    /// `Activity` keeps the tabless mux alive, and `cwd` is where the tab the
    /// user asks for will start.
    pub fn new_with_on_cancel(
        on_cancel: OnCancel,
        activity: Activity,
        cwd: Option<PathBuf>,
    ) -> Self {
        Self::build(on_cancel, Some(activity), cwd)
    }

    fn build(on_cancel: OnCancel, activity: Option<Activity>, cwd: Option<PathBuf>) -> Self {
        let shells = available_shells();
        log::debug!(
            "New Tab Options: offering shells {:?}, cwd {:?}",
            shells
                .iter()
                .map(|s| (s.shell.name(), s.path.display().to_string()))
                .collect::<Vec<_>>(),
            cwd,
        );
        Self {
            element: RefCell::new(None),
            shells,
            // Index 0 is `cmd` on any machine that has it, since detection
            // preserves `Shell::ALL` order and that list leads with cmd --
            // which is the documented default for this dialog.
            selected_shell: RefCell::new(0),
            selected_elevation: RefCell::new(Elevation::Normal),
            selected_priority: RefCell::new(Priority::Normal),
            focus: RefCell::new(FocusItem::Shell(0)),
            on_cancel,
            _activity: RefCell::new(activity),
            cwd,
        }
    }

    pub fn on_cancel(&self) -> OnCancel {
        self.on_cancel
    }

    fn dismiss(&self, term_window: &TermWindow) {
        perform_dismiss(self.on_cancel, term_window);
    }
}

/// The single place that decides what dismissing this dialog means. Both
/// routes out of the dialog go through here so they cannot drift apart --
/// which matters more than usual now that one of the two outcomes is
/// "terminate the process".
///
/// A free function taking the choice by value, rather than a method, because
/// the mouse path reaches the dialog through `TermWindow::modal`, and
/// `cancel_modal` takes `borrow_mut()` on that same `RefCell`. Holding a
/// `Ref` across the call is an immediate `BorrowMutError`, so that caller has
/// to read the choice, drop its borrow, and only then dispatch -- at which
/// point it no longer has the dialog to call a method on.
pub(crate) fn perform_dismiss(on_cancel: OnCancel, term_window: &TermWindow) {
    match on_cancel {
        OnCancel::QuitApplication => {
            let con = Connection::get().expect("call on gui thread");
            con.terminate_message_loop();
        }
        OnCancel::Dismiss => term_window.cancel_modal(),
    }
}

impl NewTabOptions {
    fn total_focus_items(&self) -> usize {
        self.shells.len() + Elevation::ALL.len() + Priority::ALL.len() + 1
    }

    fn focus_item_to_index(&self, item: FocusItem) -> usize {
        let shell_len = self.shells.len();
        match item {
            FocusItem::Shell(idx) => idx,
            FocusItem::Elevation(idx) => shell_len + idx,
            FocusItem::Priority(idx) => shell_len + Elevation::ALL.len() + idx,
            FocusItem::Run => shell_len + Elevation::ALL.len() + Priority::ALL.len(),
        }
    }

    fn index_to_focus_item(&self, index: usize) -> FocusItem {
        let shell_len = self.shells.len();
        let elevation_len = Elevation::ALL.len();
        let priority_len = Priority::ALL.len();

        if index < shell_len {
            FocusItem::Shell(index)
        } else if index < shell_len + elevation_len {
            FocusItem::Elevation(index - shell_len)
        } else if index < shell_len + elevation_len + priority_len {
            FocusItem::Priority(index - shell_len - elevation_len)
        } else {
            FocusItem::Run
        }
    }

    fn move_focus(&self, direction: isize) {
        let current_idx = self.focus_item_to_index(*self.focus.borrow());
        let total = self.total_focus_items();
        let new_idx = if direction >= 0 {
            (current_idx as isize + direction).rem_euclid(total as isize) as usize
        } else {
            (current_idx as isize + total as isize + direction).rem_euclid(total as isize) as usize
        };
        *self.focus.borrow_mut() = self.index_to_focus_item(new_idx);
    }

    /// Selects whatever is currently focused. Only correct for the
    /// keyboard path (Space/Enter), where "currently focused" and "the
    /// thing the user means to activate" are the same thing by
    /// construction. Returns a run request when the focused item was
    /// Run; the mouse path must NOT use this for the Run button (a
    /// click on Run says nothing about what was last Tab-focused), so
    /// it calls `run()` directly instead.
    fn select_focused_impl(&self) -> Option<NewTabRunRequest> {
        let current = *self.focus.borrow();
        match current {
            FocusItem::Shell(idx) => {
                *self.selected_shell.borrow_mut() = idx;
                None
            }
            FocusItem::Elevation(idx) => {
                *self.selected_elevation.borrow_mut() = Elevation::ALL[idx];
                None
            }
            FocusItem::Priority(idx) => {
                *self.selected_priority.borrow_mut() = Priority::ALL[idx];
                None
            }
            FocusItem::Run => Some(self.run()),
        }
    }

    fn compute(
        term_window: &mut TermWindow,
        shells: &[AvailableShell],
        selected_shell: usize,
        selected_elevation: Elevation,
        selected_priority: Priority,
        focus: FocusItem,
    ) -> anyhow::Result<Vec<ComputedElement>> {
        let font = term_window.fonts.command_palette_font()?;
        let heading = term_window.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let dimensions = &term_window.dimensions;
        let style = MenuStyle::new(
            term_window.config.command_palette_bg_color,
            term_window.config.command_palette_fg_color,
        );
        let width_limit = dimensions.pixel_width as f32;
        let height_limit = dimensions.pixel_height as f32;
        let width = (width_limit - 32.).clamp(1., 1000.);
        let inner_width = (width - 34.).max(1.);
        let choice = |label: &str, selected: bool, focused: bool, group, index| {
            let mut element = style
                .button(
                    &font,
                    &format!("{} {label}", if selected { "●" } else { "○" }),
                    focused,
                    true,
                )
                .item_type(UIItemType::NewTabOptionRadio {
                    group,
                    choice: index,
                })
                .margin(BoxDimension {
                    right: Dimension::Pixels(8.),
                    bottom: Dimension::Pixels(6.),
                    ..BoxDimension::default()
                });
            if selected && !focused {
                element.colors.bg = style.chip.into();
            }
            element
        };
        let group = |label: &str, choices: Vec<Element>, per_row: usize| {
            let mut rows = vec![style.text(&heading, label.into(), false)];
            let mut choices = choices.into_iter();
            loop {
                let row: Vec<_> = choices.by_ref().take(per_row).collect();
                if row.is_empty() {
                    break;
                }
                rows.push(
                    Element::new(&font, ElementContent::Children(row)).display(DisplayType::Block),
                );
            }
            style.card(&font, rows, inner_width)
        };
        let shell_items = shells
            .iter()
            .enumerate()
            .map(|(index, shell)| {
                choice(
                    shell.shell.name(),
                    selected_shell == index,
                    focus == FocusItem::Shell(index),
                    NewTabOptionGroup::Shell,
                    index,
                )
            })
            .collect();
        let elevation_items = Elevation::ALL
            .iter()
            .enumerate()
            .map(|(index, elevation)| {
                choice(
                    elevation.name(),
                    selected_elevation == *elevation,
                    focus == FocusItem::Elevation(index),
                    NewTabOptionGroup::Elevation,
                    index,
                )
            })
            .collect();
        let priority_items = Priority::ALL
            .iter()
            .enumerate()
            .map(|(index, priority)| {
                choice(
                    priority.name(),
                    selected_priority == *priority,
                    focus == FocusItem::Priority(index),
                    NewTabOptionGroup::Priority,
                    index,
                )
            })
            .collect();
        let mut close = style
            .button(&font, "×", false, true)
            .float(Float::Right)
            .min_width(Some(Dimension::Pixels(metrics.cell_size.width as f32)))
            .item_type(UIItemType::NewTabOptionClose);
        close.margin = BoxDimension::default();
        let close_width = metrics.cell_size.width as f32 + 26.;
        let header = Element::new(
            &font,
            ElementContent::Children(vec![
                style
                    .text(&font, "New Tab Options".into(), false)
                    .display(DisplayType::Inline),
                close,
            ]),
        )
        .display(DisplayType::Block)
        .min_width(Some(Dimension::Pixels((inner_width - close_width).max(1.))));
        let run = style
            .button(&font, "Run", focus == FocusItem::Run, true)
            .item_type(UIItemType::NewTabOptionRun);
        let root = style.panel(
            &font,
            vec![
                header,
                style.text(&font, "Choose how the new tab starts".into(), true),
                group("SHELL", shell_items, if inner_width > 650. { 4 } else { 2 }),
                group("ELEVATION", elevation_items, 2),
                group(
                    "PROCESS PRIORITY",
                    priority_items,
                    if inner_width > 650. { 3 } else { 2 },
                ),
                Element::new(&font, ElementContent::Children(vec![run]))
                    .display(DisplayType::Block),
                style.text(
                    &font,
                    "Arrows / Tab: select · Enter: activate · Esc / ×: cancel".into(),
                    true,
                ),
            ],
            width,
        );
        let mut computed = term_window.compute_element(
            &LayoutContext {
                height: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: height_limit,
                    pixel_cell: metrics.cell_size.height as f32,
                },
                width: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: width_limit,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds: euclid::rect((width_limit - width) / 2., 0., width, height_limit),
                metrics: &metrics,
                gl_state: term_window.render_state.as_ref().unwrap(),
                zindex: 100,
            },
            &root,
        )?;
        computed.translate(euclid::vec2(
            0.,
            ((height_limit - computed.bounds.height()) / 2.).max(0.),
        ));
        Ok(vec![computed])
    }

    pub fn handle_selection(&self, group: NewTabOptionGroup, choice: usize) {
        match group {
            NewTabOptionGroup::Shell => {
                if choice < self.shells.len() {
                    *self.selected_shell.borrow_mut() = choice;
                    *self.focus.borrow_mut() = FocusItem::Shell(choice);
                }
            }
            NewTabOptionGroup::Elevation => {
                if let Some(elevation) = Elevation::ALL.get(choice) {
                    *self.selected_elevation.borrow_mut() = *elevation;
                    *self.focus.borrow_mut() = FocusItem::Elevation(choice);
                }
            }
            NewTabOptionGroup::Priority => {
                if let Some(priority) = Priority::ALL.get(choice) {
                    *self.selected_priority.borrow_mut() = *priority;
                    *self.focus.borrow_mut() = FocusItem::Priority(choice);
                }
            }
        }
    }

    /// Builds a run request from whatever is currently selected,
    /// independent of keyboard focus -- this is what a mouse click on
    /// the Run button must call, since a click carries no information
    /// about where Tab focus last was (the keyboard path goes through
    /// `select_focused_impl` instead, since there "focused" and "the
    /// thing to run" coincide). Returning an owned request rather than
    /// acting immediately lets both call sites finish with `self`
    /// (dropping any `RefCell`/`Rc` borrow on the modal) before the
    /// actual spawn -- which needs `&mut TermWindow` -- happens.
    pub fn run(&self) -> NewTabRunRequest {
        let idx = *self.selected_shell.borrow();
        // `available_shells` never returns an empty list and every path that
        // writes `selected_shell` bounds-checks against it, so this holds --
        // but falling back to the first entry keeps a future off-by-one from
        // being a panic in the middle of a modal.
        let shell = self.shells.get(idx).or_else(|| self.shells.first());
        NewTabRunRequest {
            argv: shell.map(|s| s.shell.argv()).unwrap_or_default(),
            elevated: matches!(*self.selected_elevation.borrow(), Elevation::Admin),
            priority: self.selected_priority.borrow().to_process_priority(),
            chooser_activity: self._activity.borrow_mut().take(),
            cwd: self.cwd.clone(),
        }
    }
}

/// The three dialog selections, captured as plain owned data so they can
/// outlive the `NewTabOptions` modal's `RefCell` borrows -- see `run`.
pub struct NewTabRunRequest {
    argv: Vec<String>,
    elevated: bool,
    priority: ProcessPriority,
    /// The Activity that keeps the mux alive while the chooser dialog is open.
    /// Must be held alive until the spawned tab exists in the mux to prevent
    /// prune_dead_windows from terminating the app during spawn.
    chooser_activity: Option<Activity>,
    /// Startup mode only: the `--cwd` the launch was given, e.g. the folder
    /// that was right-clicked for "OnlyTerm Run As".
    cwd: Option<PathBuf>,
}

pub fn execute_new_tab_run_request(term_window: &mut TermWindow, request: NewTabRunRequest) {
    let NewTabRunRequest {
        argv,
        elevated,
        priority,
        chooser_activity,
        cwd,
    } = request;

    let src_window_id = term_window.mux_window_id;

    if !elevated {
        // Hold the chooser Activity across the `spawn_command` call, so that
        // the request to spawn is queued before this Activity's release is.
        //
        // `spawn_command` does not spawn anything synchronously -- it queues a
        // detached task, and that task creates its own Activity on its first
        // poll (spawn.rs, `spawn_command_internal`). Dropping ours releases the
        // last Activity and queues `prune_dead_windows`, which would delete
        // this still-tabless window and end the process. What saves us is
        // ordering, not timing: `onlyterm_promise::spawn::spawn` and
        // `spawn_into_main_thread` both schedule through
        // `schedule_runnable(_, true)`, i.e. the one high-priority main-thread
        // queue, so the spawn task is polled -- and takes its own Activity --
        // before the prune runs.
        //
        // That makes this correct but load-bearing on a detail two crates
        // away: moving the spawn to `spawn_with_low_priority` (which exists,
        // in that same module) would invert the order and silently bring back
        // "pressing Run quits the application".
        let _guard = chooser_activity;
        term_window.spawn_command(
            &SpawnCommand {
                args: Some(argv),
                priority: Some(priority),
                // Startup mode only; `None` leaves the usual inheritance in
                // place for a dialog opened over an existing pane.
                cwd,
                domain: SpawnTabDomain::CurrentPaneDomain,
                ..Default::default()
            },
            SpawnWhere::NewTab,
        );
        return;
    }

    // For elevated tabs, we use the new WebSocket rendezvous path that opens
    // a tab inside the existing window. The blocking UAC prompt and handshake
    // are handled inside `spawn_elevated_single_pane_tab`, which internally
    // offloads to `spawn_into_new_thread`. We still wrap the whole thing in
    // `onlyterm_promise::spawn::spawn` to keep this function non-blocking from the
    // caller's perspective (matching the old `spawn_elevated_window` pattern).
    let term_config = Arc::new(onlyterm_config::TermConfig::with_config(
        term_window.config.clone(),
    ));

    onlyterm_promise::spawn::spawn(async move {
        let result = crate::spawn::spawn_elevated_single_pane_tab(
            argv,
            priority,
            cwd,
            Some(src_window_id),
            term_config,
        )
        .await;

        if let Err(err) = result {
            let message = format!("New Tab Options: {}", err);
            onlyterm_toast_notification::persistent_toast_notification("OnlyTerm", &message);
        }
        // chooser_activity is dropped here, after the async spawn completes.
        drop(chooser_activity);
    })
    .detach();
}

#[path = "newtab_options/modal_impl.rs"]
mod modal_impl;

#[cfg(test)]
mod tests {
    use super::*;
    use onlyterm_config::shell::Shell;

    /// The preamble itself, and the rule for when it may be injected, are
    /// covered by `onlyterm_config::powershell`'s own tests. What matters here is that
    /// this dialog hands PowerShell a session that stays open: `-Command`
    /// without `-NoExit` would run the preamble and immediately exit, closing
    /// the tab the user just asked for.
    #[test]
    fn powershell_argv_keeps_the_session_open_and_sets_utf8() {
        let argv = Shell::Powershell.argv();
        assert_eq!(
            &argv[1..],
            onlyterm_config::powershell::powershell_utf8_args()
        );
        assert!(argv.contains(&"-NoExit".to_string()));
    }

    /// The other shells must not accidentally pick up PowerShell-only flags.
    /// argv[0] varies by machine now that it is a resolved path, so only the
    /// absence of trailing arguments is assertable here.
    #[test]
    fn other_shells_are_launched_bare() {
        for shell in [Shell::Cmd, Shell::Wsl, Shell::Bash] {
            assert_eq!(shell.argv().len(), 1, "{:?} took extra args", shell);
        }
    }

    /// The dialog selects index 0 of the detected list on open, and its
    /// documented default is "cmd". That only holds while detection puts cmd
    /// first, so pin the two together: if cmd is present at all, it leads.
    #[test]
    fn the_dialogs_default_selection_is_cmd_wherever_cmd_exists() {
        let offered = available_shells();
        if offered
            .iter()
            .any(|available| available.shell == Shell::Cmd)
        {
            assert_eq!(
                offered[0].shell,
                Shell::Cmd,
                "the dialog opens on index 0, which must be cmd"
            );
        }
    }
}
