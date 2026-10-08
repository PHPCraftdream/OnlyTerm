use crate::termwindow::box_model::*;
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
            .ok_or_else(|| anyhow::anyhow!("Вкладка уже закрыта"))?;
        let mut rows = vec![];
        for positioned in tab.iter_panes_ignoring_zoom() {
            let pane = positioned.pane;
            let local = pane.downcast_ref::<LocalPane>().ok_or_else(|| {
                anyhow::anyhow!("Отвязка доступна только для локальных процессов")
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
                                let local = pane
                                    .downcast_ref::<LocalPane>()
                                    .ok_or_else(|| anyhow::anyhow!("Панель уже недоступна"))?;
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
                        Ok(rows) => (rows, format!("Не удалось отвязать: {error}")),
                        Err(refresh) => (
                            vec![],
                            format!("Не удалось отвязать: {error}; список недоступен: {refresh:#}"),
                        ),
                    };
                    self.page
                        .replace(MenuPage::Processes(ProcessList::new(rows, Some(message))));
                }
            }
        }
        self.invalidate(term_window);
    }

    pub(crate) fn scroll(&self, amount: i64, term_window: &TermWindow) {
        if amount == 0 {
            return;
        }
        if let MenuPage::Processes(list) = &mut *self.page.borrow_mut() {
            let next = list
                .focused
                .vertical(-amount.signum(), list.rows.len(), false);
            list.focused = match next {
                ProcessFocus::All | ProcessFocus::Process(_) => next,
                _ => list
                    .rows
                    .len()
                    .checked_sub(1)
                    .map(ProcessFocus::Process)
                    .unwrap_or(ProcessFocus::All),
            };
            list.ensure_focus_visible();
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
        let bg = term_window.config.command_palette_bg_color.to_linear();
        let fg = term_window.config.command_palette_fg_color.to_linear();
        let colors = ElementColors {
            border: BorderColor::new(bg),
            bg: bg.into(),
            text: fg.into(),
        };
        let hover = ElementColors {
            border: BorderColor::new(fg),
            bg: fg.into(),
            text: bg.into(),
        };
        let row = |text: String, action: Option<ProcessMenuAction>| {
            let mut element = Element::new(&font, ElementContent::Text(text))
                .colors(colors.clone())
                .display(DisplayType::Block)
                .padding(BoxDimension {
                    left: Dimension::Cells(0.5),
                    right: Dimension::Cells(0.5),
                    top: Dimension::Cells(0.2),
                    bottom: Dimension::Cells(0.2),
                });
            if let Some(action) = action {
                element.item_type = Some(UIItemType::TabProcessMenuItem(action));
                element.hover_colors = Some(hover.clone());
            }
            element
        };
        let mut rows = vec![row("Процессы вкладки".into(), None)];
        match &mut *self.page.borrow_mut() {
            MenuPage::Actions => {
                rows.push(row(
                    "1. Отвязать дочерние процессы".into(),
                    self.pending
                        .borrow()
                        .is_none()
                        .then_some(ProcessMenuAction::OpenDetach),
                ));
                rows.push(row("Esc или F4 — закрыть меню".into(), None));
            }
            MenuPage::Processes(list) => {
                rows.push(row("Выберите процессы и их дочерние процессы".into(), None));
                let all_focused = if list.focused == ProcessFocus::All {
                    ">"
                } else {
                    " "
                };
                let all_marker = if list.all_selected() { "☑" } else { "☐" };
                let can_select = self.pending.borrow().is_none()
                    && list.rows.iter().any(|row| !row.process.detached);
                rows.push(row(
                    format!("{all_focused} {all_marker} Все процессы"),
                    can_select.then_some(ProcessMenuAction::ToggleAll),
                ));
                let row_height = metrics.cell_size.height as f32 * 1.4;
                list.visible_rows = ((dimensions.pixel_height as f32 - row_height * 8.)
                    / row_height)
                    .floor()
                    .max(1.) as usize;
                list.first_visible = list
                    .first_visible
                    .min(list.rows.len().saturating_sub(list.visible_rows));
                let end = (list.first_visible + list.visible_rows).min(list.rows.len());
                if list.rows.is_empty() && list.error.is_none() {
                    rows.push(row("Нет доступных процессов".into(), None));
                }
                for index in list.first_visible..end {
                    let entry = &list.rows[index];
                    let covered = list.covered_by_parent(index);
                    let checked = entry.process.detached || list.selected[index] || covered;
                    let marker = if checked { "☑" } else { "☐" };
                    let status = if entry.process.detached {
                        " (отвязан)"
                    } else {
                        ""
                    };
                    let focused = if list.focused == ProcessFocus::Process(index) {
                        ">"
                    } else {
                        " "
                    };
                    rows.push(row(
                        format!(
                            "{focused} {marker} {}  {}{status}",
                            entry.process.identity.pid, entry.process.name
                        ),
                        (!entry.process.detached && !covered)
                            .then_some(ProcessMenuAction::Toggle(index)),
                    ));
                }
                if end < list.rows.len() || list.first_visible != 0 {
                    rows.push(row(
                        format!(
                            "↑/↓, колёсико: {}–{} из {}",
                            list.first_visible + 1,
                            end,
                            list.rows.len()
                        ),
                        None,
                    ));
                }
                if let Some(error) = list.error.as_ref() {
                    rows.push(row(error.clone(), None));
                }
                let can_detach = self.pending.borrow().is_none()
                    && list.selected.iter().any(|selected| *selected);
                let button = |label: &str, focus: ProcessFocus, enabled: bool| {
                    let mut button = row(label.into(), enabled.then_some(focus.action()))
                        .display(DisplayType::Inline);
                    if list.focused == focus {
                        button.colors = hover.clone();
                    }
                    button
                };
                let buttons = Element::new(
                    &font,
                    ElementContent::Children(vec![
                        button("Отмена", ProcessFocus::Cancel, true),
                        button("Назад", ProcessFocus::Back, true),
                        button("Отвязать", ProcessFocus::Detach, can_detach),
                    ]),
                )
                .display(DisplayType::Block)
                .colors(colors.clone());
                rows.push(buttons);
                rows.push(row(
                    if self.pending.borrow().is_some() {
                        "Отвязка процессов…".into()
                    } else {
                        "Стрелки — выбор; Enter/пробел — действие; Esc — отмена".into()
                    },
                    None,
                ));
            }
        }
        let width_limit = dimensions.pixel_width as f32;
        let height_limit = dimensions.pixel_height as f32;
        let width = (64. * term_window.render_metrics.cell_size.width as f32)
            .min(width_limit)
            .max(1.);
        let root = Element::new(&font, ElementContent::Children(rows))
            .colors(colors)
            .padding(BoxDimension::new(Dimension::Cells(0.25)))
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .min_width(Some(Dimension::Pixels(width)));
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
                    KeyCode::Char('1') | KeyCode::Numpad1 | KeyCode::Enter if root => {
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
