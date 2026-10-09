use crate::commands::derive_command_from_key_assignment;
use crate::inputmap::ui_key;
use crate::termwindow::box_model::*;
use crate::termwindow::menu_style::{corners as panel_corners, MenuStyle};
use crate::termwindow::modal::Modal;
use crate::termwindow::{DimensionContext, TermWindow, UIItemType};
use crate::utilsprites::RenderMetrics;
use onlyterm_config::keyassignment::{KeyAssignment, KeyTable};
use onlyterm_config::{ConfigHandle, Dimension};
use onlyterm_term::{KeyCode, KeyModifiers, MouseEvent};
use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};
use std::collections::BTreeMap;
use window::color::LinearRgba;
use window::{KeyCode as WindowKey, ModifierToStringArgs, Modifiers, WindowOps};

#[path = "help_menu/selection.rs"]
mod selection;
use selection::{wrap_document, TextSelection};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpTopic {
    Keyboard,
    Settings,
    Launch,
    ConfigLaunch,
    DoubleCtrl,
}

impl HelpTopic {
    const ALL: [Self; 5] = [
        Self::Keyboard,
        Self::Settings,
        Self::Launch,
        Self::ConfigLaunch,
        Self::DoubleCtrl,
    ];
    fn title(self) -> &'static str {
        match self {
            Self::Keyboard => "Keyboard Details",
            Self::Settings => "Settings",
            Self::Launch => "Launch Options",
            Self::ConfigLaunch => "Config-based Launches",
            Self::DoubleCtrl => "Double Ctrl / Pass-through",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Keyboard => "Bindings, modes and custom shortcuts",
            Self::Settings => "Configuration, fonts and appearance",
            Self::Launch => "Command-line options and examples",
            Self::ConfigLaunch => "Startup layouts and saved commands",
            Self::DoubleCtrl => "Send the next key directly to your application",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpAction {
    Open(HelpTopic),
    Back,
    Copy,
    Close,
}

struct Shortcut {
    key: String,
    label: Cow<'static, str>,
    description: Cow<'static, str>,
}

#[derive(Clone, Copy)]
enum HelpLineStyle {
    Body,
    Heading,
    Value,
    Code,
    Binding,
}

struct HelpLine {
    text: String,
    style: HelpLineStyle,
    source: Range<usize>,
}

fn line_style(line: &str) -> HelpLineStyle {
    let text = line.trim();
    if text.starts_with("Key Table:") || (text.ends_with(':') && !text.starts_with("--")) {
        HelpLineStyle::Heading
    } else if text.starts_with("onlyterm ")
        || text.starts_with('[')
        || text.starts_with(']')
        || text.starts_with('{')
        || text.starts_with('}')
        || text.as_bytes().get(1) == Some(&b':')
        || text.contains("://")
    {
        HelpLineStyle::Code
    } else if text.split_once(':').is_some_and(|(key, _)| {
        key.starts_with("--")
            || key == "Current setting"
            || key == "Default bindings disabled"
            || key == "Tap limit"
            || key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }) {
        HelpLineStyle::Value
    } else if text
        .split_once(" - ")
        .is_some_and(|(key, _)| !key.contains(' '))
    {
        HelpLineStyle::Binding
    } else {
        HelpLineStyle::Body
    }
}

fn shortcuts(input: &KeyTable, config: &ConfigHandle) -> Vec<Shortcut> {
    let mut rows = BTreeMap::new();
    for ((key, mods), entry) in input {
        if matches!(
            entry.action,
            KeyAssignment::Nop | KeyAssignment::DisableDefaultAssignment
        ) {
            continue;
        }
        let mut display_mods = *mods;
        if let WindowKey::Char(c) = key {
            if c.is_ascii_uppercase() || "!@#$%^&*()_+{}|:\"<>?".contains(*c) {
                display_mods |= Modifiers::SHIFT;
            }
        }
        // Prefer the physical binding that Windows dispatches before its mapped alias.
        if !matches!(key, WindowKey::Physical(_)) {
            if let Some(phys) = key.to_phys() {
                if input.contains_key(&(WindowKey::Physical(phys), display_mods)) {
                    continue;
                }
            }
        }
        let mut label = ui_key(key, config.ui_key_cap_rendering);
        if label.len() == 1 {
            label.make_ascii_uppercase();
        }
        let prefix = display_mods.to_string_with_separator(ModifierToStringArgs {
            separator: "+",
            want_none: false,
            ui_key_cap_rendering: Some(config.ui_key_cap_rendering),
        });
        let label = if prefix.is_empty() {
            label
        } else {
            format!("{prefix}+{label}")
        };
        let (brief, description) = match derive_command_from_key_assignment(&entry.action) {
            Some(command) => (command.brief, command.doc),
            None => (
                format!("{:?}", entry.action).into(),
                "The label includes this action's parameters.".into(),
            ),
        };
        let rank = label
            .strip_prefix('F')
            .and_then(|s| s.parse::<u8>().ok())
            .unwrap_or(u8::MAX);
        rows.insert((rank, label), (brief, description));
    }
    rows.into_iter()
        .map(|((_, key), (label, description))| Shortcut {
            key,
            label,
            description,
        })
        .collect()
}

pub(crate) struct HelpMenu {
    shortcuts: Vec<Shortcut>,
    config_path: String,
    defaults_disabled: bool,
    double_ctrl: bool,
    tables: Vec<(String, Vec<Shortcut>)>,
    page: Cell<Option<HelpTopic>>,
    selected: Cell<usize>,
    offset: Cell<usize>,
    main_offset: Cell<usize>,
    visible: Cell<usize>,
    total: Cell<usize>,
    document: RefCell<String>,
    wrapped: RefCell<(usize, Vec<HelpLine>)>,
    elements: RefCell<Option<Vec<ComputedElement>>>,
    text_selection: RefCell<TextSelection>,
}

impl HelpMenu {
    pub(crate) fn new(window: &TermWindow) -> Self {
        let shortcuts = shortcuts(&window.input_map.keys.default, &window.config);
        let total = shortcuts.len();
        let mut tables: Vec<_> = window
            .input_map
            .keys
            .by_name
            .iter()
            .map(|(name, table)| (name.clone(), self::shortcuts(table, &window.config)))
            .collect();
        tables.sort_by(|a, b| a.0.cmp(&b.0));
        Self {
            shortcuts,
            config_path: onlyterm_config::Config::config_file_path()
                .display()
                .to_string(),
            defaults_disabled: window.config.disable_default_key_bindings,
            double_ctrl: window.config.pass_through_next_key_on_double_ctrl,
            tables,
            page: Cell::new(None),
            selected: Cell::new(0),
            offset: Cell::new(0),
            main_offset: Cell::new(0),
            visible: Cell::new(1),
            total: Cell::new(total),
            document: RefCell::new(String::new()),
            wrapped: RefCell::new((0, vec![])),
            elements: RefCell::new(None),
            text_selection: RefCell::new(TextSelection::default()),
        }
    }

    fn actions(&self) -> &'static [HelpAction] {
        if self.page.get().is_some() {
            &[HelpAction::Copy, HelpAction::Back, HelpAction::Close]
        } else {
            &[
                HelpAction::Open(HelpTopic::Keyboard),
                HelpAction::Open(HelpTopic::Settings),
                HelpAction::Open(HelpTopic::Launch),
                HelpAction::Open(HelpTopic::ConfigLaunch),
                HelpAction::Open(HelpTopic::DoubleCtrl),
                HelpAction::Close,
            ]
        }
    }

    fn invalidate(&self, window: &TermWindow) {
        self.elements.borrow_mut().take();
        if let Some(window) = window.window.as_ref() {
            window.invalidate();
        }
    }

    fn move_focus(&self, direction: isize) {
        let count = self.actions().len();
        for _ in 0..count {
            self.selected.set(
                (self.selected.get() as isize + direction).rem_euclid(count as isize) as usize,
            );
            if self.actions()[self.selected.get()] != HelpAction::Copy
                || self.text_selection.borrow().range().is_some()
            {
                break;
            }
        }
    }

    pub(crate) fn scroll(&self, direction: i64, window: &TermWindow) {
        let step = self.visible.get().max(1);
        self.offset.set(if direction < 0 {
            self.offset.get().saturating_sub(step)
        } else {
            self.offset
                .get()
                .saturating_add(step)
                .min(self.total.get().saturating_sub(self.visible.get()))
        });
        self.invalidate(window);
    }

    pub(crate) fn perform_action(&self, action: HelpAction, window: &mut TermWindow) {
        match action {
            HelpAction::Open(topic) => {
                self.main_offset.set(self.offset.get());
                self.text_selection.replace(TextSelection::default());
                self.page.set(Some(topic));
                *self.document.borrow_mut() = self.topic_text(topic);
                self.wrapped.borrow_mut().0 = 0;
                self.offset.set(0);
                self.selected.set(1);
            }
            HelpAction::Back => {
                let topic = self.page.replace(None);
                self.text_selection.replace(TextSelection::default());
                self.selected.set(
                    HelpTopic::ALL
                        .iter()
                        .position(|t| Some(*t) == topic)
                        .unwrap_or(0),
                );
                self.offset.set(self.main_offset.get());
                self.total.set(self.shortcuts.len());
            }
            HelpAction::Close => window.cancel_modal(),
            HelpAction::Copy => self.copy_selection(
                window,
                onlyterm_config::keyassignment::ClipboardCopyDestination::Clipboard,
            ),
        }
        self.invalidate(window);
    }

    fn topic_text(&self, topic: HelpTopic) -> String {
        match topic {
            HelpTopic::Keyboard => {
                let mut text = format!("These are the current normal-mode bindings, including configuration overrides. Reopen F1 after changing configuration.\n\nDefault bindings disabled: {}\n\nKey combinations mentioned in the other guides are defaults; configuration can override them. Copy/search modes and activated key tables have their own bindings, listed below. A LEADER binding requires the configured leader first. Windows can reserve Win-key shortcuts before OnlyTerm receives them.\n\nUse onlyterm show-keys to inspect mouse bindings too.\n", self.defaults_disabled);
                for row in &self.shortcuts { text.push_str(&format!("\n{} - {}\n{}\n", row.key, row.label, row.description)); }
                for (name, shortcuts) in &self.tables {
                    text.push_str(&format!("\nKey Table: {name} (only while this table is active)\n"));
                    for row in shortcuts { text.push_str(&format!("\n{} - {}\n{}\n", row.key, row.label, row.description)); }
                }
                text
            }
            HelpTopic::Settings => format!("Config path used by Ctrl+O:\n{}\n\n{}", self.config_path, SETTINGS_HELP),
            HelpTopic::Launch => LAUNCH_HELP.into(),
            HelpTopic::ConfigLaunch => CONFIG_LAUNCH_HELP.into(),
            HelpTopic::DoubleCtrl => format!(
                "Current setting: pass_through_next_key_on_double_ctrl: {}\nTap limit: {} ms; interval after first release: {} ms.\n\n{}",
                self.double_ctrl,
                crate::termwindow::keyevent::pass_through::TAP_MAX_HOLD.as_millis(),
                crate::termwindow::keyevent::pass_through::DOUBLE_TAP_INTERVAL.as_millis(),
                DOUBLE_CTRL_HELP,
            ),
        }
    }

    fn copy_selection(
        &self,
        window: &TermWindow,
        destination: onlyterm_config::keyassignment::ClipboardCopyDestination,
    ) {
        if let Some(range) = self.text_selection.borrow().range() {
            window.copy_to_clipboard(destination, self.document.borrow()[range].to_owned());
        }
    }

    pub(crate) fn text_mouse_event(
        &self,
        event: &window::MouseEvent,
        window: &mut TermWindow,
    ) -> bool {
        use window::{MouseEventKind, MousePress};
        if self.page.get().is_none() {
            return false;
        }
        let (inside, position) = {
            let elements = self.elements.borrow();
            let Some(elements) = elements.as_ref() else {
                return false;
            };
            let x = event.coords.x as f32;
            let y = event.coords.y as f32;
            (
                selection::is_text_region(elements, x, y),
                selection::text_at(elements, &self.document.borrow(), x, y),
            )
        };
        match event.kind {
            MouseEventKind::Press(MousePress::Left) if inside => {
                if let Some(position) = position {
                    let mut selection = self.text_selection.borrow_mut();
                    if !event.modifiers.contains(Modifiers::SHIFT) || selection.anchor.is_none() {
                        selection.anchor = Some(position);
                    }
                    selection.focus = position;
                    selection.dragging = true;
                }
                self.invalidate(window);
                true
            }
            MouseEventKind::Move if self.text_selection.borrow().dragging => {
                if let Some(position) = position {
                    self.text_selection.borrow_mut().focus = position;
                }
                self.invalidate(window);
                true
            }
            MouseEventKind::Release(MousePress::Left) if self.text_selection.borrow().dragging => {
                let mut selection = self.text_selection.borrow_mut();
                if let Some(position) = position {
                    selection.focus = position;
                }
                selection.dragging = false;
                drop(selection);
                self.invalidate(window);
                true
            }
            MouseEventKind::Move if inside => true,
            _ => false,
        }
    }

    fn compute(&self, window: &mut TermWindow) -> anyhow::Result<Vec<ComputedElement>> {
        let font = window.fonts.command_palette_font()?;
        let heading_font = window.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let dimensions = &window.dimensions;
        let width_limit = dimensions.pixel_width as f32;
        let height_limit = dimensions.pixel_height as f32;
        let width = (width_limit - 32.).clamp(1., 1500.);
        let height = (height_limit - 32.).max(1.);
        let cell_height = metrics.cell_size.height as f32;
        let cell_width = metrics.cell_size.width as f32;
        let content_width = (width - 34.).max(1.);
        let columns = (content_width / (16. * cell_height)).floor().clamp(1., 3.) as usize;
        let chars = ((content_width - 46.) / (cell_height * 0.6))
            .floor()
            .max(1.) as usize;
        let style = MenuStyle::new(
            window.config.command_palette_bg_color,
            window.config.command_palette_fg_color,
        );
        let fg = style.foreground;
        let muted = style.muted;
        let edge = style.edge;
        let chip = style.chip;
        let card_colors = style.card.clone();
        let focus_colors = style.focus.clone();
        let text = |value: String, ink: LinearRgba| {
            Element::new(&font, ElementContent::Text(value))
                .colors(ElementColors {
                    text: ink.into(),
                    ..ElementColors::default()
                })
                .display(DisplayType::Block)
                .padding(BoxDimension {
                    left: Dimension::Pixels(0.),
                    right: Dimension::Pixels(0.),
                    top: Dimension::Pixels(2.),
                    bottom: Dimension::Pixels(2.),
                })
        };
        let heading = |value: String, ink: LinearRgba| {
            Element::new(&heading_font, ElementContent::Text(value))
                .colors(ElementColors {
                    text: ink.into(),
                    ..ElementColors::default()
                })
                .display(DisplayType::Block)
                .padding(BoxDimension {
                    top: Dimension::Pixels(4.),
                    bottom: Dimension::Pixels(4.),
                    ..BoxDimension::default()
                })
        };
        let card = |children: Vec<Element>| {
            Element::new(&font, ElementContent::Children(children))
                .colors(card_colors.clone())
                .display(DisplayType::Block)
                .padding(BoxDimension::new(Dimension::Pixels(12.)))
                .border(BoxDimension::new(Dimension::Pixels(1.)))
                .border_corners(Some(panel_corners()))
                .min_width(Some(Dimension::Pixels((content_width - 26.).max(1.))))
                .max_width(Some(Dimension::Pixels(content_width)))
                .margin(BoxDimension {
                    bottom: Dimension::Pixels(8.),
                    ..BoxDimension::default()
                })
        };
        let keycap = |value: String| {
            Element::new(&font, ElementContent::Text(value))
                .colors(ElementColors {
                    border: BorderColor::new(edge),
                    bg: chip.into(),
                    text: fg.into(),
                })
                .padding(BoxDimension {
                    left: Dimension::Pixels(8.),
                    right: Dimension::Pixels(8.),
                    top: Dimension::Pixels(4.),
                    bottom: Dimension::Pixels(4.),
                })
                .border(BoxDimension::new(Dimension::Pixels(1.)))
                .border_corners(Some(panel_corners()))
        };
        let mut rows = vec![
            text(
                self.page
                    .get()
                    .map(HelpTopic::title)
                    .unwrap_or("OnlyTerm Help")
                    .into(),
                fg,
            ),
            text(
                self.page
                    .get()
                    .map(HelpTopic::hint)
                    .unwrap_or("Keyboard shortcuts and guides, without leaving your terminal")
                    .into(),
                muted,
            )
            .margin(BoxDimension {
                bottom: Dimension::Pixels(10.),
                ..BoxDimension::default()
            }),
        ];
        if columns == 1 && self.page.get().is_none() {
            rows.pop();
        }
        if self.page.get().is_none() {
            let reserved_rows = if columns > 1 { 23. } else { 30. };
            let row_height = cell_height + 22.;
            let grid_rows = ((height - reserved_rows * cell_height) / row_height)
                .floor()
                .clamp(1., 8.) as usize;
            self.visible.set(grid_rows * columns);
            self.total.set(self.shortcuts.len());
            self.offset.set(
                self.offset
                    .get()
                    .min(self.total.get().saturating_sub(self.visible.get())),
            );
            let end = self
                .offset
                .get()
                .saturating_add(self.visible.get())
                .min(self.total.get());
            let mut grid = vec![heading(
                format!(
                    "KEYBOARD SHORTCUTS   ·   {}–{} / {}",
                    if end == 0 { 0 } else { self.offset.get() + 1 },
                    end,
                    self.total.get()
                ),
                fg,
            )
            .margin(BoxDimension {
                bottom: Dimension::Pixels(8.),
                ..BoxDimension::default()
            })];
            let column_width = ((content_width - 46.) / columns as f32).max(1.);
            let mut entries = vec![];
            for chunk in self.shortcuts[self.offset.get()..end].chunks(columns) {
                let cells = chunk
                    .iter()
                    .map(|shortcut| {
                        let key_width = termwiz::cell::unicode_column_width(&shortcut.key, None)
                            as f32
                            * cell_height
                            * 0.6
                            + 24.;
                        let label_chars = ((column_width - key_width - 12.) / (cell_height * 0.55))
                            .floor()
                            .max(1.) as usize;
                        let labels = textwrap::wrap(&shortcut.label, label_chars);
                        let label = if labels.len() > 1 {
                            format!("{}…", labels[0])
                        } else {
                            shortcut.label.clone().into_owned()
                        };
                        let description =
                            text(label, muted)
                                .display(DisplayType::Inline)
                                .padding(BoxDimension {
                                    left: Dimension::Pixels(10.),
                                    right: Dimension::Pixels(0.),
                                    top: Dimension::Pixels(5.),
                                    bottom: Dimension::Pixels(5.),
                                });
                        Element::new(
                            &font,
                            ElementContent::Children(vec![
                                keycap(shortcut.key.clone()),
                                description,
                            ]),
                        )
                        .min_width(Some(Dimension::Pixels(column_width)))
                        .max_width(Some(Dimension::Pixels(column_width)))
                        .margin(BoxDimension {
                            bottom: Dimension::Pixels(6.),
                            ..BoxDimension::default()
                        })
                    })
                    .collect();
                entries.push(
                    Element::new(&font, ElementContent::Children(cells))
                        .display(DisplayType::Block),
                );
            }
            grid.push(style.viewport(
                &font,
                entries,
                content_width - 26.,
                grid_rows as f32 * row_height,
                crate::termwindow::menu_style::ScrollPosition {
                    offset: self.offset.get(),
                    visible: self.visible.get(),
                    total: self.total.get(),
                },
            ));
            rows.push(card(grid));
        } else {
            let mut wrapped = self.wrapped.borrow_mut();
            if wrapped.0 != chars {
                wrapped.0 = chars;
                wrapped.1 = wrap_document(&self.document.borrow(), chars);
            }
            self.visible.set(
                ((height - 9. * cell_height - 60.) / (cell_height + 10.))
                    .floor()
                    .max(1.) as usize,
            );
            self.total.set(wrapped.1.len());
            self.offset.set(
                self.offset
                    .get()
                    .min(self.total.get().saturating_sub(self.visible.get())),
            );
            let end = self
                .offset
                .get()
                .saturating_add(self.visible.get())
                .min(self.total.get());
            let mut body = vec![heading(
                format!(
                    "REFERENCE   ·   {}–{} / {}",
                    self.offset.get() + 1,
                    end,
                    self.total.get()
                ),
                muted,
            )
            .margin(BoxDimension {
                bottom: Dimension::Pixels(8.),
                ..BoxDimension::default()
            })];
            let mut entries = vec![];
            for line in &wrapped.1[self.offset.get()..end] {
                let source_start = line.source.start;
                let source_end = line.source.end;
                let mark = |element: Element, start: usize, end: usize| {
                    element.item_type(UIItemType::HelpText { start, end })
                };
                let entry = match line.style {
                    HelpLineStyle::Heading => {
                        mark(heading(line.text.clone(), fg), source_start, source_end)
                            .border(BoxDimension {
                                bottom: Dimension::Pixels(1.),
                                ..BoxDimension::default()
                            })
                            .colors(ElementColors {
                                border: BorderColor::new(edge),
                                text: fg.into(),
                                ..ElementColors::default()
                            })
                    }
                    HelpLineStyle::Value | HelpLineStyle::Binding => {
                        let separator = if matches!(line.style, HelpLineStyle::Binding) {
                            " - "
                        } else {
                            ":"
                        };
                        if let Some((label, value)) = line.text.split_once(separator) {
                            Element::new(
                                &font,
                                ElementContent::Children(vec![
                                    mark(
                                        heading(format!("{label}{separator}"), muted),
                                        source_start,
                                        source_start + label.len() + separator.len(),
                                    )
                                    .display(DisplayType::Inline)
                                    .vertical_align(VerticalAlign::Middle),
                                    mark(
                                        text(value.to_string(), fg),
                                        source_start + label.len() + separator.len(),
                                        source_end,
                                    )
                                    .display(DisplayType::Inline)
                                    .vertical_align(VerticalAlign::Middle),
                                ]),
                            )
                            .display(DisplayType::Block)
                            .colors(ElementColors {
                                bg: chip.into(),
                                ..ElementColors::default()
                            })
                        } else {
                            mark(text(line.text.clone(), fg), source_start, source_end)
                        }
                    }
                    HelpLineStyle::Code => {
                        mark(text(line.text.clone(), fg), source_start, source_end).colors(
                            ElementColors {
                                bg: chip.into(),
                                text: fg.into(),
                                ..ElementColors::default()
                            },
                        )
                    }
                    HelpLineStyle::Body => mark(
                        text(
                            if line.text.is_empty() {
                                " ".into()
                            } else {
                                line.text.clone()
                            },
                            muted,
                        ),
                        source_start,
                        source_end,
                    ),
                };
                entries.push(
                    Element::new(&font, ElementContent::Children(vec![entry]))
                        .display(DisplayType::Block)
                        .min_height(Some(Dimension::Pixels(cell_height + 10.))),
                );
            }
            let text_region = Element::new(&font, ElementContent::Children(entries))
                .display(DisplayType::Block)
                .min_width(Some(Dimension::Pixels((content_width - 46.).max(1.))))
                .min_height(Some(Dimension::Pixels(
                    self.visible.get() as f32 * (cell_height + 10.),
                )))
                .item_type(UIItemType::HelpTextRegion);
            body.push(style.viewport(
                &font,
                vec![text_region],
                content_width - 26.,
                self.visible.get() as f32 * (cell_height + 10.),
                crate::termwindow::menu_style::ScrollPosition {
                    offset: self.offset.get(),
                    visible: self.visible.get(),
                    total: self.total.get(),
                },
            ));
            rows.push(card(body));
        }
        if self.actions()[self.selected.get()] == HelpAction::Copy
            && self.text_selection.borrow().range().is_none()
        {
            self.move_focus(1);
        }
        let actions = self.actions();
        let action_card = |index: usize, topic: HelpTopic, card_width: f32| {
            Element::new(
                &font,
                ElementContent::Children(vec![
                    heading(topic.title().into(), fg),
                    text(
                        if topic == HelpTopic::DoubleCtrl {
                            format!(
                                "{}  ·  {}",
                                if self.double_ctrl {
                                    "Enabled"
                                } else {
                                    "Disabled"
                                },
                                topic.hint()
                            )
                        } else {
                            topic.hint().into()
                        },
                        muted,
                    ),
                ]),
            )
            .colors(if self.selected.get() == index {
                focus_colors.clone()
            } else {
                card_colors.clone()
            })
            .padding(BoxDimension::new(Dimension::Pixels(if columns > 1 {
                9.
            } else {
                6.
            })))
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .border_corners(Some(panel_corners()))
            .hover_colors(Some(focus_colors.clone()))
            .item_type(UIItemType::HelpMenuItem(HelpAction::Open(topic)))
            .min_width(Some(Dimension::Pixels(
                (card_width - if columns > 1 { 20. } else { 14. }).max(1.),
            )))
            .max_width(Some(Dimension::Pixels(card_width)))
            .margin(BoxDimension {
                right: Dimension::Pixels(8.),
                bottom: Dimension::Pixels(8.),
                ..BoxDimension::default()
            })
        };
        if self.page.get().is_none() {
            rows.push(heading("MORE GUIDES".into(), fg).margin(BoxDimension {
                bottom: Dimension::Pixels(6.),
                ..BoxDimension::default()
            }));
            let topic_columns = if columns > 1 { 2 } else { 1 };
            let card_width = (content_width / topic_columns as f32 - 8.).max(1.);
            for chunk in HelpTopic::ALL[..4].chunks(topic_columns) {
                let cards = chunk
                    .iter()
                    .map(|&topic| {
                        let index = HelpTopic::ALL.iter().position(|t| *t == topic).unwrap_or(0);
                        action_card(index, topic, card_width)
                    })
                    .collect();
                rows.push(
                    Element::new(&font, ElementContent::Children(cards))
                        .display(DisplayType::Block),
                );
            }
            rows.push(
                action_card(4, HelpTopic::DoubleCtrl, (content_width - 8.).max(1.))
                    .display(DisplayType::Block),
            );
        }
        let mut footer = vec![];
        for (index, &action) in actions.iter().enumerate() {
            if matches!(action, HelpAction::Open(_)) {
                continue;
            }
            let label = match action {
                HelpAction::Back => "Back",
                HelpAction::Close => "Close",
                HelpAction::Copy => "Copy",
                HelpAction::Open(_) => continue,
            };
            let mut button = Element::new(&font, ElementContent::Text(label.into()))
                .colors(if self.selected.get() == index {
                    focus_colors.clone()
                } else {
                    card_colors.clone()
                })
                .padding(BoxDimension {
                    left: Dimension::Pixels(12.),
                    right: Dimension::Pixels(12.),
                    top: Dimension::Pixels(6.),
                    bottom: Dimension::Pixels(6.),
                })
                .border(BoxDimension::new(Dimension::Pixels(1.)))
                .border_corners(Some(panel_corners()))
                .margin(BoxDimension {
                    right: Dimension::Pixels(8.),
                    ..BoxDimension::default()
                });
            button.item_type = Some(UIItemType::HelpMenuItem(action));
            button.hover_colors = Some(focus_colors.clone());
            if action == HelpAction::Copy && self.text_selection.borrow().range().is_none() {
                button.item_type = None;
                button.hover_colors = None;
                button.colors.text = muted.into();
            }
            footer.push(button);
        }
        rows.push(
            Element::new(&font, ElementContent::Children(footer))
                .display(DisplayType::Block)
                .margin(BoxDimension {
                    top: Dimension::Pixels(4.),
                    bottom: Dimension::Pixels(6.),
                    ..BoxDimension::default()
                }),
        );
        rows.push(text(
            if self.page.get().is_some() {
                "Drag to select · Ctrl+A: all · Ctrl+C: copy"
            } else {
                "Arrows / Tab · Enter · PgUp / PgDn · Esc / F1"
            }
            .into(),
            muted,
        ));
        let root = style.panel(&font, rows, width);
        let mut computed = window.compute_element(
            &LayoutContext {
                height: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: height,
                    pixel_cell: cell_height,
                },
                width: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: width,
                    pixel_cell: cell_width,
                },
                bounds: euclid::rect((width_limit - width) / 2., 0., width, height),
                metrics: &metrics,
                gl_state: window.render_state.as_ref().unwrap(),
                zindex: 100,
            },
            &root,
        )?;
        computed.translate(euclid::vec2(
            0.,
            ((height_limit - computed.bounds.height()) / 2.).max(0.),
        ));
        if let Some(range) = self.text_selection.borrow().range() {
            let palette = window.palette();
            let colors = ElementColors {
                bg: palette
                    .selection_bg
                    .to_linear()
                    .when_fully_transparent(style.chip)
                    .into(),
                text: palette
                    .selection_fg
                    .to_linear()
                    .when_fully_transparent(style.foreground)
                    .into(),
                ..ElementColors::default()
            };
            selection::apply_selection(&mut computed, &self.document.borrow(), &range, &colors);
        }
        Ok(vec![computed])
    }
}

