use crate::spawn::SpawnWhere;
use crate::termwindow::box_model::*;
use crate::termwindow::modal::Modal;
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use crate::termwindow::{DimensionContext, NewTabOptionGroup, TermWindow, UIItemType};
use crate::utilsprites::RenderMetrics;
use config::keyassignment::{KeyAssignment, SpawnCommand, SpawnTabDomain};
use config::{Dimension, ProcessPriority};
use std::cell::{Ref, RefCell};
use std::sync::Arc;
use wezterm_term::{KeyCode, KeyModifiers, MouseEvent};

/// Re-exported from the config crate so the dialog and `--start-conf`
/// layouts cannot disagree about what argv each shell name means.
use config::shell::Shell;

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
    selected_shell: RefCell<Shell>,
    selected_elevation: RefCell<Elevation>,
    selected_priority: RefCell<Priority>,
    focus: RefCell<FocusItem>,
}

impl NewTabOptions {
    pub fn new(_term_window: &TermWindow) -> Self {
        Self {
            element: RefCell::new(None),
            selected_shell: RefCell::new(Shell::Cmd),
            selected_elevation: RefCell::new(Elevation::Normal),
            selected_priority: RefCell::new(Priority::Normal),
            focus: RefCell::new(FocusItem::Shell(0)),
        }
    }

    fn total_focus_items() -> usize {
        Shell::ALL.len() + Elevation::ALL.len() + Priority::ALL.len() + 1
    }

    fn focus_item_to_index(item: FocusItem) -> usize {
        match item {
            FocusItem::Shell(idx) => idx,
            FocusItem::Elevation(idx) => Shell::ALL.len() + idx,
            FocusItem::Priority(idx) => Shell::ALL.len() + Elevation::ALL.len() + idx,
            FocusItem::Run => Shell::ALL.len() + Elevation::ALL.len() + Priority::ALL.len(),
        }
    }

    fn index_to_focus_item(index: usize) -> FocusItem {
        let shell_len = Shell::ALL.len();
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
        let current_idx = Self::focus_item_to_index(*self.focus.borrow());
        let total = Self::total_focus_items();
        let new_idx = if direction >= 0 {
            (current_idx as isize + direction).rem_euclid(total as isize) as usize
        } else {
            (current_idx as isize + total as isize + direction).rem_euclid(total as isize) as usize
        };
        *self.focus.borrow_mut() = Self::index_to_focus_item(new_idx);
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
                *self.selected_shell.borrow_mut() = Shell::ALL[idx];
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
        selected_shell: Shell,
        selected_elevation: Elevation,
        selected_priority: Priority,
        focus: FocusItem,
    ) -> anyhow::Result<Vec<ComputedElement>> {
        let font = term_window
            .fonts
            .command_palette_font()
            .expect("to resolve new tab options font");
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());

        let dimensions = &term_window.dimensions;
        let pixel_width = dimensions.pixel_width;
        let pixel_height = dimensions.pixel_height;
        let cols = pixel_width / term_window.render_metrics.cell_size.width as usize;
        let rows = pixel_height / term_window.render_metrics.cell_size.height as usize;

        let padding_top = 40.;
        let padding_bottom = 40.;

        // Size the box to the widest row's actual content instead of a
        // fixed `cols / 4` guess: with 6 priority options ("Below Normal"
        // and "Above Normal" are the longest names), the Priority row is
        // reliably wider than that guess allowed for, which clipped it
        // off the right edge of the box. Each radio item contributes its
        // left/right margin (1.0 + 0.5 cells, matching the radio_element
        // margins built below) plus "<radio glyph> <name>".
        let row_width_cells = |label: &str, names: &[&str]| -> usize {
            let mut width = label.chars().count();
            for name in names {
                width += 1 /* left margin, rounded */ + 2 /* radio glyph + space */ + name.chars().count();
            }
            width
        };
        let shell_names: Vec<&str> = Shell::ALL.iter().map(|s| s.name()).collect();
        let elevation_names: Vec<&str> = Elevation::ALL.iter().map(|s| s.name()).collect();
        let priority_names: Vec<&str> = Priority::ALL.iter().map(|s| s.name()).collect();
        let content_width_cells = [
            "New Tab Options".chars().count(),
            row_width_cells("Shell:", &shell_names),
            row_width_cells("Elevation:", &elevation_names),
            row_width_cells("Priority:", &priority_names),
        ]
        .iter()
        .copied()
        .max()
        .unwrap_or(60);
        // +6 cells covers the root box's own margin/padding/border plus
        // the row wrapper's own margin, so content isn't flush against
        // (or clipped by) the box edge.
        let desired_width = (content_width_cells + 6).min(cols);

        let avail_pixel_width = cols as f32 * term_window.render_metrics.cell_size.width as f32;
        let desired_pixel_width =
            desired_width as f32 * term_window.render_metrics.cell_size.width as f32;

