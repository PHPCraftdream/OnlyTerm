use crate::spawn::SpawnWhere;
use crate::termwindow::box_model::*;
use crate::termwindow::modal::Modal;
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use crate::termwindow::{DimensionContext, NewTabOptionGroup, TermWindow, UIItemType};
use crate::utilsprites::RenderMetrics;
use ::window::{Connection, ConnectionOps};
use config::keyassignment::{KeyAssignment, SpawnCommand, SpawnTabDomain};
use config::{Dimension, ProcessPriority};
use mux::activity::Activity;
use std::cell::{Ref, RefCell};
use std::path::PathBuf;
use std::sync::Arc;
use wezterm_term::{KeyCode, KeyModifiers, MouseEvent};

/// Re-exported from the config crate so the dialog and `--start-conf`
/// layouts cannot disagree about what argv each shell name means.
use config::shell::{available_shells, AvailableShell};

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
        let shell_names: Vec<&str> = shells.iter().map(|s| s.shell.name()).collect();
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

        // Title: its own block-level row so it starts on its own line, with
        // the close cross floated to the right edge of that row -- the same
        // arrangement `fancy_tab_bar` uses for a tab's own close button.
        // `×` (U+00D7) rather than a dedicated cross glyph: it is present in
        // essentially every font, so it cannot fall back to a tofu box.
        //
        // A `Children` row shrink-wraps to its own content by default (see
        // `box_model::compute_element`'s `Children` branch), so without help
        // this row would only be as wide as "New Tab Options ×" -- which is
        // why the cross used to sit right after the text instead of in the
        // panel's corner. An explicit `min_width` on the row fixes that.
        //
        // The subtle part is what that width must be. A `Float::Right` child
        // is laid out at `max_x + float_width` (where `max_x` has already
        // been raised to `min_width`), so the float's own width is added ON
        // TOP of `min_width` rather than fitting inside it. Asking for the
        // full panel width here therefore produces a row one cross wider
        // than the panel, and the cross hangs off the right edge -- which is
        // exactly what happened. Reserving space by *guessing* the cross's
        // rendered width failed too: an estimate of "padding + about one
        // cell" undershot, because text shaping makes no promise that a
        // glyph advance is a whole number of grid cells.
        //
        // So the cross's width is not estimated, it is anchored: `min_width`
        // holds its content box at one cell, making its outer width
        // padding + 1 cell. (`max_width` must NOT be used for this. On a
        // text element that is a clipping bound, not a shrink bound -- the
        // glyph loop breaks as soon as the next advance would cross it, so
        // pinning max_width to one cell deleted the "×" entirely and left
        // only its hover rectangle behind.)
        //
        // The other half is measuring against the right thing. The panel is
        // handed a layout rect of `desired_pixel_width`, but its own border
        // and padding are subtracted from *inside* that, so the content area
        // every row actually gets is narrower -- by 0.25 cell of padding and
        // 1px of border on each side. Computing this row from the outer
        // figure, as an earlier attempt did, therefore overshot by exactly
        // that much and pushed the cross past the panel edge.
        let cell_w = term_window.render_metrics.cell_size.width as f32;
        let title_row_margin_cells = 1.0; // this row's own 0.5 + 0.5 left/right margin, below
                                          // Equal left/right padding, so the glyph sits centred in the
                                          // highlight box that appears under the pointer -- unequal padding
                                          // put it visibly off to one side.
        let close_pad_cells = 0.5;
        let close_content_cells = 1.0; // anchored by min_width below
                                       // A margin, not a narrower row: the float is positioned by its OUTER
                                       // width, so a right margin pushes the visible box inward while the
                                       // row -- and therefore the rule drawn under it -- still spans the
                                       // full width. Shrinking the row instead would have pulled that rule
                                       // short on one side only. It doubles as slack for the last couple of
                                       // pixels of overshoot that the arithmetic above still leaves.
        let close_margin_right_cells = 0.75;
        let close_outer_width =
            (close_pad_cells * 2. + close_content_cells + close_margin_right_cells) * cell_w;

        // The panel's usable content width: its layout rect less its own
        // 0.25-cell padding and 1px border on each side.
        let panel_content_width = (desired_pixel_width - 0.5 * cell_w - 2.0).max(0.);
        // The width this row occupies inside that, less its own margins.
        let title_row_total_width = (panel_content_width - title_row_margin_cells * cell_w).max(0.);
        // What to ask for, given a Float::Right child is placed at
        // `min_width + float_width` and so adds its own width on top.
        let title_row_min_width = (title_row_total_width - close_outer_width).max(0.);

        let title_str = "New Tab Options";
        let title_text_width = title_str.chars().count() as f32 * cell_w;
        // Centers the title across the row's full width -- there is no
        // text-align primitive in this box model, only padding. Clamped so a
        // long title can never be pushed underneath the cross.
        let title_left_pad = ((title_row_total_width - title_text_width) / 2.)
            .min((title_row_min_width - title_text_width).max(0.))
            .max(0.);
        let title_text = Element::new(&font, ElementContent::Text(title_str.to_string()))
            .colors(normal_colors.clone())
            .padding(BoxDimension {
                left: Dimension::Pixels(title_left_pad),
                right: Dimension::Pixels(0.),
                top: Dimension::Pixels(0.),
                bottom: Dimension::Pixels(0.),
            });

        let close_element = Element::new(&font, ElementContent::Text("×".to_string()))
            .colors(normal_colors.clone())
            .hover_colors(Some(selected_colors.clone()))
            // No min_width: it would hold the content box at a full cell
            // while the "×" glyph advance is narrower, so the glyph rendered
            // hard against the left of its own highlight box. Letting the box
            // shrink-wrap the glyph lets the equal padding centre it. The
            // reserve computed above stays valid because it now OVER-states
            // the cross's width, and over-stating is the safe direction:
            // the float is placed at `row min_width + its actual width`, so
            // a too-large reserve only tucks it further inside.
            .padding(BoxDimension {
                left: Dimension::Cells(close_pad_cells),
                right: Dimension::Cells(close_pad_cells),
                top: Dimension::Cells(0.),
                bottom: Dimension::Cells(0.),
            })
            .margin(BoxDimension {
                left: Dimension::Cells(0.),
                right: Dimension::Cells(close_margin_right_cells),
                top: Dimension::Cells(0.),
                bottom: Dimension::Cells(0.),
            })
            .float(Float::Right)
            .item_type(UIItemType::NewTabOptionClose);

        // The rule under the title is a bottom-only border: `BoxDimension`
        // fields are independent, so left/top/right stay zero and nothing
        // renders there, while a nonzero bottom width and an opaque border
        // color (unlike `normal_colors.border`, which matches the
        // background and is therefore invisible by design elsewhere in this
        // dialog) draw a single visible line spanning this row's width.
        let title_rule_colors = ElementColors {
            border: BorderColor::new(fg_color),
            ..normal_colors.clone()
        };
        let title_element = Element::new(
            &font,
            ElementContent::Children(vec![title_text, close_element]),
        )
        .colors(title_rule_colors)
        .min_width(Some(Dimension::Pixels(title_row_min_width)))
        .margin(BoxDimension {
            left: Dimension::Cells(0.5),
            right: Dimension::Cells(0.5),
            top: Dimension::Cells(0.5),
            bottom: Dimension::Cells(0.25),
        })
        .padding(BoxDimension {
            left: Dimension::Pixels(0.),
            top: Dimension::Pixels(0.),
            right: Dimension::Pixels(0.),
            bottom: Dimension::Cells(0.25),
        })
        .border(BoxDimension {
            left: Dimension::Pixels(0.),
            top: Dimension::Pixels(0.),
            right: Dimension::Pixels(0.),
            bottom: Dimension::Pixels(1.),
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

        let shell_items = shells
            .iter()
            .enumerate()
            .map(|(idx, available)| {
                let is_selected = selected_shell == idx;
                let is_focused = matches!(focus, FocusItem::Shell(i) if i == idx);
                let radio_char = if is_selected { "●" } else { "○" };
                let text = format!("{} {}", radio_char, available.shell.name());
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

        // Esc has always dismissed this dialog, but nothing on screen said
        // so. Carries no `item_type`, so it is inert: not clickable, and not
        // part of the Tab focus order (`FocusItem` drives that, and this line
        // deliberately has no entry there).
        let hint_str = "Esc or × to close";
        let hint_left_pad =
            ((title_row_total_width - hint_str.chars().count() as f32 * cell_w) / 2.).max(0.);
        let hint_element = Element::new(&font, ElementContent::Text(hint_str.to_string()))
            .colors(normal_colors.clone())
            .padding(BoxDimension {
                left: Dimension::Pixels(hint_left_pad),
                right: Dimension::Pixels(0.),
                top: Dimension::Pixels(0.),
                bottom: Dimension::Pixels(0.),
            })
            .margin(BoxDimension {
                left: Dimension::Cells(0.5),
                right: Dimension::Cells(0.5),
                top: Dimension::Cells(0.),
                bottom: Dimension::Cells(0.5),
            })
            .display(DisplayType::Block);
        elements.push(hint_element);

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
        // ordering, not timing: `promise::spawn::spawn` and
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
    // `promise::spawn::spawn` to keep this function non-blocking from the
    // caller's perspective (matching the old `spawn_elevated_window` pattern).
    let term_config = Arc::new(config::TermConfig::with_config(term_window.config.clone()));

    promise::spawn::spawn(async move {
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
            wezterm_toast_notification::persistent_toast_notification("OnlyTerm", &message);
        }
        // chooser_activity is dropped here, after the async spawn completes.
        drop(chooser_activity);
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
                self.dismiss(term_window);
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
                &self.shells,
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
    use config::shell::Shell;

    /// The preamble itself, and the rule for when it may be injected, are
    /// covered by `config::powershell`'s own tests. What matters here is that
    /// this dialog hands PowerShell a session that stays open: `-Command`
    /// without `-NoExit` would run the preamble and immediately exit, closing
    /// the tab the user just asked for.
    #[test]
    fn powershell_argv_keeps_the_session_open_and_sets_utf8() {
        let argv = Shell::Powershell.argv();
        assert_eq!(&argv[1..], config::powershell::powershell_utf8_args());
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

    /// The ordinary constructor defaults to OnCancel::Dismiss, so a
    /// right-click on the plus button (or ActivateNewTabOptions key binding)
    /// never accidentally quits the application even if the startup
    /// chooser code later changes.
    #[test]
    fn ordinary_ctor_defaults_to_dismiss() {
        let dialog = NewTabOptions::new();
        assert_eq!(dialog.on_cancel(), OnCancel::Dismiss);
    }
}