impl Modal for HelpMenu {
    fn perform_assignment(&self, assignment: &KeyAssignment, window: &mut TermWindow) -> bool {
        use onlyterm_config::keyassignment::ClipboardCopyDestination;
        match assignment {
            KeyAssignment::CopySelectionOrInterrupt => {
                self.copy_selection(window, ClipboardCopyDestination::Clipboard)
            }
            KeyAssignment::CopyTo(destination) => self.copy_selection(window, *destination),
            _ => return false,
        }
        true
    }

    fn scroll_position(&self) -> Option<crate::termwindow::menu_style::ScrollPosition> {
        Some(crate::termwindow::menu_style::ScrollPosition {
            offset: self.offset.get(),
            visible: self.visible.get(),
            total: self.total.get(),
        })
    }

    fn set_scroll_offset(&self, offset: usize, window: &mut TermWindow) {
        self.offset
            .set(offset.min(self.total.get().saturating_sub(self.visible.get())));
        self.invalidate(window);
    }

    fn blocks_terminal_input(&self) -> bool {
        true
    }
    fn mouse_event(&self, _event: MouseEvent, _window: &mut TermWindow) -> anyhow::Result<()> {
        Ok(())
    }
    fn key_down(
        &self,
        key: KeyCode,
        mods: KeyModifiers,
        window: &mut TermWindow,
    ) -> anyhow::Result<bool> {
        match (key, mods) {
            (KeyCode::Escape | KeyCode::Function(1), _) => window.cancel_modal(),
            (KeyCode::Char('c' | 'C'), KeyModifiers::CTRL) if self.page.get().is_some() => {
                self.copy_selection(
                    window,
                    onlyterm_config::keyassignment::ClipboardCopyDestination::Clipboard,
                );
            }
            (KeyCode::Char('a' | 'A'), KeyModifiers::CTRL) if self.page.get().is_some() => {
                self.text_selection.replace(TextSelection {
                    anchor: Some(0),
                    focus: self.document.borrow().len(),
                    dragging: false,
                });
                self.invalidate(window);
            }
            (KeyCode::UpArrow | KeyCode::DownArrow, KeyModifiers::NONE)
                if self.page.get().is_some() =>
            {
                let offset = if key == KeyCode::UpArrow {
                    self.offset.get().saturating_sub(1)
                } else {
                    self.offset.get().saturating_add(1)
                };
                self.set_scroll_offset(offset, window);
            }
            (KeyCode::UpArrow | KeyCode::LeftArrow, KeyModifiers::NONE)
            | (KeyCode::Tab, KeyModifiers::SHIFT) => {
                self.move_focus(-1);
                self.invalidate(window);
            }
            (KeyCode::DownArrow | KeyCode::RightArrow | KeyCode::Tab, KeyModifiers::NONE) => {
                self.move_focus(1);
                self.invalidate(window);
            }
            (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE) => {
                self.perform_action(self.actions()[self.selected.get()], window)
            }
            (KeyCode::PageUp, KeyModifiers::NONE) => self.scroll(-1, window),
            (KeyCode::PageDown, KeyModifiers::NONE) => self.scroll(1, window),
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.offset.set(0);
                self.invalidate(window);
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.offset
                    .set(self.total.get().saturating_sub(self.visible.get()));
                self.invalidate(window);
            }
            (KeyCode::Backspace, KeyModifiers::NONE) if self.page.get().is_some() => {
                self.perform_action(HelpAction::Back, window)
            }
            _ => {}
        }
        Ok(true)
    }
    fn computed_element(
        &self,
        window: &mut TermWindow,
    ) -> anyhow::Result<Ref<'_, [ComputedElement]>> {
        if self.elements.borrow().is_none() {
            *self.elements.borrow_mut() = Some(self.compute(window)?);
        }
        Ok(Ref::map(self.elements.borrow(), |e| {
            e.as_ref().unwrap().as_slice()
        }))
    }
    fn reconfigure(&self, window: &mut TermWindow) {
        self.invalidate(window);
    }
}