        let bg_color = term_window.config.command_palette_bg_color.to_linear();
        let fg_color = term_window.config.command_palette_fg_color.to_linear();
        let normal_colors = ElementColors {
            border: BorderColor::new(bg_color),
            bg: bg_color.into(),
            text: fg_color.into(),
        };
        // Selection state (the radio glyph's ●) always renders the same
        // way -- inverted bg/text -- regardless of keyboard focus, so it
        // stays visible even after focus moves elsewhere. Keyboard focus
        // is a *separate* signal, shown as a thin border instead (see
        // `style_item` below), so the two states don't compete for the
        // same visual treatment.
        let selected_colors = ElementColors {
            border: BorderColor::new(fg_color),
            bg: fg_color.into(),
            text: bg_color.into(),
        };
        let pick_colors = |is_selected: bool| {
            if is_selected {
                selected_colors.clone()
            } else {
                normal_colors.clone()
            }
        };
        // Applies the selection colors, then overlays a thin 1px border
        // that's either visible (focused) or drawn in the item's own
        // background color (not focused, so it's invisible). The border
        // box is reserved unconditionally either way -- if it were only
        // added on focus, that item (and the row containing it) would be
        // 1px taller/wider than its unfocused siblings, visibly
        // reflowing the whole dialog every time focus moved. The border
        // color is whichever of bg/fg contrasts with that item's own
        // (possibly inverted) background, so the outline stays visible
        // in both the selected and unselected case once focused.
        let style_item = |element: Element, is_selected: bool, is_focused: bool| -> Element {
            let colors = pick_colors(is_selected);
            let own_bg = if is_selected { fg_color } else { bg_color };
            let border_color = if !is_focused {
                own_bg
            } else if is_selected {
                bg_color
            } else {
                fg_color
            };
            element
                .border(BoxDimension::new(Dimension::Pixels(1.)))
                .colors(ElementColors {
                    border: BorderColor::new(border_color),
                    ..colors
                })
        };

        let mut elements = Vec::new();

        // Title: its own block-level row so it starts on its own line.
        let title_element =
            Element::new(&font, ElementContent::Text("New Tab Options".to_string()))
                .colors(normal_colors.clone())
                .margin(BoxDimension {
                    left: Dimension::Cells(0.5),
                    right: Dimension::Cells(0.5),
                    top: Dimension::Cells(0.5),
                    bottom: Dimension::Cells(0.5),
                })
                .display(DisplayType::Block);
        elements.push(title_element);

        // Each option group is one block-level row (so groups stack
        // vertically) containing the label followed by its radio options
        // as inline children (so options within a group sit side by side).
        // `box_model::Element`'s default display is `Inline` -- without
        // `.display(DisplayType::Block)` on the row wrapper, every label
        // and radio across all three groups flows into one continuous
        // horizontal line instead of one row per group (this is what
        // CommandPalette does for each of its own rows, see palette.rs).
        fn build_row(
            font: &std::rc::Rc<wezterm_font::LoadedFont>,
            label: &str,
            normal_colors: &ElementColors,
            row_items: Vec<Element>,
        ) -> Element {
            let label_element = Element::new(font, ElementContent::Text(label.to_string()))
                .colors(normal_colors.clone());
            let mut row = vec![label_element];
            row.extend(row_items);
            Element::new(font, ElementContent::Children(row))
                .colors(normal_colors.clone())
                .margin(BoxDimension {
                    left: Dimension::Cells(0.5),
                    right: Dimension::Cells(0.5),
                    top: Dimension::Cells(0.25),
                    bottom: Dimension::Cells(0.25),
                })
                .display(DisplayType::Block)
        }

        let shell_items = Shell::ALL
            .iter()
            .enumerate()
            .map(|(idx, shell)| {
                let is_selected = selected_shell == *shell;
                let is_focused = matches!(focus, FocusItem::Shell(i) if i == idx);
                let radio_char = if is_selected { "●" } else { "○" };
                let text = format!("{} {}", radio_char, shell.name());
                let mut radio_element = style_item(
                    Element::new(&font, ElementContent::Text(text)),
                    is_selected,
                    is_focused,
                );
                radio_element = radio_element.margin(BoxDimension {
                    left: Dimension::Cells(1.0),
                    right: Dimension::Cells(0.5),
                    top: Dimension::Cells(0.),
                    bottom: Dimension::Cells(0.),
                });
                radio_element.item_type = Some(UIItemType::NewTabOptionRadio {
                    group: NewTabOptionGroup::Shell,
                    choice: idx,
                });
                radio_element
            })
            .collect();
        elements.push(build_row(&font, "Shell:", &normal_colors, shell_items));

