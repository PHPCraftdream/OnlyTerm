// The range_plus_one lint can't see when the LHS is not compatible with
// and inclusive range
#![allow(clippy::range_plus_one)]
use super::*;
use crate::color::{ColorPalette, RgbColor};
use crate::config::{BidiMode, NewlineCanon};
use log::debug;
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use terminfo::Database;
use termwiz::input::KeyboardEncoding;
use url::Url;
use wezterm_bidi::ParagraphDirectionHint;
use wezterm_cell::image::ImageData;
use wezterm_cell::UnicodeVersion;
use wezterm_escape_parser::csi::{CursorStyle, Edit, EraseInDisplay, EraseInLine, TabulationClear};
use wezterm_escape_parser::{OperatingSystemCommand, CSI};
use wezterm_surface::{CursorShape, CursorVisibility, SequenceNo};

mod csi;
mod image;
mod iterm;
mod keyboard;
mod kitty;
mod mouse;
pub(crate) mod performer;
mod sixel;
use crate::terminalstate::image::*;
use crate::terminalstate::kitty::*;

lazy_static::lazy_static! {
    static ref DB: Database = {
        let data = include_bytes!("../../../termwiz/data/wezterm");
        Database::from_buffer(&data[..]).unwrap()
    };
}

pub(crate) struct TabStop {
    tabs: Vec<bool>,
    tab_width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharSet {
    Ascii,
    Uk,
    DecLineDrawing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MouseEncoding {
    X10,
    Utf8,
    // SGR is the standard xterm/VT abbreviation for "Select Graphic Rendition";
    // renaming it would diverge from the spec terminology used across the codebase.
    #[allow(clippy::upper_case_acronyms)]
    SGR,
    SgrPixels,
}

impl TabStop {
    fn new(screen_width: usize, tab_width: usize) -> Self {
        let mut tabs = Vec::with_capacity(screen_width);

        for i in 0..screen_width {
            tabs.push((i % tab_width) == 0);
        }
        Self { tabs, tab_width }
    }

    fn set_tab_stop(&mut self, col: usize) {
        self.tabs[col] = true;
    }

    fn find_prev_tab_stop(&self, col: usize) -> Option<usize> {
        (0..col.min(self.tabs.len())).rev().find(|&i| self.tabs[i])
    }

    fn find_next_tab_stop(&self, col: usize) -> Option<usize> {
        (col + 1..self.tabs.len()).find(|&i| self.tabs[i])
    }

    /// Respond to the terminal resizing.
    /// If the screen got bigger, we need to expand the tab stops
    /// into the new columns with the appropriate width.
    fn resize(&mut self, screen_width: usize) {
        let current = self.tabs.len();
        if screen_width > current {
            for i in current..screen_width {
                self.tabs.push((i % self.tab_width) == 0);
            }
        }
    }

    fn clear(&mut self, to_clear: TabulationClear, col: usize, log_unknown_escape_sequences: bool) {
        match to_clear {
            TabulationClear::ClearCharacterTabStopAtActivePosition => {
                if let Some(t) = self.tabs.get_mut(col) {
                    *t = false;
                }
            }
            // If we want to exactly match VT100/xterm behavior, then
            // we cannot honor ClearCharacterTabStopsAtActiveLine.
            TabulationClear::ClearAllCharacterTabStops => {
                // | TabulationClear::ClearCharacterTabStopsAtActiveLine
                for t in &mut self.tabs {
                    *t = false;
                }
            }
            _ => {
                if log_unknown_escape_sequences {
                    log::warn!("unhandled TabulationClear {:?}", to_clear);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SavedCursor {
    position: CursorPosition,
    wrap_next: bool,
    pen: CellAttributes,
    dec_origin_mode: bool,
    g0_charset: CharSet,
    g1_charset: CharSet,
    // TODO: selective_erase when supported
}

struct ScreenOrAlt {
    /// The primary screen + scrollback
    screen: Screen,
    /// The alternate screen; no scrollback
    alt_screen: Screen,
    /// Tells us which screen is active
    alt_screen_is_active: bool,
}

impl Deref for ScreenOrAlt {
    type Target = Screen;

    fn deref(&self) -> &Screen {
        if self.alt_screen_is_active {
            &self.alt_screen
        } else {
            &self.screen
        }
    }
}

impl DerefMut for ScreenOrAlt {
    fn deref_mut(&mut self) -> &mut Screen {
        if self.alt_screen_is_active {
            &mut self.alt_screen
        } else {
            &mut self.screen
        }
    }
}

impl ScreenOrAlt {
    pub fn new(
        size: TerminalSize,
        config: &Arc<dyn TerminalConfiguration>,
        seqno: SequenceNo,
        bidi_mode: BidiMode,
    ) -> Self {
        let screen = Screen::new(size, config, true, seqno, bidi_mode);
        let alt_screen = Screen::new(size, config, false, seqno, bidi_mode);

        Self {
            screen,
            alt_screen,
            alt_screen_is_active: false,
        }
    }

    pub fn resize(
        &mut self,
        size: TerminalSize,
        cursor_main: CursorPosition,
        cursor_alt: CursorPosition,
        seqno: SequenceNo,
        is_conpty: bool,
        bidi_mode: BidiMode,
    ) -> (CursorPosition, CursorPosition) {
        let cursor_main = self
            .screen
            .resize(size, cursor_main, seqno, is_conpty, bidi_mode);
        let cursor_alt = self
            .alt_screen
            .resize(size, cursor_alt, seqno, is_conpty, bidi_mode);
        (cursor_main, cursor_alt)
    }

    pub fn activate_alt_screen(&mut self, seqno: SequenceNo) {
        self.alt_screen_is_active = true;
        self.dirty_top_phys_rows(seqno);
    }

    pub fn activate_primary_screen(&mut self, seqno: SequenceNo) {
        self.alt_screen_is_active = false;
        self.dirty_top_phys_rows(seqno);
    }

    // When switching between alt and primary screen, we implicitly change
    // the content associated with StableRowIndex 0..num_rows.  The muxer
    // use case needs to know to invalidate its cache, so we mark those rows
    // as dirty.
    fn dirty_top_phys_rows(&mut self, seqno: SequenceNo) {
        let num_rows = self.screen.physical_rows;
        for line_idx in 0..num_rows {
            self.screen
                .line_mut(line_idx)
                .update_last_change_seqno(seqno);
        }
    }

    pub fn is_alt_screen_active(&self) -> bool {
        self.alt_screen_is_active
    }

    pub fn saved_cursor(&mut self) -> &mut Option<SavedCursor> {
        if self.alt_screen_is_active {
            &mut self.alt_screen.saved_cursor
        } else {
            &mut self.screen.saved_cursor
        }
    }

    pub fn full_reset(&mut self) {
        self.screen.full_reset();
        self.alt_screen.full_reset();
    }
}

/// Manages the state for the terminal
pub struct TerminalState {
    config: Arc<dyn TerminalConfiguration>,

    screen: ScreenOrAlt,
    /// The current set of attributes in effect for the next
    /// attempt to print to the display
    pen: CellAttributes,
    /// The current cursor position, relative to the top left
    /// of the screen.  0-based index.
    cursor: CursorPosition,

    /// if true, implicitly move to the next line on the next
    /// printed character
    wrap_next: bool,

    clear_semantic_attribute_on_newline: bool,

    /// If true, writing a character inserts a new cell
    insert: bool,

    /// https://vt100.net/docs/vt510-rm/DECAWM.html
    dec_auto_wrap: bool,

    /// Reverse Wraparound Mode
    reverse_wraparound_mode: bool,

    /// Reverse video mode
    reverse_video_mode: bool,

    /// https://vt100.net/docs/vt510-rm/DECOM.html
    /// When OriginMode is enabled, cursor is constrained to the
    /// scroll region and its position is relative to the scroll
    /// region.
    dec_origin_mode: bool,

    /// The scroll region
    top_and_bottom_margins: Range<VisibleRowIndex>,
    left_and_right_margins: Range<usize>,
    left_and_right_margin_mode: bool,

    /// When set, modifies the sequence of bytes sent for keys
    /// designated as cursor keys.  This includes various navigation
    /// keys.  The code in key_down() is responsible for interpreting this.
    application_cursor_keys: bool,
    modify_other_keys: Option<i64>,

    dec_ansi_mode: bool,

    /// https://vt100.net/dec/ek-vt38t-ug-001.pdf#page=132 has a
    /// discussion on what sixel dispay mode (DECSDM) does.
    sixel_display_mode: bool,
    use_private_color_registers_for_each_graphic: bool,

    /// Graphics mode color register map.
    color_map: HashMap<u16, RgbColor>,

    /// When set, modifies the sequence of bytes sent for keys
    /// in the numeric keypad portion of the keyboard.
    application_keypad: bool,

    /// When set, pasting the clipboard should bracket the data with
    /// designated marker characters.
    bracketed_paste: bool,

    /// Movement events enabled
    any_event_mouse: bool,
    focus_tracking: bool,
    /// X10 (legacy), SGR, and SGR-Pixels style mouse tracking and
    /// reporting is enabled
    mouse_encoding: MouseEncoding,
    mouse_tracking: bool,
    /// Button events enabled
    button_event_mouse: bool,
    current_mouse_buttons: Vec<MouseButton>,
    last_mouse_move: Option<MouseEvent>,
    cursor_visible: bool,

    keyboard_encoding: KeyboardEncoding,
    /// Support for US, UK, and DEC Special Graphics
    g0_charset: CharSet,
    g1_charset: CharSet,
    shift_out: bool,

    newline_mode: bool,

    tabs: TabStop,

    /// The terminal title string (OSC 2)
    title: String,
    /// The icon title string (OSC 1)
    icon_title: Option<String>,
    progress: Progress,

    palette: Option<ColorPalette>,

    pixel_width: usize,
    pixel_height: usize,
    dpi: u32,

    clipboard: Option<Arc<dyn Clipboard>>,
    device_control_handler: Option<Box<dyn DeviceControlHandler>>,
    alert_handler: Option<Box<dyn AlertHandler>>,
    download_handler: Option<Arc<dyn DownloadHandler>>,

    current_dir: Option<Url>,

    term_program: String,
    term_version: String,

    writer: BufWriter<Box<dyn std::io::Write + Send>>,

    image_cache: lru::LruCache<[u8; 32], Arc<ImageData>>,
    sixel_scrolls_right: bool,

    user_vars: HashMap<String, String>,

    kitty_img: KittyImageState,
    seqno: SequenceNo,

    /// The unicode version that is in effect
    unicode_version: UnicodeVersion,
    unicode_version_stack: Vec<UnicodeVersionStackEntry>,

    enable_conpty_quirks: bool,
    /// On Windows, the ConPTY layer emits an OSC sequence to
    /// set the title shortly after it starts up.
    /// We don't want that, so we use this flag to remember
    /// whether we want to skip it or not.
    suppress_initial_title_change: bool,

    accumulating_title: Option<String>,

    /// seqno when we last lost focus
    lost_focus_seqno: SequenceNo,
    /// seqno when we last emitted Alert::OutputSinceFocusLost
    lost_focus_alerted_seqno: SequenceNo,
    focused: bool,
    /// Lock-free publication of `has_unseen_output()`
    /// (`!focused && seqno > lost_focus_seqno`), held behind an `Arc` so
    /// that holders of a `Terminal` -- notably `LocalPane`, polled from
    /// the GUI title-refresh path on the GUI thread -- can observe it
    /// without taking `terminal.lock()`. Kept in sync by
    /// `publish_unseen_output()`, called from the only two mutators of
    /// the condition (`increment_seqno` and `focus_changed`).
    unseen_output_published: Arc<AtomicBool>,

    /// True if lines should be marked as bidi-enabled, and thus
    /// have the renderer apply the bidi algorithm.
    /// true is equivalent to "implicit" bidi mode as described in
    /// <https://terminal-wg.pages.freedesktop.org/bidi/recommendation/basic-modes.html>
    /// If none, then the default value specified by the config is used.
    bidi_enabled: Option<bool>,
    /// When set, specifies the bidi direction information that should be
    /// applied to lines.
    /// If none, then the default value specified by the config is used.
    bidi_hint: Option<ParagraphDirectionHint>,
}

#[derive(Debug)]
struct UnicodeVersionStackEntry {
    vers: UnicodeVersion,
    label: Option<String>,
}

fn default_color_map() -> HashMap<u16, RgbColor> {
    let mut color_map = HashMap::new();
    // Match colors to the VT340 color table:
    // https://github.com/hackerb9/vt340test/blob/main/colormap/showcolortable.png
    for (idx, r, g, b) in [
        (0, 0, 0, 0),
        (1, 0x33, 0x33, 0xcc),
        (2, 0xcc, 0x23, 0x23),
        (3, 0x33, 0xcc, 0x33),
        (4, 0xcc, 0x33, 0xcc),
        (5, 0x33, 0xcc, 0xcc),
        (6, 0xcc, 0xcc, 0xcc),
        (7, 0x77, 0x77, 0x77),
        (8, 0x44, 0x44, 0x44),
        (9, 0x56, 0x56, 0x99),
        (10, 0x99, 0x44, 0x44),
        (11, 0x56, 0x99, 0x56),
        (12, 0x99, 0x56, 0x99),
        (13, 0x56, 0x99, 0x99),
        (14, 0x99, 0x99, 0x56),
        (15, 0xcc, 0xcc, 0xcc),
    ] {
        color_map.insert(idx, RgbColor::new_8bpc(r, g, b));
    }
    color_map
}

/// This struct implements a writer that sends the data across
/// to another thread so that the write side of the terminal
/// processing never blocks.
///
/// This is important for example when processing large pastes into
/// vim.  In that scenario, we can fill up the data pending
/// on vim's input buffer, while it is busy trying to send
/// output to the terminal.  A deadlock is reached because
/// send_paste blocks on the writer, but it is unable to make
/// progress until we're able to read the output from vim.
///
/// We either need input or output to be non-blocking.
/// Output seems safest because we want to be able to exert
/// back-pressure when there is a lot of data to read,
/// and we're in control of the write side, which represents
/// input from the interactive user, or pastes.
struct ThreadedWriter {
    sender: Sender<WriterMessage>,
}

enum WriterMessage {
    Data(Vec<u8>),
    Flush,
}

impl ThreadedWriter {
    fn new(mut writer: Box<dyn std::io::Write + Send>) -> Self {
        let (sender, receiver) = channel::<WriterMessage>();

        std::thread::spawn(move || {
            while let Ok(msg) = receiver.recv() {
                match msg {
                    WriterMessage::Data(buf) => {
                        if writer.write_all(&buf).is_err() {
                            break;
                        }
                    }
                    WriterMessage::Flush => {
                        if writer.flush().is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Self { sender }
    }
}

impl std::io::Write for ThreadedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.sender
            .send(WriterMessage::Data(buf.to_vec()))
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::BrokenPipe, err))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.sender
            .send(WriterMessage::Flush)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::BrokenPipe, err))?;
        Ok(())
    }
}

impl TerminalState {
    /// Constructs the terminal state.
    /// You generally want the `Terminal` struct rather than this one;
    /// Terminal contains and dereferences to `TerminalState`.
    pub fn new(
        size: TerminalSize,
        config: Arc<dyn TerminalConfiguration>,
        term_program: &str,
        term_version: &str,
        writer: Box<dyn std::io::Write + Send>,
    ) -> TerminalState {
        let writer: Box<dyn std::io::Write + Send> = Box::new(ThreadedWriter::new(writer));
        Self::new_impl(size, config, term_program, term_version, writer)
    }

    /// Like `new`, but for callers who are handing in a `writer` that is
    /// *already* non-blocking (its own `write`/`flush` enqueue onto some
    /// background thread of its own rather than performing a real,
    /// possibly-blocking I/O operation inline -- e.g. `mux`'s
    /// `WriterWrapper`, which every caller of `Pane::writer()` also holds a
    /// clone of). Wrapping such a writer in `ThreadedWriter` again would
    /// just add a second, redundant hop -- and worse, it would mean
    /// `Pane::writer()` writes and `Terminal`'s own internal writes (e.g.
    /// keyboard/mouse encoding, DA/DSR answerback) end up going through two
    /// *independent* background threads/queues instead of the same one,
    /// making their already-best-effort relative ordering into the pty
    /// even less predictable than it is today.
    ///
    /// Using this constructor with a writer that is *not* already
    /// non-blocking would defeat the purpose of `TerminalState` never
    /// blocking on its write side; it exists specifically for writers that
    /// already provide that guarantee on their own.
    pub fn new_with_nonblocking_writer(
        size: TerminalSize,
        config: Arc<dyn TerminalConfiguration>,
        term_program: &str,
        term_version: &str,
        writer: Box<dyn std::io::Write + Send>,
    ) -> TerminalState {
        Self::new_impl(size, config, term_program, term_version, writer)
    }

    fn new_impl(
        size: TerminalSize,
        config: Arc<dyn TerminalConfiguration>,
        term_program: &str,
        term_version: &str,
        writer: Box<dyn std::io::Write + Send>,
    ) -> TerminalState {
        let writer = BufWriter::new(writer);
        let seqno = 1;
        let screen = ScreenOrAlt::new(size, &config, seqno, config.bidi_mode());

        let color_map = default_color_map();

        let unicode_version = config.unicode_version();

        TerminalState {
            config,
            screen,
            pen: CellAttributes::default(),
            cursor: CursorPosition::default(),
            top_and_bottom_margins: 0..size.rows as VisibleRowIndex,
            left_and_right_margins: 0..size.cols,
            left_and_right_margin_mode: false,
            wrap_next: false,
            clear_semantic_attribute_on_newline: false,
            // We default auto wrap to true even though the default for
            // a dec terminal is false, because it is more useful this way.
            dec_auto_wrap: true,
            reverse_wraparound_mode: false,
            reverse_video_mode: false,
            dec_origin_mode: false,
            insert: false,
            application_cursor_keys: false,
            modify_other_keys: None,
            dec_ansi_mode: false,
            sixel_display_mode: false,
            use_private_color_registers_for_each_graphic: false,
            color_map,
            application_keypad: false,
            bracketed_paste: false,
            focus_tracking: false,
            mouse_encoding: MouseEncoding::X10,
            keyboard_encoding: KeyboardEncoding::Xterm,
            sixel_scrolls_right: false,
            any_event_mouse: false,
            button_event_mouse: false,
            mouse_tracking: false,
            last_mouse_move: None,
            cursor_visible: true,
            g0_charset: CharSet::Ascii,
            g1_charset: CharSet::Ascii,
            shift_out: false,
            newline_mode: false,
            current_mouse_buttons: vec![],
            tabs: TabStop::new(size.cols, 8),
            title: crate::DEFAULT_TERMINAL_TITLE.to_string(),
            icon_title: None,
            palette: None,
            pixel_height: size.pixel_height,
            pixel_width: size.pixel_width,
            dpi: size.dpi,
            clipboard: None,
            device_control_handler: None,
            alert_handler: None,
            download_handler: None,
            current_dir: None,
            term_program: term_program.to_string(),
            term_version: term_version.to_string(),
            writer,
            image_cache: lru::LruCache::new(NonZeroUsize::new(16).unwrap()),
            user_vars: HashMap::new(),
            kitty_img: Default::default(),
            seqno,
            unicode_version,
            unicode_version_stack: vec![],
            suppress_initial_title_change: false,
            enable_conpty_quirks: false,
            accumulating_title: None,
            lost_focus_seqno: seqno,
            lost_focus_alerted_seqno: seqno,
            focused: true,
            // Matches `!focused && seqno > lost_focus_seqno` at
            // construction (focused, seqno == lost_focus_seqno == 1):
            // a freshly created terminal has no unseen output.
            unseen_output_published: Arc::new(AtomicBool::new(false)),
            bidi_enabled: None,
            bidi_hint: None,
            progress: Progress::default(),
        }
    }

    pub fn enable_conpty_quirks(&mut self) {
        self.enable_conpty_quirks = true;
        self.suppress_initial_title_change = true;
    }

    pub fn current_seqno(&self) -> SequenceNo {
        self.seqno
    }

    pub fn increment_seqno(&mut self) {
        self.seqno += 1;
        self.publish_unseen_output();
    }

    pub fn set_config(&mut self, config: Arc<dyn TerminalConfiguration>) {
        self.config = config;
    }

    pub fn get_config(&self) -> Arc<dyn TerminalConfiguration> {
        Arc::clone(&self.config)
    }

    pub fn set_clipboard(&mut self, clipboard: &Arc<dyn Clipboard>) {
        self.clipboard.replace(Arc::clone(clipboard));
    }

    pub fn set_device_control_handler(&mut self, handler: Box<dyn DeviceControlHandler>) {
        self.device_control_handler.replace(handler);
    }

    pub fn set_notification_handler(&mut self, handler: Box<dyn AlertHandler>) {
        self.alert_handler.replace(handler);
    }

    pub fn set_download_handler(&mut self, handler: &Arc<dyn DownloadHandler>) {
        self.download_handler.replace(handler.clone());
    }

    /// Returns the title text associated with the terminal session.
    /// The title can be changed by the application using a number
    /// of escape sequences:
    /// OSC 2 is used to set the window title.
    /// OSC 1 is used to set the "icon title", which some terminal
    /// emulators interpret as a shorter title string for use when
    /// showing the tab title.
    /// Here in wezterm the terminalstate is isolated from other
    /// tabs; we process escape sequences without knowledge of other
    /// tabs, so we maintain both title strings here.
    /// The gui layer doesn't currently have a concept of what the
    /// overall window title should be beyond the title for the
    /// active tab with some decoration about the number of tabs.
    /// Shell toolkits such as oh-my-zsh prefer OSC 1 titles for
    /// abbreviated information.
    /// What we do here is prefer to return the OSC 1 icon title
    /// if it is set, otherwise return the OSC 2 window title.
    pub fn get_title(&self) -> &str {
        self.icon_title.as_ref().unwrap_or(&self.title)
    }

    pub fn get_progress(&self) -> Progress {
        self.progress.clone()
    }

    /// Returns the current working directory associated with the
    /// terminal session.  The working directory can be changed by
    /// the applicaiton using the OSC 7 escape sequence.
    pub fn get_current_dir(&self) -> Option<&Url> {
        self.current_dir.as_ref()
    }

    /// Returns a copy of the palette.
    /// By default we don't keep a copy in the terminal state,
    /// preferring to take the config values from the users
    /// config file and updating to changes live.
    /// However, if they have used dynamic color scheme escape
    /// sequences we'll fork a copy of the palette at that time
    /// so that we can start tracking those changes.
    pub fn palette(&self) -> ColorPalette {
        self.palette
            .as_ref()
            .cloned()
            .unwrap_or_else(|| self.config.color_palette())
    }

    /// Called in response to dynamic color scheme escape sequences.
    /// Will make a copy of the palette from the config file if this
    /// is the first of these escapes we've seen.
    pub fn palette_mut(&mut self) -> &mut ColorPalette {
        if self.palette.is_none() {
            self.palette.replace(self.config.color_palette());
        }
        self.palette.as_mut().unwrap()
    }

    /// If the current overridden palette is effectively the same as
    /// the configured palette, remove the override and treat it as
    /// being the same as the configured state.
    /// This allows runtime changes to the configuration to take effect.
    pub fn implicit_palette_reset_if_same_as_configured(&mut self) {
        if self
            .palette
            .as_ref()
            .map(|p| *p == self.config.color_palette())
            .unwrap_or(false)
        {
            self.palette.take();
        }
    }

    /// Returns a reference to the active screen (either the primary or
    /// the alternate screen).
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// Returns a mutable reference to the active screen (either the primary or
    /// the alternate screen).
    pub fn screen_mut(&mut self) -> &mut Screen {
        &mut self.screen
    }

    fn set_clipboard_contents(
        &self,
        selection: ClipboardSelection,
        text: Option<String>,
    ) -> anyhow::Result<()> {
        if let Some(clip) = self.clipboard.as_ref() {
            clip.set_contents(selection, text)?;
        }
        Ok(())
    }

    pub fn erase_scrollback_and_viewport(&mut self) {
        // Since we may be called outside of perform_actions,
        // we need to ensure that we increment the seqno in
        // order to correctly invalidate the display
        self.increment_seqno();
        self.erase_in_display(EraseInDisplay::EraseScrollback);

        let row_index = self.screen.phys_row(self.cursor.y);
        let rows = self.screen.lines_in_phys_range(row_index..row_index + 1);

        self.erase_in_display(EraseInDisplay::EraseDisplay);

        for (idx, row) in rows.into_iter().enumerate() {
            *self.screen.line_mut(idx) = row;
        }

        self.cursor.y = 0;
    }

    /// Discards the scrollback, leaving only the data that is present
    /// in the viewport.
    pub fn erase_scrollback(&mut self) {
        // Since we may be called outside of perform_actions,
        // we need to ensure that we increment the seqno in
        // order to correctly invalidate the display
        self.increment_seqno();
        self.screen_mut().erase_scrollback();
    }

    /// Returns true if the associated application has enabled any of the
    /// supported mouse reporting modes.
    /// This is useful for the hosting GUI application to decide how best
    /// to dispatch mouse events to the terminal.
    pub fn is_mouse_grabbed(&self) -> bool {
        self.mouse_tracking || self.button_event_mouse || self.any_event_mouse
    }

    pub fn is_alt_screen_active(&self) -> bool {
        self.screen.is_alt_screen_active()
    }

    /// Returns true if the associated application has enabled
    /// bracketed paste mode, which can be helpful to the hosting
    /// GUI application to decide about fragmenting a large paste.
    pub fn bracketed_paste_enabled(&self) -> bool {
        self.bracketed_paste
    }

    /// Advise the terminal about a change in its focus state
    pub fn focus_changed(&mut self, focused: bool) {
        if focused == self.focused {
            return;
        }
        if !focused {
            // notify app of release of buttons
            let buttons = self.current_mouse_buttons.clone();
            for b in buttons {
                self.mouse_event(MouseEvent {
                    kind: MouseEventKind::Release,
                    button: b,
                    modifiers: KeyModifiers::NONE,
                    x: 0,
                    y: 0,
                    x_pixel_offset: 0,
                    y_pixel_offset: 0,
                })
                .ok();
            }
        }
        if self.focus_tracking {
            write!(self.writer, "{}{}", CSI, if focused { "I" } else { "O" }).ok();
            self.writer.flush().ok();
        }
        self.focused = focused;
        if !focused {
            self.lost_focus_seqno = self.seqno;
        }
        self.publish_unseen_output();
    }

    /// Returns true if there is new output since the terminal
    /// lost focus
    pub fn has_unseen_output(&self) -> bool {
        !self.focused && self.seqno > self.lost_focus_seqno
    }

    /// Recompute and publish the lock-free mirror of
    /// `has_unseen_output()`. Called only from the two mutators of that
    /// condition (`increment_seqno` and `focus_changed`) so the `Arc`
    /// seen by lock-free readers stays in sync with the in-lock state.
    fn publish_unseen_output(&self) {
        let value = !self.focused && self.seqno > self.lost_focus_seqno;
        self.unseen_output_published.store(value, Ordering::Release);
    }

    /// Returns a lock-free handle that mirrors `has_unseen_output()`,
    /// updated whenever focus or the sequence number changes. Lets
    /// callers observe unseen-output state without taking the terminal
    /// mutex -- important because that mutex is held by the pty output
    /// parser under load, and `LocalPane` is polled from the GUI thread
    /// on essentially every event.
    pub fn unseen_output_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.unseen_output_published)
    }

    pub(crate) fn trigger_unseen_output_notif(&mut self) {
        if self.has_unseen_output() {
            // We want to avoid over-notifying about output events,
            // so here we gate the notification to the case where
            // we have lost the focus more recently than the last
            // time we notified about it
            if self.lost_focus_seqno > self.lost_focus_alerted_seqno {
                self.lost_focus_alerted_seqno = self.seqno;
                if let Some(handler) = self.alert_handler.as_mut() {
                    handler.alert(Alert::OutputSinceFocusLost);
                }
            }
        }
    }

    /// Send text to the terminal that is the result of pasting.
    /// If bracketed paste mode is enabled, the paste is enclosed
    /// De-fang the text by removing any embedded bracketed paste
    /// sequence that may be present.  Loops until no more occurrences
    /// remain, because a single pass of str::replace is not idempotent:
    /// nested sequences like `\x1b\x1b[200~[200~` can leave a valid
    /// marker behind after the first sweep.
    fn defang_paste(text: &str) -> String {
        let mut result = text.to_string();
        loop {
            let prev = result.clone();
            result = result.replace("\x1b[200~", "").replace("\x1b[201~", "");
            if result == prev {
                break;
            }
        }
        result
    }

    /// in the bracketing, otherwise it is fed to the writer as-is.
    pub fn send_paste(&mut self, text: &str) -> Result<(), Error> {
        let mut buf = String::new();
        if self.bracketed_paste {
            buf.push_str("\x1b[200~");
        }

        let canon = if self.bracketed_paste {
            NewlineCanon::None
        } else {
            self.config.canonicalize_pasted_newlines()
        };

        let canon = canon.canonicalize(text);
        let de_fanged = Self::defang_paste(&canon);
        buf.push_str(&de_fanged);

        if self.bracketed_paste {
            buf.push_str("\x1b[201~");
        }

        self.writer.write_all(buf.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }

    /// Informs the terminal that the viewport of the window has resized to the
    /// specified dimensions.
    /// We need to resize both the primary and alt screens, adjusting
    /// the cursor positions of both accordingly.
    pub fn resize(&mut self, size: TerminalSize) {
        self.increment_seqno();
        let (cursor_main, cursor_alt) = if self.screen.alt_screen_is_active {
            (
                self.screen
                    .screen
                    .saved_cursor
                    .as_ref()
                    .map(|s| s.position)
                    .unwrap_or_default(),
                self.cursor,
            )
        } else {
            (
                self.cursor,
                self.screen
                    .alt_screen
                    .saved_cursor
                    .as_ref()
                    .map(|s| s.position)
                    .unwrap_or_default(),
            )
        };

        let bidi_mode = self.get_bidi_mode();
        let (adjusted_cursor_main, adjusted_cursor_alt) = self.screen.resize(
            size,
            cursor_main,
            cursor_alt,
            self.seqno,
            self.enable_conpty_quirks,
            bidi_mode,
        );
        self.top_and_bottom_margins = 0..size.rows as i64;
        self.left_and_right_margins = 0..size.cols;
        self.pixel_height = size.pixel_height;
        self.pixel_width = size.pixel_width;
        self.dpi = size.dpi;
        self.tabs.resize(size.cols);

        if self.screen.alt_screen_is_active {
            self.set_cursor_pos(
                &Position::Absolute(adjusted_cursor_alt.x as i64),
                &Position::Absolute(adjusted_cursor_alt.y),
            );

            if let Some(saved) = self.screen.screen.saved_cursor.as_mut() {
                saved.position.x = adjusted_cursor_main.x;
                saved.position.y = adjusted_cursor_main.y;
                saved.position.seqno = self.seqno;
                saved.wrap_next = false;
            }
        } else {
            self.set_cursor_pos(
                &Position::Absolute(adjusted_cursor_main.x as i64),
                &Position::Absolute(adjusted_cursor_main.y),
            );
            if let Some(saved) = self.screen.alt_screen.saved_cursor.as_mut() {
                saved.position.x = adjusted_cursor_alt.x;
                saved.position.y = adjusted_cursor_alt.y;
                saved.position.seqno = self.seqno;
                saved.wrap_next = false;
            }
        }
    }

    pub fn get_size(&self) -> TerminalSize {
        let screen = self.screen();
        TerminalSize {
            dpi: self.dpi,
            pixel_width: self.pixel_width,
            pixel_height: self.pixel_height,
            rows: screen.physical_rows,
            cols: screen.physical_cols,
        }
    }

    fn palette_did_change(&mut self) {
        self.make_all_lines_dirty();
        if let Some(handler) = self.alert_handler.as_mut() {
            handler.alert(Alert::PaletteChanged);
        }
    }

    /// When dealing with selection, mark a range of lines as dirty
    pub fn make_all_lines_dirty(&mut self) {
        let seqno = self.seqno;
        let screen = self.screen_mut();
        screen.for_each_phys_line_mut(|_, line| {
            line.update_last_change_seqno(seqno);
        });
    }

    /// Returns the 0-based cursor position relative to the top left of
    /// the visible screen
    pub fn cursor_pos(&self) -> CursorPosition {
        CursorPosition {
            x: self.cursor.x,
            y: self.cursor.y,
            shape: self.cursor.shape,
            visibility: if self.cursor_visible {
                CursorVisibility::Visible
            } else {
                CursorVisibility::Hidden
            },
            seqno: self.cursor.seqno,
        }
    }

    /// Returns the current cell attributes of the screen
    pub fn pen(&self) -> CellAttributes {
        self.pen.clone()
    }

    pub fn user_vars(&self) -> &HashMap<String, String> {
        &self.user_vars
    }

    fn clear_semantic_attribute_due_to_movement(&mut self) {
        if self.clear_semantic_attribute_on_newline {
            self.clear_semantic_attribute_on_newline = false;
            self.pen.set_semantic_type(SemanticType::default());
        }
    }

    /// Sets the cursor position to precisely the x and values provided
    fn set_cursor_position_absolute(&mut self, x: usize, y: VisibleRowIndex) {
        if self.cursor.y != y {
            self.clear_semantic_attribute_due_to_movement();
        }
        self.cursor.y = y;
        self.cursor.x = x;
        self.cursor.seqno = self.seqno;
        self.wrap_next = false;
    }

    /// Sets the cursor position. x and y are 0-based and relative to the
    /// top left of the visible screen.
    fn set_cursor_pos(&mut self, x: &Position, y: &Position) {
        let x = match *x {
            Position::Relative(x) => (self.cursor.x as i64 + x)
                .min(
                    if self.dec_origin_mode {
                        self.left_and_right_margins.end
                    } else {
                        self.screen().physical_cols
                    } as i64
                        - 1,
                )
                .max(0),
            Position::Absolute(x) => (x + if self.dec_origin_mode {
                self.left_and_right_margins.start
            } else {
                0
            } as i64)
                .min(
                    if self.dec_origin_mode {
                        self.left_and_right_margins.end
                    } else {
                        // We allow 1 extra for the cursor x position
                        // to account for some resize/rewrap scenarios
                        // where we don't want to forget that the
                        // cursor belongs to a wrapped line
                        self.screen().physical_cols + 1
                    } as i64
                        - 1,
                )
                .max(0),
        };

        let y = match *y {
            Position::Relative(y) => (self.cursor.y + y)
                .min(
                    if self.dec_origin_mode {
                        self.top_and_bottom_margins.end
                    } else {
                        self.screen().physical_rows as i64
                    } - 1,
                )
                .max(0),
            Position::Absolute(y) => (y + if self.dec_origin_mode {
                self.top_and_bottom_margins.start
            } else {
                0
            })
            .min(
                if self.dec_origin_mode {
                    self.top_and_bottom_margins.end
                } else {
                    self.screen().physical_rows as i64
                } - 1,
            )
            .max(0),
        };

        self.set_cursor_position_absolute(x as usize, y);
    }

    fn scroll_up(&mut self, num_rows: usize) {
        let seqno = self.seqno;
        let blank_attr = self.pen.clone_sgr_only();
        let top_and_bottom_margins = self.top_and_bottom_margins.clone();
        let left_and_right_margins = self.left_and_right_margins.clone();
        let bidi_mode = self.get_bidi_mode();
        self.screen_mut().scroll_up_within_margins(
            &top_and_bottom_margins,
            &left_and_right_margins,
            num_rows,
            seqno,
            blank_attr,
            bidi_mode,
        )
    }

    fn scroll_down(&mut self, num_rows: usize) {
        let seqno = self.seqno;
        let blank_attr = self.pen.clone_sgr_only();
        let top_and_bottom_margins = self.top_and_bottom_margins.clone();
        let left_and_right_margins = self.left_and_right_margins.clone();
        let bidi_mode = self.get_bidi_mode();
        self.screen_mut().scroll_down_within_margins(
            &top_and_bottom_margins,
            &left_and_right_margins,
            num_rows,
            seqno,
            blank_attr,
            bidi_mode,
        )
    }

    /// Defined by FinalTermSemanticPrompt; a fresh-line is a NOP if the
    /// cursor is already at the left margin, otherwise it is the same as
    /// a new line.
    fn fresh_line(&mut self) {
        if self.cursor.x == self.left_and_right_margins.start {
            return;
        }
        self.new_line(true);
    }

    fn new_line(&mut self, move_to_first_column: bool) {
        let x = if move_to_first_column {
            self.left_and_right_margins.start
        } else {
            self.cursor.x
        };
        let y = self.cursor.y;
        let y = if y == self.top_and_bottom_margins.end - 1 {
            self.scroll_up(1);
            y
        } else {
            y + 1
        };
        self.set_cursor_pos(&Position::Absolute(x as i64), &Position::Absolute(y));
    }

    /// Moves the cursor down one line in the same column.
    /// If the cursor is at the bottom margin, the page scrolls up.
    fn c1_index(&mut self) {
        if self.left_and_right_margins.contains(&self.cursor.x) {
            if self.cursor.y == self.top_and_bottom_margins.end - 1 {
                self.scroll_up(1);
            } else {
                self.set_cursor_pos(&Position::Relative(0), &Position::Relative(1));
            }
        }
    }

    /// Moves the cursor to the first position on the next line.
    /// If the cursor is at the bottom margin, the page scrolls up.
    fn c1_nel(&mut self) {
        let y_clamp = if self.top_and_bottom_margins.contains(&self.cursor.y) {
            self.top_and_bottom_margins.end - 1
        } else {
            self.screen().physical_rows as VisibleRowIndex - 1
        };

        if self.left_and_right_margins.contains(&self.cursor.x) {
            if self.cursor.y == self.top_and_bottom_margins.end - 1 {
                self.scroll_up(1);
                self.set_cursor_position_absolute(self.left_and_right_margins.start, self.cursor.y);
            } else {
                self.set_cursor_position_absolute(
                    self.left_and_right_margins.start,
                    (self.cursor.y + 1).min(y_clamp),
                );
            }
        } else {
            // When outside left/right margins, NEL moves but does not scroll
            self.set_cursor_position_absolute(
                if self.cursor.x < self.left_and_right_margins.start {
                    self.cursor.x
                } else {
                    self.left_and_right_margins.start
                },
                (self.cursor.y + 1).min(y_clamp),
            );
        }
    }

    /// Sets a horizontal tab stop at the column where the cursor is.
    fn c1_hts(&mut self) {
        self.tabs.set_tab_stop(self.cursor.x);
    }

    /// Moves the cursor to the next tab stop. If there are no more tab stops,
    /// the cursor moves to the right margin. HT does not cause text to auto
    /// wrap.
    fn c0_horizontal_tab(&mut self) {
        let seqno = self.seqno;
        let x = match self.tabs.find_next_tab_stop(self.cursor.x) {
            Some(x) => x,
            None => self.left_and_right_margins.end - 1,
        };
        self.cursor.x = x.min(self.left_and_right_margins.end - 1);
        self.cursor.seqno = seqno;
    }

    /// Move the cursor up 1 line.  If the position is at the top scroll margin,
    /// scroll the region down.
    fn c1_reverse_index(&mut self) {
        if self.left_and_right_margins.contains(&self.cursor.x) {
            if self.cursor.y == self.top_and_bottom_margins.start {
                self.scroll_down(1);
            } else {
                self.set_cursor_pos(&Position::Relative(0), &Position::Relative(-1));
            }
        }
    }

    fn set_hyperlink(&mut self, link: Option<Hyperlink>) {
        self.pen.set_hyperlink(link.map(Arc::new));
    }

    fn erase_in_display(&mut self, erase: EraseInDisplay) {
        let seqno = self.seqno;
        let cy = self.cursor.y;
        let pen = self.pen.clone_sgr_only();
        let rows = self.screen().physical_rows as VisibleRowIndex;
        let col_range = 0..self.screen().physical_cols;
        let row_range = match erase {
            EraseInDisplay::EraseToEndOfDisplay => {
                self.perform_csi_edit(Edit::EraseInLine(EraseInLine::EraseToEndOfLine));
                cy + 1..rows
            }
            EraseInDisplay::EraseToStartOfDisplay => {
                self.perform_csi_edit(Edit::EraseInLine(EraseInLine::EraseToStartOfLine));
                0..cy
            }
            EraseInDisplay::EraseDisplay => 0..rows,
            EraseInDisplay::EraseScrollback => {
                self.screen_mut().erase_scrollback();
                return;
            }
        };

        {
            let bidi_mode = self.get_bidi_mode();
            let screen = self.screen_mut();
            for y in row_range {
                screen.clear_line(y, col_range.clone(), &pen, seqno, bidi_mode);
                let line_idx = screen.phys_row(y);
                screen.line_mut(line_idx).set_single_width(seqno);
            }
        }
    }

    fn get_bidi_mode(&self) -> BidiMode {
        let mut mode = self.config.bidi_mode();
        if let Some(enabled) = &self.bidi_enabled {
            mode.enabled = *enabled;
        }
        if let Some(hint) = &self.bidi_hint {
            mode.hint = *hint;
        }
        mode
    }

    /// Computes the set of `SemanticZone`s for the current terminal screen.
    /// Semantic zones are contiguous runs of cells that have the same
    /// `SemanticType` (Prompt, Input, Output).
    /// Due to the way that the terminal clears the screen, the raw, literal
    /// set of zones is overly fragmented by blanks.  This method will ignore
    /// trailing Output regions when computing the SemanticZone bounds.
    ///
    /// By default, all screen data is of type Output.  The shell needs to
    /// employ OSC 133 escapes to markup its output.
    pub fn get_semantic_zones(&mut self) -> anyhow::Result<Vec<SemanticZone>> {
        let screen = self.screen_mut();

        let mut current_zone: Option<SemanticZone> = None;
        let mut zones = vec![];

        let first_stable_row = screen.phys_to_stable_row_index(0);
        screen.for_each_phys_line_mut(|idx, line| {
            let stable_row = first_stable_row + idx as StableRowIndex;

            for zone_range in line.semantic_zone_ranges() {
                let new_zone = match current_zone.as_ref() {
                    None => true,
                    Some(zone) => zone.semantic_type != zone_range.semantic_type,
                };

                if new_zone {
                    if let Some(zone) = current_zone.take() {
                        zones.push(zone);
                    }

                    current_zone.replace(SemanticZone {
                        start_x: zone_range.range.start as usize,
                        start_y: stable_row,
                        end_x: zone_range.range.end as usize,
                        end_y: stable_row,
                        semantic_type: zone_range.semantic_type,
                    });
                }

                if let Some(zone) = current_zone.as_mut() {
                    zone.end_x = zone_range.range.end as usize;
                    zone.end_y = stable_row;
                }
            }
        });
        if let Some(zone) = current_zone.take() {
            zones.push(zone);
        }

        Ok(zones)
    }

    #[inline]
    pub fn get_reverse_video(&self) -> bool {
        self.reverse_video_mode
    }

    pub fn get_keyboard_encoding(&self) -> KeyboardEncoding {
        self.screen()
            .keyboard_stack
            .last()
            .copied()
            .unwrap_or(self.keyboard_encoding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defang_paste_no_escape() {
        assert_eq!(TerminalState::defang_paste("hello world"), "hello world");
    }

    #[test]
    fn defang_paste_single_start() {
        assert_eq!(TerminalState::defang_paste("\x1b[200~hello"), "hello");
    }

    #[test]
    fn defang_paste_single_end() {
        assert_eq!(TerminalState::defang_paste("hello\x1b[201~"), "hello");
    }

    #[test]
    fn defang_paste_both() {
        assert_eq!(
            TerminalState::defang_paste("\x1b[200~hello\x1b[201~"),
            "hello"
        );
    }

    #[test]
    fn defang_paste_double_nested_start() {
        // Reporter's exploit: nesting strips the inner pair first,
        // leaving the outer escape sequences intact as a valid marker.
        assert_eq!(
            TerminalState::defang_paste("\x1b\x1b[200~[200~injected"),
            "injected"
        );
    }

    #[test]
    fn defang_paste_triple_nested() {
        assert_eq!(
            TerminalState::defang_paste("\x1b\x1b\x1b[200~[200~[200~injected"),
            "injected"
        );
    }

    #[test]
    fn defang_paste_mixed_double_nested() {
        // Both start and end markers nested
        assert_eq!(
            TerminalState::defang_paste("\x1b\x1b[200~[200~hello\x1b\x1b[201~[201~"),
            "hello"
        );
    }
}