const SETTINGS_HELP: &str = "OnlyTerm uses a static ktav configuration file, not Lua or rhai. The usual location is %USERPROFILE%/.onlyterm.ktav. Ctrl+O opens the selected config path; missing files get a commented starter config.\n\nThe loaded file is watched for changes. Ctrl+Shift+R reloads it manually. Most settings update immediately; initial window geometry applies to newly created windows.\n\nCommon settings:\nfont_size: 12.0\ninitial_cols: 120\ninitial_rows: 28\ndefault_prog: [powershell.exe, -NoLogo]\ndefault_cwd: C:/work\n\nfont sets font families; color_scheme selects a theme. keys defines custom shortcuts; disable_default_key_bindings: true removes built-in bindings.\n\nExample key binding:\nkeys: [\n  { key: F1, mods: NONE, action: ActivateHelpMenu }\n  { key: t, mods: CTRL|ALT, action: ActivateNewTabOptions }\n]\n\nktav values are not quoted. Lines starting with ## are comments. --config-file selects a different main config; ONLYTERM_CONFIG_FILE can also select it. --skip-config ignores configuration files. Command-line --config overrides remain in force after reload.";

const LAUNCH_HELP: &str = "Start a program instead of the default shell:\nonlyterm start -- powershell.exe -NoLogo\n\nSet the initial working directory:\nonlyterm start --cwd C:/work\n\nOpen New Tab Options at startup:\nonlyterm start --choose-tab --cwd C:/work\nCanceling this startup chooser exits its empty window. Ctrl+Alt+T or right-clicking the plus button opens the same shell/elevation/priority chooser inside an existing window; canceling that dialog keeps the window open.\n\nGlobal options go before start:\n--config-file PATH: choose the main ktav config.\n--skip-config: use built-in defaults.\n--config key=value: override a setting; may be repeated.\nExample: onlyterm --config font_size=14 start\n\nStart options:\n--start-conf PATH: load a startup tab layout.\n--cwd PATH: initial directory; conflicts with --start-conf.\n--choose-tab: choose initial tab options; conflicts with PROG and --start-conf.\n--workspace NAME: select a workspace.\n--class NAME: set the window class.\n--position X,Y: set the initial window position.\n--domain NAME: select a configured mux domain.\n--attach: attach without spawning another program when the domain already has panes.\n--no-auto-connect: skip domains marked connect_automatically.\n--always-new-process: request a separate GUI process.\n--new-tab: request a tab when reusing a GUI; this fork starts a separate GUI for start invocations.\n\nUse onlyterm start --help for the complete CLI reference. Available shells must already be installed; choosing admin can show a Windows UAC prompt.";