        let elevation_items = Elevation::ALL
            .iter()
            .enumerate()
            .map(|(idx, elevation)| {
                let is_selected = selected_elevation == *elevation;
                let is_focused = matches!(focus, FocusItem::Elevation(i) if i == idx);
                let radio_char = if is_selected { "●" } else { "○" };
                let text = format!("{} {}", radio_char, elevation.name());
                let mut radio_element = style_item(
                    Element::new(&font, ElementContent::Text(text)),
                    is_selected,
                    is_focused,
                );
                radio_element = radio_element.margin(BoxDimension {
                    left: Dimension::Cells(1.0),
                    right: Dimension::Cells(0.5),
                    top: Dimension::Cells(0.),
                    bottom: Dimension::Cells(0.),
                });
                radio_element.item_type = Some(UIItemType::NewTabOptionRadio {
                    group: NewTabOptionGroup::Elevation,
                    choice: idx,
                });
                radio_element
            })
            .collect();
        elements.push(build_row(
            &font,
            "Elevation:",
            &normal_colors,
            elevation_items,
        ));

        let priority_items = Priority::ALL
            .iter()
            .enumerate()
            .map(|(idx, priority)| {
                let is_selected = selected_priority == *priority;
                let is_focused = matches!(focus, FocusItem::Priority(i) if i == idx);
                let radio_char = if is_selected { "●" } else { "○" };
                let text = format!("{} {}", radio_char, priority.name());
                let mut radio_element = style_item(
                    Element::new(&font, ElementContent::Text(text)),
                    is_selected,
                    is_focused,
                );
                radio_element = radio_element.margin(BoxDimension {
                    left: Dimension::Cells(1.0),
                    right: Dimension::Cells(0.5),
                    top: Dimension::Cells(0.),
                    bottom: Dimension::Cells(0.),
                });
                radio_element.item_type = Some(UIItemType::NewTabOptionRadio {
                    group: NewTabOptionGroup::Priority,
                    choice: idx,
                });
                radio_element
            })
            .collect();
        elements.push(build_row(
            &font,
            "Priority:",
            &normal_colors,
            priority_items,
        ));

        // Run button: its own block-level row. Not a "selection" in the
        // radio sense, so it only ever gets the focus border, never the
        // inverted selected-colors treatment.
        let is_run_focused = matches!(focus, FocusItem::Run);
        let mut run_element = style_item(
            Element::new(&font, ElementContent::Text("[ Run ]".to_string())),
            false,
            is_run_focused,
        )
        .margin(BoxDimension {
            left: Dimension::Cells(0.5),
            right: Dimension::Cells(0.5),
            top: Dimension::Cells(0.5),
            bottom: Dimension::Cells(0.5),
        })
        .display(DisplayType::Block);
        run_element.item_type = Some(UIItemType::NewTabOptionRun);
        elements.push(run_element);

