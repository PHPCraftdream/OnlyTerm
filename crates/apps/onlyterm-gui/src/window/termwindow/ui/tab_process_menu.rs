use crate::termwindow::box_model::*;
use crate::termwindow::menu_style::MenuStyle;
use crate::termwindow::modal::Modal;
use crate::termwindow::{DimensionContext, TermWindow, TermWindowNotif, UIItemType};
use crate::utilsprites::RenderMetrics;
use onlyterm_config::Dimension;
use onlyterm_mux::localpane::LocalPane;
use onlyterm_mux::pane::{Pane, PaneId};
use onlyterm_mux::tab::TabId;
use onlyterm_mux::Mux;
use onlyterm_term::{KeyCode, KeyModifiers, MouseEvent};
use portable_pty::win::detach::{ProcessIdentity, PtyProcessInfo};
use std::cell::{Ref, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use window::WindowOps;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessMenuAction {
    OpenDetach,
    ToggleAll,
    Toggle(usize),
    Cancel,
    Back,
    Detach,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcessFocus {
    All,
    Process(usize),
    Cancel,
    Back,
    Detach,
}

impl ProcessFocus {
    fn action(self) -> ProcessMenuAction {
        match self {
            Self::All => ProcessMenuAction::ToggleAll,
            Self::Process(index) => ProcessMenuAction::Toggle(index),
            Self::Cancel => ProcessMenuAction::Cancel,
            Self::Back => ProcessMenuAction::Back,
            Self::Detach => ProcessMenuAction::Detach,
        }
    }

    fn vertical(self, direction: i64, count: usize, can_detach: bool) -> Self {
        if direction < 0 {
            match self {
                Self::All | Self::Process(0) => Self::All,
                Self::Process(index) => Self::Process(index - 1),
                Self::Cancel => count.checked_sub(1).map(Self::Process).unwrap_or(Self::All),
                Self::Back => Self::Cancel,
                Self::Detach => Self::Back,
            }
        } else {
            match self {
                Self::All if count != 0 => Self::Process(0),
                Self::All => Self::Cancel,
                Self::Process(index) if index + 1 < count => Self::Process(index + 1),
                Self::Process(_) => Self::Cancel,
                Self::Cancel => Self::Back,
                Self::Back if can_detach => Self::Detach,
                Self::Back => Self::Back,
                Self::Detach => Self::Detach,
            }
        }
    }

    fn horizontal(self, direction: i64, can_detach: bool) -> Self {
        match (self, direction < 0) {
            (Self::Back, true) => Self::Cancel,
            (Self::Detach, true) => Self::Back,
            (Self::Cancel, false) => Self::Back,
            (Self::Back, false) if can_detach => Self::Detach,
            _ => self,
        }
    }
}

struct ProcessRow {
    pane: Arc<dyn Pane>,
    process: PtyProcessInfo,
}

struct ProcessList {
    rows: Vec<ProcessRow>,
    parents: Vec<Option<usize>>,
    selected: Vec<bool>,
    focused: ProcessFocus,
    first_visible: usize,
    visible_rows: usize,
    error: Option<String>,
}

impl ProcessList {
    fn new(rows: Vec<ProcessRow>, error: Option<String>) -> Self {
        let by_pid: HashMap<_, _> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| ((row.pane.pane_id(), row.process.identity.pid), index))
            .collect();
        let parents = rows
            .iter()
            .map(|row| {
                by_pid
                    .get(&(row.pane.pane_id(), row.process.parent_pid))
                    .copied()
                    .filter(|&parent| {
                        rows[parent].process.identity.created < row.process.identity.created
                    })
            })
            .collect();
        Self {
            selected: vec![false; rows.len()],
            rows,
            parents,
            focused: ProcessFocus::All,
            first_visible: 0,
            visible_rows: 1,
            error,
        }
    }

    fn covered_by_parent(&self, index: usize) -> bool {
        let mut parent = self.parents[index];
        while let Some(index) = parent {
            if self.selected[index] || self.rows[index].process.detached {
                return true;
            }
            parent = self.parents[index];
        }
        false
    }

    fn toggle(&mut self, index: usize) {
        if index < self.rows.len()
            && !self.rows[index].process.detached
            && !self.covered_by_parent(index)
        {
            self.selected[index] = !self.selected[index];
            self.focused = ProcessFocus::Process(index);
        }
    }

    fn all_selected(&self) -> bool {
        !self.rows.is_empty()
            && self.rows.iter().enumerate().all(|(index, row)| {
                row.process.detached || self.selected[index] || self.covered_by_parent(index)
            })
    }

    fn toggle_all(&mut self) {
        let selected = !self.all_selected();
        for (index, row) in self.rows.iter().enumerate() {
            self.selected[index] = selected && !row.process.detached;
        }
        self.focused = ProcessFocus::All;
        self.first_visible = 0;
    }

    fn ensure_focus_visible(&mut self) {
        match self.focused {
            ProcessFocus::All => self.first_visible = 0,
            ProcessFocus::Process(index) if index < self.first_visible => {
                self.first_visible = index;
            }
            ProcessFocus::Process(index) if index >= self.first_visible + self.visible_rows => {
                self.first_visible = index + 1 - self.visible_rows;
            }
            _ => {}
        }
    }

    fn move_focus(&mut self, direction: i64, can_detach: bool, horizontal: bool) {
        self.focused = if horizontal {
            self.focused.horizontal(direction, can_detach)
        } else {
            self.focused
                .vertical(direction, self.rows.len(), can_detach)
        };
        self.ensure_focus_visible();
    }
}

enum MenuPage {
    Actions,
    Processes(ProcessList),
}

pub(crate) struct TabProcessMenu {
    tab_id: TabId,
    page: RefCell<MenuPage>,
    elements: RefCell<Option<Vec<ComputedElement>>>,
    pending: RefCell<Option<Arc<()>>>,
}

impl TabProcessMenu {
    pub(crate) fn new(term_window: &TermWindow) -> Option<Self> {
        let tab = Mux::get().get_active_tab_for_window(term_window.mux_window_id)?;
        Some(Self {
            tab_id: tab.tab_id(),
            page: RefCell::new(MenuPage::Actions),
            elements: RefCell::new(None),
            pending: RefCell::new(None),
        })
    }

    fn process_rows(&self) -> anyhow::Result<Vec<ProcessRow>> {
        let tab = Mux::get()
            .get_tab(self.tab_id)
            .ok_or_else(|| anyhow::anyhow!("The tab has already closed"))?;
        let mut rows = vec![];
        for positioned in tab.iter_panes_ignoring_zoom() {
            let pane = positioned.pane;
            let local = pane.downcast_ref::<LocalPane>().ok_or_else(|| {
                anyhow::anyhow!("Detachment is available only for local processes")
            })?;
            for process in local.child_processes()? {
                rows.push(ProcessRow {
                    pane: Arc::clone(&pane),
                    process,
                });
            }
        }
        rows.sort_by_key(|row| (row.pane.pane_id(), row.process.identity.pid));
        Ok(rows)
    }

    fn invalidate(&self, term_window: &TermWindow) {
        self.elements.borrow_mut().take();
        if let Some(window) = term_window.window.as_ref() {
            window.invalidate();
        }
    }

    pub(crate) fn perform_action(&self, action: ProcessMenuAction, term_window: &mut TermWindow) {
        match action {
            ProcessMenuAction::Cancel => term_window.cancel_modal(),
            ProcessMenuAction::Back => {
                self.page.replace(MenuPage::Actions);
            }
            ProcessMenuAction::OpenDetach => {
                if self.pending.borrow().is_some() {
                    return;
                }
                let (rows, error) = match self.process_rows() {
                    Ok(rows) => (rows, None),
                    Err(error) => (vec![], Some(format!("{error:#}"))),
                };
                self.page
                    .replace(MenuPage::Processes(ProcessList::new(rows, error)));
            }
            ProcessMenuAction::ToggleAll => {
                if self.pending.borrow().is_some() {
                    return;
                }
                if let MenuPage::Processes(list) = &mut *self.page.borrow_mut() {
                    list.toggle_all();
                }
            }
            ProcessMenuAction::Toggle(index) => {
                if self.pending.borrow().is_some() {
                    return;
                }
                if let MenuPage::Processes(list) = &mut *self.page.borrow_mut() {
                    list.toggle(index);
                }
            }
            ProcessMenuAction::Detach => {
                if self.pending.borrow().is_some() {
                    return;
                }
                let mut groups: BTreeMap<PaneId, (Arc<dyn Pane>, Vec<ProcessIdentity>)> =
                    BTreeMap::new();
                if let MenuPage::Processes(list) = &*self.page.borrow() {
                    for (row, selected) in list.rows.iter().zip(&list.selected) {
                        if *selected && !row.process.detached {
                            groups
                                .entry(row.pane.pane_id())
                                .or_insert_with(|| (Arc::clone(&row.pane), vec![]))
                                .1
                                .push(row.process.identity);
                        }
                    }
                }
                if groups.is_empty() {
                    return;
                }
                let Some(window) = term_window.window.as_ref().cloned() else {
                    return;
                };
                let operation = Arc::new(());
                self.pending.replace(Some(Arc::clone(&operation)));
                let worker = std::thread::Builder::new()
                    .name("detach-tab-processes".into())
                    .spawn(move || {
                        let result = (|| -> anyhow::Result<()> {
                            let helper = std::env::current_exe()?;
                            for (_, (pane, processes)) in groups {
                                let local = pane.downcast_ref::<LocalPane>().ok_or_else(|| {
                                    anyhow::anyhow!("The pane is no longer available")
                                })?;
                                local.detach_processes(&processes, &helper)?;
                            }
                            Ok(())
                        })()
                        .map_err(|error| format!("{error:#}"));
                        if let Err(error) = &result {
                            log::warn!("Process detachment failed: {error}");
                        }
                        window.notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                            if let Some(modal) = term_window.get_modal() {
                                if let Some(menu) = modal.downcast_ref::<TabProcessMenu>() {
                                    let matching =
                                        menu.pending.borrow().as_ref().is_some_and(|pending| {
                                            Arc::ptr_eq(pending, &operation)
                                        });
                                    if matching {
                                        menu.complete_detachment(result, term_window);
                                    }
                                }
                            }
                        })));
                    });
                match worker {
                    // The worker owns its request and reports through Window::notify.
                    Ok(worker) => drop(worker),
                    Err(error) => self.complete_detachment(Err(error.to_string()), term_window),
                }
            }
        }
        self.invalidate(term_window);
    }

    fn complete_detachment(&self, result: Result<(), String>, term_window: &mut TermWindow) {
        self.pending.borrow_mut().take();
        if matches!(*self.page.borrow(), MenuPage::Processes(_)) {
            match result {
                Ok(()) => term_window.cancel_modal(),
                Err(error) => {
                    let (rows, message) = match self.process_rows() {
                        Ok(rows) => (rows, format!("Failed to detach: {error}")),
                        Err(refresh) => (
                            vec![],
                            format!(
                                "Failed to detach: {error}; process list unavailable: {refresh:#}"
                            ),
                        ),
                    };
                    self.page
                        .replace(MenuPage::Processes(ProcessList::new(rows, Some(message))));
                }
            }
        }
        self.invalidate(term_window);
    }

    fn navigate(&self, direction: i64, horizontal: bool, term_window: &TermWindow) {
        if let MenuPage::Processes(list) = &mut *self.page.borrow_mut() {
            let can_detach =
                self.pending.borrow().is_none() && list.selected.iter().any(|selected| *selected);
            list.move_focus(direction, can_detach, horizontal);
        }
        self.invalidate(term_window);
    }

    fn compute(&self, term_window: &mut TermWindow) -> anyhow::Result<Vec<ComputedElement>> {
        let font = term_window.fonts.command_palette_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let dimensions = &term_window.dimensions;
        let style = MenuStyle::new(
            term_window.config.command_palette_bg_color,
            term_window.config.command_palette_fg_color,
        );
        let heading = term_window.fonts.title_font()?;
        let colors = style.card.clone();
        let hover = style.focus.clone();
        let width_limit = dimensions.pixel_width as f32;
        let height_limit = dimensions.pixel_height as f32;
        let width = (68. * term_window.render_metrics.cell_size.width as f32)
            .min(width_limit - 32.)
            .max(1.);
        let inner_width = (width - 34.).max(1.);
        let row = |text: String, action: Option<ProcessMenuAction>| {
            if action.is_none() {
                return style.text(&font, text, true);
            }
            let mut element = Element::new(&font, ElementContent::Text(text))
                .colors(colors.clone())
                .display(DisplayType::Block)
                .padding(BoxDimension::new(Dimension::Pixels(5.)))
                .border(BoxDimension::new(Dimension::Pixels(1.)))
                .border_corners(Some(crate::termwindow::menu_style::corners()))
                .min_width(Some(Dimension::Pixels((inner_width - 34.).max(1.))))
                .max_width(Some(Dimension::Pixels((inner_width - 22.).max(1.))))
                .margin(BoxDimension {
                    bottom: Dimension::Pixels(4.),
                    ..BoxDimension::default()
                });
            if let Some(action) = action {
                element.item_type = Some(UIItemType::TabProcessMenuItem(action));
                element.hover_colors = Some(hover.clone());
            }
            element
        };
        let mut rows = vec![
            style.text(&font, "Tab Processes".into(), false),
            style.text(&font, "Manage processes started by this tab".into(), true),
        ];
        match &mut *self.page.borrow_mut() {
            MenuPage::Actions => {
                let mut action = row(
                    "1. Detach child processes".into(),
                    self.pending
                        .borrow()
                        .is_none()
                        .then_some(ProcessMenuAction::OpenDetach),
                );
                if self.pending.borrow().is_none() {
                    action.colors = hover.clone();
                }
                rows.push(style.card(
                    &font,
                    vec![
                        style.text(&heading, "PROCESS ACTIONS".into(), false),
                        action,
                    ],
                    inner_width,
                ));
                rows.push(style.text(&font, "Enter / 1: open · Esc / F4: close".into(), true));
            }
            MenuPage::Processes(list) => {
                rows.push(row(
                    "Select processes and their child-process trees".into(),
                    None,
                ));
                let mut process_rows =
                    vec![style.text(&heading, "PROCESS SELECTION".into(), false)];
                let all_focused = if list.focused == ProcessFocus::All {
                    ">"
                } else {
                    " "
                };
                let all_marker = if list.all_selected() { "☑" } else { "☐" };
                let can_select = self.pending.borrow().is_none()
                    && list.rows.iter().any(|row| !row.process.detached);
                let mut all = row(
                    format!("{all_focused} {all_marker} All Processes"),
                    can_select.then_some(ProcessMenuAction::ToggleAll),
                );
                if list.focused == ProcessFocus::All && can_select {
                    all.colors = hover.clone();
                }
                process_rows.push(all);
                let row_height = metrics.cell_size.height as f32 + 26.;
                list.visible_rows = ((dimensions.pixel_height as f32 - row_height * 10.)
                    / row_height)
                    .floor()
                    .max(1.) as usize;
                list.visible_rows = list.visible_rows.min(list.rows.len().max(1));
                list.first_visible = list
                    .first_visible
                    .min(list.rows.len().saturating_sub(list.visible_rows));
                let end = (list.first_visible + list.visible_rows).min(list.rows.len());
                if list.rows.is_empty() && list.error.is_none() {
                    process_rows.push(style.text(&font, "No available processes".into(), true));
                }
                let mut entries = vec![];
                for index in list.first_visible..end {
                    let entry = &list.rows[index];
                    let covered = list.covered_by_parent(index);
                    let checked = entry.process.detached || list.selected[index] || covered;
                    let marker = if checked { "☑" } else { "☐" };
                    let status = if entry.process.detached {
                        " (detached)"
                    } else {
                        ""
                    };
                    let focused = if list.focused == ProcessFocus::Process(index) {
                        ">"
                    } else {
                        " "
                    };
                    let mut process = row(
                        String::new(),
                        (!entry.process.detached && !covered)
                            .then_some(ProcessMenuAction::Toggle(index)),
                    );
                    process.content = ElementContent::Children(vec![
                        style
                            .text(&font, format!("{focused} {marker}"), false)
                            .display(DisplayType::Inline),
                        style
                            .keycap(&heading, entry.process.identity.pid.to_string())
                            .margin(BoxDimension {
                                left: Dimension::Pixels(8.),
                                right: Dimension::Pixels(8.),
                                ..BoxDimension::default()
                            }),
                        style
                            .text(
                                &font,
                                format!("{}{status}", entry.process.name),
                                entry.process.detached,
                            )
                            .display(DisplayType::Inline),
                    ]);
                    process.min_width = Some(Dimension::Pixels((inner_width - 54.).max(1.)));
                    process.max_width = Some(Dimension::Pixels((inner_width - 42.).max(1.)));
                    if list.focused == ProcessFocus::Process(index) {
                        process.colors = hover.clone();
                    }
                    entries.push(
                        Element::new(&font, ElementContent::Children(vec![process]))
                            .display(DisplayType::Block)
                            .min_height(Some(Dimension::Pixels(row_height))),
                    );
                }
                process_rows.push(style.viewport(
                    &font,
                    entries,
                    inner_width - 22.,
                    list.visible_rows as f32 * row_height,
                    crate::termwindow::menu_style::ScrollPosition {
                        offset: list.first_visible,
                        visible: list.visible_rows,
                        total: list.rows.len(),
                    },
                ));
                if let Some(error) = list.error.as_ref() {
                    process_rows.push(style.text(&font, error.clone(), true));
                }
                let can_detach = self.pending.borrow().is_none()
                    && list.selected.iter().any(|selected| *selected);
                rows.push(style.card(&font, process_rows, inner_width));
                let button = |label: &str, focus: ProcessFocus, enabled: bool| {
                    let mut button = style.button(&font, label, list.focused == focus, enabled);
                    if enabled {
                        button.item_type = Some(UIItemType::TabProcessMenuItem(focus.action()));
                    }
                    button
                };
                let buttons = Element::new(
                    &font,
                    ElementContent::Children(vec![
                        button("Cancel", ProcessFocus::Cancel, true),
                        button("Back", ProcessFocus::Back, true),
                        button("Detach", ProcessFocus::Detach, can_detach),
                    ]),
                )
                .display(DisplayType::Block)
                .colors(colors.clone());
                rows.push(buttons);
                rows.push(row(
                    if self.pending.borrow().is_some() {
                        "Detaching processes…".into()
                    } else {
                        "Arrows to select; Enter/Space to activate; Esc to cancel".into()
                    },
                    None,
                ));
            }
        }
        let root = style.panel(&font, rows, width);
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
}