const CONFIG_LAUNCH_HELP: &str = "There are two different configuration mechanisms.\n\n1. Main .onlyterm.ktav config:\ndefault_prog selects the default program and arguments; default_cwd sets its fallback directory. set_environment_variables adds environment variables to spawned programs.\n\nlaunch_menu defines named commands for the launcher / command palette:\nlaunch_menu: [\n  { label: Project, args: [powershell.exe, -NoLogo], cwd: C:/work }\n]\nCtrl+Shift+P opens the command palette, which includes these configured launches. A ShowLauncherArgs key assignment can also open the launcher.\n\n2. A standalone startup layout:\nonlyterm start --start-conf project.ktav\n\nExample project.ktav:\nroot_dir: C:/work\nshell: powershell\npriority: Normal\nadmin: false\nvars: { PROJECT: demo }\ncommands: [echo Starting project]\ntabs: [\n  { title: Editor }\n  { title: Server, commands: [npm run dev] }\n]\n\nAt least one tab is required. Tabs open in one new window, in listed order; the first tab is active when startup completes.\n\nLayout fields: root_dir, shell, priority, admin, vars, commands, tabs. Each tab supports title plus the same fields except tabs. Per-tab root_dir/shell/priority/admin overrides layout defaults. vars merge per key with tab values winning. Global commands run before per-tab commands.\n\nRelative root_dir paths are relative to the layout file, not the current launch directory. This layout is parsed separately, not merged into the main config.\n\nCommands are queued as shell input, not executed by the ktav parser. There is no prompt-readiness detection; interactive programs can consume later commands, so put them last. Elevated tabs may show individual UAC prompts; their custom vars cannot be passed through ShellExecute.\n\n--start-conf conflicts with an explicit program, --cwd and --choose-tab. Never use an untrusted layout: commands can execute arbitrary programs.";

