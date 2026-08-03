use crate::termwindow::{PaneInformation, TabInformation, UIItem, UIItemType};
use config::{ConfigHandle, TabBarColors};
use finl_unicode::grapheme_clusters::Graphemes;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use termwiz::cell::{unicode_column_width, Cell, CellAttributes};
use termwiz::color::{AnsiColor, ColorSpec};
use termwiz::escape::csi::Sgr;
use termwiz::escape::parser::Parser;
use termwiz::escape::{Action, ControlCode, CSI};
use termwiz::surface::SEQ_ZERO;
use termwiz_funcs::{format_as_escapes, FormatColor, FormatItem};
use wezterm_term::{Line, Progress};
use window::parameters::Parameters;
use window::{IntegratedTitleButton, IntegratedTitleButtonAlignment, IntegratedTitleButtonStyle};

#[derive(Clone, Debug, PartialEq)]
pub struct TabBarState {
    line: Line,
    items: Vec<TabEntry>,
    /// When a tab shows the built-in indeterminate progress spinner, the instant
    /// at which the tab bar should be rebuilt to advance to the next frame;
    /// None when no spinner is visible.
    next_progress_frame_due: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabBarItem {
    None,
    LeftStatus,
    RightStatus,
    Tab { tab_idx: usize, active: bool },
    NewTabButton,
    WindowButton(IntegratedTitleButton),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabEntry {
    pub item: TabBarItem,
    pub title: Line,
    x: usize,
    width: usize,
}

#[derive(Clone, Debug)]
struct TitleText {
    items: Vec<FormatItem>,
    len: usize,
    /// True when the built-in title path rendered an indeterminate spinner
    /// frame. A custom format-tab-title callback always leaves this false.
    has_indeterminate: bool,
}

/// pct is a percentage in the range 0-100.
/// We want to map it to one of the nerdfonts:
///
/// * `md-checkbox_blank_circle_outline` (0xf0130) for an empty circle
/// * `md_circle_slice_1..=7` (0xf0a9e ..= 0xf0aa4) for a partly filled
///   circle
/// * `md_circle_slice_8` (0xf0aa5) for a filled circle
///
/// We use an empty circle for values close to 0%, a filled circle for values
/// close to 100%, and a partly filled circle for the rest (roughly evenly
/// distributed).
fn pct_to_glyph(pct: u8) -> char {
    match pct {
        0..=5 => '\u{f0130}',    // empty circle
        6..=18 => '\u{f0a9e}',   // centered at 12 (slightly smaller than 12.5)
        19..=31 => '\u{f0a9f}',  // centered at 25
        32..=43 => '\u{f0aa0}',  // centered at 37.5
        44..=56 => '\u{f0aa1}',  // half-filled circle, centered at 50
        57..=68 => '\u{f0aa2}',  // centered at 62.5
        69..=81 => '\u{f0aa3}',  // centered at 75
        82..=94 => '\u{f0aa4}',  // centered at 88 (slightly larger than 87.5)
        95..=100 => '\u{f0aa5}', // filled circle
        // Any other value is mapped to a filled circle.
        _ => '\u{f0aa5}',
    }
}

/// How long each indeterminate progress spinner frame is shown before the next
/// is due.
const INDETERMINATE_SPINNER_INTERVAL: Duration = Duration::from_millis(100);

/// Reference instant for the indeterminate spinner. The displayed frame and its
/// next-due time are both derived from the elapsed time since this instant, so
/// they stay in step no matter when a repaint happens to rebuild the tab bar.
static SPINNER_EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Renders `value` as a braille cell whose lit dots count up in the cell's
/// reading order, down the left column then down the right column. Stepping
/// `value` through 0..=255 reproduces the `dots8Bit` animation of
/// https://github.com/sindresorhus/cli-spinners without a lookup table.
fn braille_counter(value: u8) -> char {
    // Unicode braille dot bit values in reading order: dots 1, 2, 3, 7 fill the
    // left column and dots 4, 5, 6, 8 the right column.
    const DOTS: [u32; 8] = [0x01, 0x02, 0x04, 0x40, 0x08, 0x10, 0x20, 0x80];
    let mut pattern = 0u32;
    for (bit, dot) in DOTS.iter().enumerate() {
        if value & (1 << bit) != 0 {
            pattern |= dot;
        }
    }
    char::from_u32(0x2800 + pattern).expect("braille pattern is a valid codepoint")
}

/// Returns the spinner glyph to show for the current moment, advancing one
/// frame per INDETERMINATE_SPINNER_INTERVAL. `seed` offsets the starting frame
/// so that tabs busy at the same time do not animate in lock step.
fn indeterminate_spinner_glyph(seed: u64) -> char {
    let elapsed = SPINNER_EPOCH.elapsed().as_millis() as u64;
    let interval = INDETERMINATE_SPINNER_INTERVAL.as_millis() as u64;
    // braille_counter wraps at 256, matching the animation's frame count.
    braille_counter((elapsed / interval + seed) as u8)
}

/// Returns the instant at which the spinner next advances a frame, snapped to
/// the frame grid measured from SPINNER_EPOCH. Because the result falls on a
/// grid boundary rather than a fixed offset from now, repeated rebuilds within
/// one frame all return the same instant and the animation advances steadily
/// even when unrelated repaints rebuild the tab bar in between.
fn next_spinner_frame_due() -> Instant {
    let interval = INDETERMINATE_SPINNER_INTERVAL.as_nanos();
    let elapsed = SPINNER_EPOCH.elapsed().as_nanos();
    let next_frame = elapsed / interval + 1;
    *SPINNER_EPOCH + Duration::from_nanos((next_frame * interval) as u64)
}

/// Scrambles a tab id into a spinner phase offset. Tab ids are usually handed
/// out sequentially, which would leave adjacent tabs only one frame apart; the
/// splitmix64 finalizer avalanches the low bits so their spinners spread across
/// the animation instead.
fn spinner_phase(tab_id: usize) -> u64 {
    let mut z = tab_id as u64;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// Extracts the last path component (basename) from a `file://`-style URL
/// string (as produced by `Pane::get_current_working_dir`, e.g.
/// `file:///home/user/project` on Unix or `file:///C:/Users/foo/bar` on
/// Windows) or, failing that, from a plain filesystem path string.
///
/// Trailing slashes are ignored. If the path has no component to strip (for
/// example the Unix root `/`, a bare Windows drive root `C:/`, or an empty
/// string), the original `path` is returned unchanged so callers always get
/// a non-empty, human-meaningful string instead of an empty title.
pub(crate) fn basename_of_path(path: &str) -> String {
    // Prefer proper URL parsing so that percent-encoded characters and the
    // `file://` scheme/host are stripped correctly. `Url::to_file_path()`
    // fails for things like a bare root (`file:///`) that don't resolve to
    // a valid platform path, so fall back to inspecting the URL's decoded
    // `path()` component directly rather than re-parsing the whole URL
    // string as if it were a raw filesystem path.
    if let Ok(url) = url::Url::parse(path) {
        if let Ok(file_path) = url.to_file_path() {
            if let Some(name) = file_path.file_name().and_then(|n| n.to_str()) {
                return name.to_string();
            }
            return path.to_string();
        }

        let trimmed = url.path().trim_end_matches('/');
        return match std::path::Path::new(trimmed)
            .file_name()
            .and_then(|n| n.to_str())
        {
            Some(name) => name.to_string(),
            None => path.to_string(),
        };
    }

    // Not a URL at all; treat it as a plain filesystem path.
    let trimmed = path.trim_end_matches(['/', '\\']);
    match std::path::Path::new(trimmed)
        .file_name()
        .and_then(|n| n.to_str())
    {
        Some(name) => name.to_string(),
        None => path.to_string(),
    }
}

fn compute_tab_title(tab: &TabInformation, config: &ConfigHandle) -> TitleText {
    let mut items = vec![];
    let mut len = 0;
    let mut has_indeterminate = false;

    if let Some(pane) = &tab.active_pane {
        let mut title = if !tab.tab_title.is_empty() {
            // An explicitly assigned tab title (via `wezterm cli
            // set-tab-title` or similar) always wins.
            tab.tab_title.clone()
        } else if config.use_cwd_basename_as_tab_title {
            // Fall back to the cwd basename when requested and
            // available; otherwise fall back further to the pane's
            // own title (usually the foreground process name).
            match &pane.current_working_dir {
                Some(cwd) if !cwd.is_empty() => basename_of_path(cwd),
                _ => pane.title.clone(),
            }
        } else {
            pane.title.clone()
        };

        let classic_spacing = if config.use_fancy_tab_bar { "" } else { " " };
        if config.show_tab_index_in_tab_bar {
            let index = format!(
                "{classic_spacing}{}: ",
                tab.tab_index
                    + if config.tab_and_split_indices_are_zero_based {
                        0
                    } else {
                        1
                    }
            );
            len += unicode_column_width(&index, None);
            items.push(FormatItem::Text(index));

            title = format!("{}{classic_spacing}", title);
        }

        match pane.progress {
            Progress::None => {}
            Progress::Percentage(pct) | Progress::Error(pct) => {
                let graphic = format!("{} ", pct_to_glyph(pct));
                len += unicode_column_width(&graphic, None);
                let color = if matches!(pane.progress, Progress::Percentage(_)) {
                    FormatItem::Foreground(FormatColor::AnsiColor(AnsiColor::Green))
                } else {
                    FormatItem::Foreground(FormatColor::AnsiColor(AnsiColor::Red))
                };
                items.push(color);
                items.push(FormatItem::Text(graphic));
                items.push(FormatItem::Foreground(FormatColor::Default));
            }
            Progress::Indeterminate => {
                has_indeterminate = true;
                let graphic = format!(
                    "{} ",
                    indeterminate_spinner_glyph(spinner_phase(tab.tab_id))
                );
                len += unicode_column_width(&graphic, None);
                items.push(FormatItem::Foreground(FormatColor::AnsiColor(
                    AnsiColor::Green,
                )));
                items.push(FormatItem::Text(graphic));
                items.push(FormatItem::Foreground(FormatColor::Default));
            }
        }

        // We have a preferred soft minimum on tab width to make it
        // easier to click on tab titles, but we'll still go below
        // this if there are too many tabs to fit the window at
        // this width.
        if !config.use_fancy_tab_bar {
            while len + unicode_column_width(&title, None) < 5 {
                title.push(' ');
            }
        }

        len += unicode_column_width(&title, None);
        items.push(FormatItem::Text(title));
    } else {
        let title = " no pane ".to_string();
        len += unicode_column_width(&title, None);
        items.push(FormatItem::Text(title));
    };

    TitleText {
        len,
        items,
        has_indeterminate,
    }
}

fn is_tab_hover(mouse_x: Option<usize>, x: usize, tab_title_len: usize) -> bool {
    mouse_x
        .map(|mouse_x| mouse_x >= x && mouse_x < x + tab_title_len)
        .unwrap_or(false)
}

impl TabBarState {
    pub fn default() -> Self {
        Self {
            line: Line::with_width(1, SEQ_ZERO),
            items: vec![TabEntry {
                item: TabBarItem::None,
                title: Line::from_text(" ", &CellAttributes::blank(), 1, None),
                x: 1,
                width: 1,
            }],
            next_progress_frame_due: None,
        }
    }

    pub fn line(&self) -> &Line {
        &self.line
    }

    pub fn items(&self) -> &[TabEntry] {
        &self.items
    }

    pub fn next_progress_frame_due(&self) -> Option<Instant> {
        self.next_progress_frame_due
    }

    fn integrated_title_buttons(
        mouse_x: Option<usize>,
        x: &mut usize,
        config: &ConfigHandle,
        items: &mut Vec<TabEntry>,
        line: &mut Line,
        colors: &TabBarColors,
    ) {
        let default_cell = if config.use_fancy_tab_bar {
            CellAttributes::default()
        } else {
            colors.new_tab().as_cell_attributes()
        };

        let default_cell_hover = if config.use_fancy_tab_bar {
            CellAttributes::default()
        } else {
            colors.new_tab_hover().as_cell_attributes()
        };

        let window_hide =
            parse_status_text(&config.tab_bar_style.window_hide, default_cell.clone());
        let window_hide_hover = parse_status_text(
            &config.tab_bar_style.window_hide_hover,
            default_cell_hover.clone(),
        );

        let window_maximize =
            parse_status_text(&config.tab_bar_style.window_maximize, default_cell.clone());
        let window_maximize_hover = parse_status_text(
            &config.tab_bar_style.window_maximize_hover,
            default_cell_hover.clone(),
        );

        let window_close =
            parse_status_text(&config.tab_bar_style.window_close, default_cell.clone());
        let window_close_hover = parse_status_text(
            &config.tab_bar_style.window_close_hover,
            default_cell_hover.clone(),
        );

        for button in &config.integrated_title_buttons {
            use IntegratedTitleButton as Button;
            let title = match button {
                Button::Hide => {
                    let hover = is_tab_hover(mouse_x, *x, window_hide_hover.len());

                    if hover {
                        &window_hide_hover
                    } else {
                        &window_hide
                    }
                }
                Button::Maximize => {
                    let hover = is_tab_hover(mouse_x, *x, window_maximize_hover.len());

                    if hover {
                        &window_maximize_hover
                    } else {
                        &window_maximize
                    }
                }
                Button::Close => {
                    let hover = is_tab_hover(mouse_x, *x, window_close_hover.len());

                    if hover {
                        &window_close_hover
                    } else {
                        &window_close
                    }
                }
            };

            line.append_line(title.to_owned(), SEQ_ZERO);

            let width = title.len();
            items.push(TabEntry {
                item: TabBarItem::WindowButton(*button),
                title: title.to_owned(),
                x: *x,
                width,
            });

            *x += width;
        }
    }

    /// Build a new tab bar from the current state
    /// mouse_x is some if the mouse is on the same row as the tab bar.
    /// title_width is the total number of cell columns in the window.
    /// window allows access to the tabs associated with the window.
    #[allow(clippy::too_many_arguments)] // tab-bar layout: params are the inherent title/mouse/tab context
    pub fn new(
        title_width: usize,
        mouse_x: Option<usize>,
        tab_info: &[TabInformation],
        _pane_info: &[PaneInformation],
        colors: Option<&TabBarColors>,
        config: &ConfigHandle,
        left_status: &str,
        right_status: &str,
        cell_width: f32,
        os_parameters: Option<&Parameters>,
    ) -> Self {
        let colors = colors.cloned().unwrap_or_else(TabBarColors::default);

        let active_cell_attrs = colors.active_tab().as_cell_attributes();
        let inactive_hover_attrs = colors.inactive_tab_hover().as_cell_attributes();
        let inactive_cell_attrs = colors.inactive_tab().as_cell_attributes();
        let new_tab_hover_attrs = colors.new_tab_hover().as_cell_attributes();
        let new_tab_attrs = colors.new_tab().as_cell_attributes();

        let new_tab = parse_status_text(
            &config.tab_bar_style.new_tab,
            if config.use_fancy_tab_bar {
                CellAttributes::default()
            } else {
                new_tab_attrs.clone()
            },
        );
        let new_tab_hover = parse_status_text(
            &config.tab_bar_style.new_tab_hover,
            if config.use_fancy_tab_bar {
                CellAttributes::default()
            } else {
                new_tab_hover_attrs.clone()
            },
        );

        let use_integrated_title_buttons = config
            .window_decorations
            .contains(window::WindowDecorations::INTEGRATED_BUTTONS);

        // We ultimately want to produce a line looking like this:
        // ` | tab1-title x | tab2-title x |  +      . - X `
        // Where the `+` sign will spawn a new tab (or show a context
        // menu with tab creation options) and the other three chars
        // are symbols representing minimize, maximize and close.

        let mut active_tab_no = 0;

        let tab_titles: Vec<TitleText> = if config.show_tabs_in_tab_bar {
            tab_info
                .iter()
                .map(|tab| {
                    if tab.is_active {
                        active_tab_no = tab.tab_index;
                    }
                    compute_tab_title(tab, config)
                })
                .collect()
        } else {
            vec![]
        };
        let titles_len: usize = tab_titles.iter().map(|s| s.len).sum();
        let number_of_tabs = tab_titles.len();

        let available_cells =
            title_width.saturating_sub(number_of_tabs.saturating_sub(1) + new_tab.len());
        let tab_width_max = if config.use_fancy_tab_bar || available_cells >= titles_len {
            // We can render each title with its full width
            usize::MAX
        } else {
            // We need to clamp the length to balance them out
            available_cells / number_of_tabs
        }
        .min(config.tab_max_width);

        let mut line = Line::with_width(0, SEQ_ZERO);

        let mut x = 0;
        let mut items = vec![];
        let mut has_indeterminate_progress = false;

        let black_cell = Cell::blank_with_attrs(
            CellAttributes::default()
                .set_background(ColorSpec::TrueColor(*colors.background()))
                .clone(),
        );

        if use_integrated_title_buttons
            && config.integrated_title_button_style == IntegratedTitleButtonStyle::MacOsNative
            && !config.use_fancy_tab_bar
            && !config.tab_bar_at_bottom
        {
            let num_padding_cells = Self::compute_num_padding_cells(cell_width, os_parameters);
            for _ in 0..num_padding_cells {
                line.insert_cell(0, black_cell.clone(), title_width, SEQ_ZERO);
                x += 1;
            }
        }

        if use_integrated_title_buttons
            && config.integrated_title_button_style != IntegratedTitleButtonStyle::MacOsNative
            && config.integrated_title_button_alignment == IntegratedTitleButtonAlignment::Left
        {
            Self::integrated_title_buttons(mouse_x, &mut x, config, &mut items, &mut line, &colors);
        }

        let left_status_line = parse_status_text(left_status, black_cell.attrs().clone());
        if left_status_line.len() > 0 {
            items.push(TabEntry {
                item: TabBarItem::LeftStatus,
                title: left_status_line.clone(),
                x,
                width: left_status_line.len(),
            });
            x += left_status_line.len();
            line.append_line(left_status_line, SEQ_ZERO);
        }

        for (tab_idx, tab_title) in tab_titles.iter().enumerate() {
            let tab_title_len = tab_title.len.min(tab_width_max);
            let active = tab_idx == active_tab_no;
            let hover = !active && is_tab_hover(mouse_x, x, tab_title_len);

            let cell_attrs = if active {
                &active_cell_attrs
            } else if hover {
                &inactive_hover_attrs
            } else {
                &inactive_cell_attrs
            };

            let tab_start_idx = x;

            has_indeterminate_progress |= tab_title.has_indeterminate;

            let esc = format_as_escapes(tab_title.items.clone()).expect("already parsed ok above");
            let mut tab_line = parse_status_text(
                &esc,
                if config.use_fancy_tab_bar {
                    CellAttributes::default()
                } else {
                    cell_attrs.clone()
                },
            );

            let title = tab_line.clone();
            if tab_line.len() > tab_width_max {
                tab_line.resize(tab_width_max, SEQ_ZERO);
            }

            let width = tab_line.len();

            items.push(TabEntry {
                item: TabBarItem::Tab { tab_idx, active },
                title,
                x: tab_start_idx,
                width,
            });

            line.append_line(tab_line, SEQ_ZERO);
            x += width;
        }

        // New tab button
        if config.show_new_tab_button_in_tab_bar {
            let hover = is_tab_hover(mouse_x, x, new_tab_hover.len());

            let new_tab_button = if hover { &new_tab_hover } else { &new_tab };

            let button_start = x;
            let width = new_tab_button.len();

            line.append_line(new_tab_button.clone(), SEQ_ZERO);

            items.push(TabEntry {
                item: TabBarItem::NewTabButton,
                title: new_tab_button.clone(),
                x: button_start,
                width,
            });

            x += width;
        }

        // Reserve place for integrated title buttons
        let title_width = if use_integrated_title_buttons
            && config.integrated_title_button_style != IntegratedTitleButtonStyle::MacOsNative
            && config.integrated_title_button_alignment == IntegratedTitleButtonAlignment::Right
        {
            let window_hide =
                parse_status_text(&config.tab_bar_style.window_hide, CellAttributes::default());
            let window_hide_hover = parse_status_text(
                &config.tab_bar_style.window_hide_hover,
                CellAttributes::default(),
            );

            let window_maximize = parse_status_text(
                &config.tab_bar_style.window_maximize,
                CellAttributes::default(),
            );
            let window_maximize_hover = parse_status_text(
                &config.tab_bar_style.window_maximize_hover,
                CellAttributes::default(),
            );
            let window_close = parse_status_text(
                &config.tab_bar_style.window_close,
                CellAttributes::default(),
            );
            let window_close_hover = parse_status_text(
                &config.tab_bar_style.window_close_hover,
                CellAttributes::default(),
            );

            let hide_len = window_hide.len().max(window_hide_hover.len());
            let maximize_len = window_maximize.len().max(window_maximize_hover.len());
            let close_len = window_close.len().max(window_close_hover.len());

            let mut width_to_reserve = 0;
            for button in &config.integrated_title_buttons {
                use IntegratedTitleButton as Button;
                let button_len = match button {
                    Button::Hide => hide_len,
                    Button::Maximize => maximize_len,
                    Button::Close => close_len,
                };
                width_to_reserve += button_len;
            }

            title_width.saturating_sub(width_to_reserve)
        } else {
            title_width
        };

        let status_space_available = title_width.saturating_sub(x);

        let mut right_status_line = parse_status_text(right_status, black_cell.attrs().clone());
        items.push(TabEntry {
            item: TabBarItem::RightStatus,
            title: right_status_line.clone(),
            x,
            width: status_space_available,
        });

        while right_status_line.len() > status_space_available {
            right_status_line.remove_cell(0, SEQ_ZERO);
        }

        line.append_line(right_status_line, SEQ_ZERO);
        while line.len() < title_width {
            line.insert_cell(x, black_cell.clone(), title_width, SEQ_ZERO);
        }

        if use_integrated_title_buttons
            && config.integrated_title_button_style != IntegratedTitleButtonStyle::MacOsNative
            && config.integrated_title_button_alignment == IntegratedTitleButtonAlignment::Right
        {
            x = title_width;
            Self::integrated_title_buttons(mouse_x, &mut x, config, &mut items, &mut line, &colors);
        }

        Self {
            line,
            items,
            next_progress_frame_due: has_indeterminate_progress.then(next_spinner_frame_due),
        }
    }

    pub fn compute_ui_items(&self, y: usize, cell_height: usize, cell_width: usize) -> Vec<UIItem> {
        let mut items = vec![];

        for entry in self.items.iter() {
            items.push(UIItem {
                x: entry.x * cell_width,
                width: entry.width * cell_width,
                y,
                height: cell_height,
                item_type: UIItemType::TabBar(entry.item),
            });
        }

        items
    }

    fn compute_num_padding_cells(cell_width: f32, os_parameters: Option<&Parameters>) -> usize {
        let left_pixel_margin = os_parameters
            .map(|p| p.title_bar.padding_left.get() as f32)
            .unwrap_or(0.0);
        let extra_padding_in_pixels = 0.5 * cell_width;
        let total_pixel_margin = left_pixel_margin + extra_padding_in_pixels;
        if cell_width > 0.0 {
            (total_pixel_margin / cell_width).ceil() as usize
        } else {
            10
        }
    }
}

pub fn parse_status_text(text: &str, default_cell: CellAttributes) -> Line {
    let mut pen = default_cell.clone();
    let mut cells = vec![];
    let mut ignoring = false;
    let mut print_buffer = String::new();

    fn flush_print(buf: &mut String, cells: &mut Vec<Cell>, pen: &CellAttributes) {
        for g in Graphemes::new(buf.as_str()) {
            let cell = Cell::new_grapheme(g, pen.clone(), None);
            let width = cell.width();
            cells.push(cell);
            for _ in 1..width {
                // Line/Screen expect double wide graphemes to be followed by a blank in
                // the next column position, otherwise we'll render incorrectly
                cells.push(Cell::blank_with_attrs(pen.clone()));
            }
        }
        buf.clear();
    }

    let mut parser = Parser::new();
    parser.parse(text.as_bytes(), |action| {
        if ignoring {
            return;
        }
        match action {
            Action::Print(c) => print_buffer.push(c),
            Action::PrintString(s) => print_buffer.push_str(&s),
            Action::Control(c) => {
                flush_print(&mut print_buffer, &mut cells, &pen);
                match c {
                    ControlCode::CarriageReturn | ControlCode::LineFeed => {
                        ignoring = true;
                    }
                    _ => {}
                }
            }
            Action::CSI(csi) => {
                flush_print(&mut print_buffer, &mut cells, &pen);
                if let CSI::Sgr(sgr) = csi {
                    match sgr {
                        Sgr::Reset => pen = default_cell.clone(),
                        Sgr::Intensity(i) => {
                            pen.set_intensity(i);
                        }
                        Sgr::Underline(u) => {
                            pen.set_underline(u);
                        }
                        Sgr::Overline(o) => {
                            pen.set_overline(o);
                        }
                        Sgr::VerticalAlign(o) => {
                            pen.set_vertical_align(o);
                        }
                        Sgr::Blink(b) => {
                            pen.set_blink(b);
                        }
                        Sgr::Italic(i) => {
                            pen.set_italic(i);
                        }
                        Sgr::Inverse(inverse) => {
                            pen.set_reverse(inverse);
                        }
                        Sgr::Invisible(invis) => {
                            pen.set_invisible(invis);
                        }
                        Sgr::StrikeThrough(strike) => {
                            pen.set_strikethrough(strike);
                        }
                        Sgr::Foreground(col) => {
                            if let ColorSpec::Default = col {
                                pen.set_foreground(default_cell.foreground());
                            } else {
                                pen.set_foreground(col);
                            }
                        }
                        Sgr::Background(col) => {
                            if let ColorSpec::Default = col {
                                pen.set_background(default_cell.background());
                            } else {
                                pen.set_background(col);
                            }
                        }
                        Sgr::UnderlineColor(col) => {
                            pen.set_underline_color(col);
                        }
                        Sgr::Font(_) => {}
                    }
                }
            }
            Action::OperatingSystemCommand(_)
            | Action::DeviceControl(_)
            | Action::Esc(_)
            | Action::KittyImage(_)
            | Action::XtGetTcap(_)
            | Action::Sixel(_) => {
                flush_print(&mut print_buffer, &mut cells, &pen);
            }
        }
    });
    flush_print(&mut print_buffer, &mut cells, &pen);
    Line::from_cells(cells, SEQ_ZERO)
}

#[cfg(test)]
mod tests {
    use super::TabBarState;
    use window::parameters::{Parameters, TitleBar};
    use window::ULength;

    fn params_with_padding(padding: usize) -> Parameters {
        Parameters {
            title_bar: TitleBar {
                padding_left: ULength::new(padding),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn num_padding_cells_without_os_parameters_uses_half_cell_minimum() {
        // With no measured button geometry (None, e.g. non-macOS or pre-realize),
        // left_pixel_margin defaults to 0.0, so only the half-cell breathing room
        // applies and rounds up to a single cell. The fallback to 10 fires only
        // when cell_width == 0 (see num_padding_cells_zero_cell_width_falls_back_to_10).
        assert_eq!(TabBarState::compute_num_padding_cells(10.0, None), 1);
        assert_eq!(TabBarState::compute_num_padding_cells(8.0, None), 1);
        assert_eq!(TabBarState::compute_num_padding_cells(20.0, None), 1);
    }

    #[test]
    fn num_padding_cells_zero_cell_width_falls_back_to_10() {
        let params = params_with_padding(64);
        assert_eq!(
            TabBarState::compute_num_padding_cells(0.0, Some(&params)),
            10
        );
    }

    #[test]
    fn num_padding_cells_rounds_up() {
        // padding_left=64, cell_width=10: 64 + 0.5*10 = 69; 69/10 = 6.9 -> ceil 7
        let params = params_with_padding(64);
        assert_eq!(
            TabBarState::compute_num_padding_cells(10.0, Some(&params)),
            7
        );

        // padding_left=70, cell_width=10: 70 + 5 = 75; 75/10 = 7.5 -> ceil 8
        let params = params_with_padding(70);
        assert_eq!(
            TabBarState::compute_num_padding_cells(10.0, Some(&params)),
            8
        );

        // Exact multiple: 15 + 5 = 20; 20/10 = 2.0 -> ceil 2 (no over-reserve)
        let params = params_with_padding(15);
        assert_eq!(
            TabBarState::compute_num_padding_cells(10.0, Some(&params)),
            2
        );
    }

    #[test]
    fn num_padding_cells_zero_padding_keeps_half_cell() {
        // Even with no measured button geometry, the half-cell breathing room
        // rounds up to one whole cell.
        let params = Parameters::default();
        assert_eq!(
            TabBarState::compute_num_padding_cells(10.0, Some(&params)),
            1
        );
        assert_eq!(
            TabBarState::compute_num_padding_cells(7.0, Some(&params)),
            1
        );
    }

    use super::basename_of_path;

    #[test]
    fn basename_of_unix_style_url() {
        assert_eq!(basename_of_path("file:///home/user/project"), "project");
    }

    #[test]
    fn basename_of_unix_style_url_with_trailing_slash() {
        assert_eq!(basename_of_path("file:///home/user/project/"), "project");
    }

    #[test]
    fn basename_of_unix_root() {
        // No component to strip; falls back to the original string.
        assert_eq!(basename_of_path("file:///"), "file:///");
        assert_eq!(basename_of_path("/"), "/");
    }

    #[test]
    fn basename_of_windows_style_url() {
        assert_eq!(basename_of_path("file:///C:/Users/foo/bar"), "bar");
    }

    #[test]
    fn basename_of_windows_style_url_with_trailing_slash() {
        assert_eq!(basename_of_path("file:///C:/Users/foo/bar/"), "bar");
    }

    #[test]
    fn basename_of_windows_drive_root() {
        // A bare drive root has no basename component; falls back to original.
        let result = basename_of_path("file:///C:/");
        assert_eq!(result, "file:///C:/");
    }

    #[test]
    fn basename_of_plain_path_single_component() {
        assert_eq!(basename_of_path("project"), "project");
    }

    #[test]
    fn basename_of_plain_unix_path() {
        assert_eq!(basename_of_path("/home/user/project"), "project");
    }

    #[test]
    fn basename_of_plain_windows_path() {
        assert_eq!(basename_of_path("C:\\Users\\foo\\bar"), "bar");
    }

    #[test]
    fn basename_of_plain_path_with_trailing_slash() {
        assert_eq!(basename_of_path("/home/user/project/"), "project");
    }

    #[test]
    fn basename_of_unc_path() {
        // UNC paths (\\server\share\path) aren't produced by
        // get_current_working_dir today, but basename_of_path should still
        // degrade gracefully rather than panicking.
        assert_eq!(basename_of_path("\\\\server\\share\\path"), "path");
    }
}