impl Modal for TabProcessMenu {
    fn scroll_position(&self) -> Option<crate::termwindow::menu_style::ScrollPosition> {
        match &*self.page.borrow() {
            MenuPage::Processes(list) => Some(crate::termwindow::menu_style::ScrollPosition {
                offset: list.first_visible,
                visible: list.visible_rows,
                total: list.rows.len(),
            }),
            _ => None,
        }
    }

    fn set_scroll_offset(&self, offset: usize, window: &mut TermWindow) {
        if let MenuPage::Processes(list) = &mut *self.page.borrow_mut() {
            list.first_visible = offset.min(list.rows.len().saturating_sub(list.visible_rows));
            if let ProcessFocus::Process(index) = list.focused {
                list.focused = ProcessFocus::Process(index.clamp(
                    list.first_visible,
                    list.first_visible + list.visible_rows.saturating_sub(1),
                ));
            }
        }
        self.invalidate(window);
    }

    fn blocks_terminal_input(&self) -> bool {
        true
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
            (KeyCode::Escape, _) | (KeyCode::Function(4), KeyModifiers::NONE) => {
                self.perform_action(ProcessMenuAction::Cancel, term_window);
            }
            (key, KeyModifiers::NONE) => {
                let root = matches!(*self.page.borrow(), MenuPage::Actions);
                match key {
                    KeyCode::Char('1') | KeyCode::Numpad1 | KeyCode::Enter | KeyCode::Char(' ')
                        if root =>
                    {
                        self.perform_action(ProcessMenuAction::OpenDetach, term_window);
                    }
                    KeyCode::Char(' ') | KeyCode::Enter if !root => {
                        let action = match &*self.page.borrow() {
                            MenuPage::Processes(list) => list.focused.action(),
                            MenuPage::Actions => ProcessMenuAction::OpenDetach,
                        };
                        self.perform_action(action, term_window);
                    }
                    KeyCode::UpArrow if !root => self.navigate(-1, false, term_window),
                    KeyCode::DownArrow if !root => self.navigate(1, false, term_window),
                    KeyCode::LeftArrow if !root => self.navigate(-1, true, term_window),
                    KeyCode::RightArrow if !root => self.navigate(1, true, term_window),
                    KeyCode::Backspace if !root => {
                        self.perform_action(ProcessMenuAction::Back, term_window);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(true)
    }

    fn computed_element(
        &self,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<Ref<'_, [ComputedElement]>> {
        if self.elements.borrow().is_none() {
            self.elements
                .borrow_mut()
                .replace(self.compute(term_window)?);
        }
        Ok(Ref::map(self.elements.borrow(), |elements| {
            elements.as_ref().unwrap().as_slice()
        }))
    }

    fn reconfigure(&self, _term_window: &mut TermWindow) {
        self.elements.borrow_mut().take();
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessFocus as Focus;

    #[test]
    fn vertical_focus_reaches_buttons_and_returns_to_the_list() {
        let mut focus = Focus::All;
        for expected in [
            Focus::Process(0),
            Focus::Process(1),
            Focus::Cancel,
            Focus::Back,
            Focus::Detach,
        ] {
            focus = focus.vertical(1, 2, true);
            assert_eq!(focus, expected);
        }
        for expected in [
            Focus::Back,
            Focus::Cancel,
            Focus::Process(1),
            Focus::Process(0),
            Focus::All,
        ] {
            focus = focus.vertical(-1, 2, true);
            assert_eq!(focus, expected);
        }
    }

    #[test]
    fn horizontal_buttons_are_bounded_and_skip_disabled_detach() {
        assert_eq!(Focus::Cancel.horizontal(-1, true), Focus::Cancel);
        assert_eq!(Focus::Cancel.horizontal(1, true), Focus::Back);
        assert_eq!(Focus::Back.horizontal(1, false), Focus::Back);
        assert_eq!(Focus::Back.horizontal(1, true), Focus::Detach);
        assert_eq!(Focus::Detach.horizontal(1, true), Focus::Detach);
        assert_eq!(Focus::Detach.horizontal(-1, true), Focus::Back);
        assert_eq!(Focus::Process(0).horizontal(1, true), Focus::Process(0));
    }

    #[test]
    fn empty_list_keeps_cancel_and_back_keyboard_accessible() {
        let cancel = Focus::All.vertical(1, 0, false);
        assert_eq!(cancel, Focus::Cancel);
        let back = cancel.vertical(1, 0, false);
        assert_eq!(back, Focus::Back);
        assert_eq!(back.vertical(1, 0, false), Focus::Back);
        assert_eq!(cancel.vertical(-1, 0, false), Focus::All);
    }
}