const DOUBLE_CTRL_HELP: &str = "Tap and release Ctrl twice quickly, without another key in either tap. Each tap must finish within the tap limit shown above; the second press must begin within the interval after the first release.\n\nThis arms a one-shot pass-through mode. The next non-modifier key press, including a modifier chord, goes to the terminal application without OnlyTerm shortcut interception. Held-key repeats keep bypassing until release. Modifier keys alone do not spend the mode.\n\nFor example: double Ctrl, then Ctrl+W sends Ctrl+W to the program instead of closing the tab. This does not insert literal text or disable the terminal's keyboard protocol.\n\nThe tab bar shows an accent rim while armed. Double Ctrl again cancels the mode. Losing window focus also cancels it. When armed inside a blocking graphical menu, that menu closes so the next key can reach the terminal.\n\nControl this behavior in .onlyterm.ktav:\npass_through_next_key_on_double_ctrl: true\nSet false to disable the gesture. It is separate from the configurable LEADER key.";

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> HelpMenu {
        HelpMenu {
            shortcuts: vec![],
            config_path: String::new(),
            defaults_disabled: false,
            double_ctrl: true,
            tables: vec![],
            page: Cell::new(None),
            selected: Cell::new(0),
            offset: Cell::new(0),
            main_offset: Cell::new(0),
            visible: Cell::new(10),
            total: Cell::new(0),
            document: RefCell::new(String::new()),
            wrapped: RefCell::new((0, vec![])),
            elements: RefCell::new(None),
            text_selection: RefCell::new(TextSelection::default()),
        }
    }

    #[test]
    fn navigation_wraps_between_guides_and_dialog_buttons() {
        let menu = menu();
        menu.move_focus(-1);
        assert_eq!(menu.actions()[menu.selected.get()], HelpAction::Close);
        menu.move_focus(-1);
        assert_eq!(
            menu.actions()[menu.selected.get()],
            HelpAction::Open(HelpTopic::DoubleCtrl)
        );
        menu.page.set(Some(HelpTopic::Settings));
        menu.selected.set(1);
        menu.move_focus(1);
        assert_eq!(menu.actions()[menu.selected.get()], HelpAction::Close);
        menu.move_focus(1);
        assert_eq!(menu.actions()[menu.selected.get()], HelpAction::Back);
    }
}