        let element = Element::new(&font, ElementContent::Children(elements))
            .colors(ElementColors {
                border: BorderColor::new(term_window.config.command_palette_bg_color.to_linear()),
                bg: term_window
                    .config
                    .command_palette_bg_color
                    .to_linear()
                    .into(),
                text: term_window
                    .config
                    .command_palette_fg_color
                    .to_linear()
                    .into(),
            })
            .margin(BoxDimension {
                left: Dimension::Cells(0.25),
                right: Dimension::Cells(0.25),
                top: Dimension::Cells(0.25),
                bottom: Dimension::Cells(0.25),
            })
            .padding(BoxDimension {
                left: Dimension::Cells(0.25),
                right: Dimension::Cells(0.25),
                top: Dimension::Cells(0.25),
                bottom: Dimension::Cells(0.25),
            })
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .border_corners(Some(Corners {
                top_left: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: TOP_LEFT_ROUNDED_CORNER,
                },
                top_right: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: TOP_RIGHT_ROUNDED_CORNER,
                },
                bottom_left: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: BOTTOM_LEFT_ROUNDED_CORNER,
                },
                bottom_right: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: BOTTOM_RIGHT_ROUNDED_CORNER,
                },
            }))
            .min_width(Some(Dimension::Pixels(desired_pixel_width)));

        let x_adjust = ((avail_pixel_width) - desired_pixel_width) / 2.;

        let computed = term_window.compute_element(
            &LayoutContext {
                height: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_height as f32,
                    pixel_cell: metrics.cell_size.height as f32,
                },
                width: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_width as f32,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds: euclid::rect(
                    x_adjust,
                    padding_top,
                    desired_pixel_width,
                    rows as f32 * term_window.render_metrics.cell_size.height as f32
                        - padding_top
                        - padding_bottom,
                ),
                metrics: &metrics,
                gl_state: term_window.render_state.as_ref().unwrap(),
                zindex: 100,
            },
            &element,
        )?;

        Ok(vec![computed])
    }

    pub fn handle_selection(&self, group: NewTabOptionGroup, choice: usize) {
        match group {
            NewTabOptionGroup::Shell => {
                if let Some(shell) = Shell::ALL.get(choice) {
                    *self.selected_shell.borrow_mut() = *shell;
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
        NewTabRunRequest {
            argv: self.selected_shell.borrow().argv(),
            elevated: matches!(*self.selected_elevation.borrow(), Elevation::Admin),
            priority: self.selected_priority.borrow().to_process_priority(),
        }
    }
}

/// The three dialog selections, captured as plain owned data so they can
/// outlive the `NewTabOptions` modal's `RefCell` borrows -- see `run`.
pub struct NewTabRunRequest {
    argv: Vec<String>,
    elevated: bool,
    priority: ProcessPriority,
}

/// Acts on a `NewTabRunRequest`: spawns a normal tab directly, or -- for
/// the elevated case -- spawns an elevated tab inside the existing window
/// and reports back via a toast notification if the user declines or it fails.
/// A declined/failed elevation intentionally does not open a placeholder tab:
/// doing so safely would mean either shell-escaping the failure message before
/// handing it to `cmd.exe /c`, or inventing a new non-PTY pane type that nothing
/// else in this codebase has -- the toast convention already used for
/// `OpenConfigFile` failures avoids both.
pub fn execute_new_tab_run_request(term_window: &mut TermWindow, request: NewTabRunRequest) {
    let NewTabRunRequest {
        argv,
        elevated,
        priority,
    } = request;

    if !elevated {
        term_window.spawn_command(
            &SpawnCommand {
                args: Some(argv),
                priority: Some(priority),
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
    // `promise::spawn::spawn` to keep this function non-blocking from the
    // caller's perspective (matching the old `spawn_elevated_window` pattern).
    let src_window_id = term_window.mux_window_id;
    let term_config = Arc::new(config::TermConfig::with_config(term_window.config.clone()));

    promise::spawn::spawn(async move {
        let result = crate::spawn::spawn_elevated_single_pane_tab(
            argv,
            priority,
            None,
            Some(src_window_id),
            term_config,
        )
        .await;

        if let Err(err) = result {
            let message = format!("New Tab Options: {}", err);
            wezterm_toast_notification::persistent_toast_notification("OnlyTerm", &message);
        }
    })
    .detach();
}

impl Modal for NewTabOptions {
    fn perform_assignment(
        &self,
        _assignment: &KeyAssignment,
        _term_window: &mut TermWindow,
    ) -> bool {
        false
    }

    fn mouse_event(&self, _event: MouseEvent, _term_window: &mut TermWindow) -> anyhow::Result<()> {
        Ok(())
    }

    fn key_down(
        &self,
        key: KeyCode,
        mods: KeyModifiers,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<bool> {
        match (key, mods) {
            (KeyCode::Escape, KeyModifiers::NONE) => {
                term_window.cancel_modal();
            }
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.move_focus(1);
                term_window.invalidate_modal();
            }
            (KeyCode::Tab, KeyModifiers::SHIFT) => {
                self.move_focus(-1);
                term_window.invalidate_modal();
            }
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                match self.select_focused_impl() {
                    Some(request) => {
                        term_window.cancel_modal();
                        execute_new_tab_run_request(term_window, request);
                    }
                    None => term_window.invalidate_modal(),
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn computed_element(
        &self,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<Ref<'_, [ComputedElement]>> {
        if self.element.borrow().is_none() {
            let element = Self::compute(
                term_window,
                *self.selected_shell.borrow(),
                *self.selected_elevation.borrow(),
                *self.selected_priority.borrow(),
                *self.focus.borrow(),
            )?;
            self.element.borrow_mut().replace(element);
        }
        Ok(Ref::map(self.element.borrow(), |v| {
            v.as_ref().unwrap().as_slice()
        }))
    }

    fn reconfigure(&self, _term_window: &mut TermWindow) {
        self.element.borrow_mut().take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The preamble itself, and the rule for when it may be injected, are
    /// covered by `config::powershell`'s own tests. What matters here is that
    /// this dialog hands PowerShell a session that stays open: `-Command`
    /// without `-NoExit` would run the preamble and immediately exit, closing
    /// the tab the user just asked for.
    #[test]
    fn powershell_argv_keeps_the_session_open_and_sets_utf8() {
        let mut expected = vec!["powershell.exe".to_string()];
        expected.extend(config::powershell::powershell_utf8_args());

        assert_eq!(Shell::Powershell.argv(), expected);
        assert!(expected.contains(&"-NoExit".to_string()));
    }

    /// The other shells must not accidentally pick up PowerShell-only flags.
    #[test]
    fn other_shells_are_launched_bare() {
        assert_eq!(Shell::Cmd.argv(), vec!["cmd.exe".to_string()]);
        assert_eq!(Shell::Wsl.argv(), vec!["wsl.exe".to_string()]);
    }
}
