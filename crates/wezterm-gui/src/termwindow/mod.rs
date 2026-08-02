#![allow(clippy::range_plus_one)]
use super::renderstate::*;
use super::utilsprites::RenderMetrics;
use crate::colorease::ColorEase;
use crate::frontend::{front_end, try_front_end};
use crate::inputmap::InputMap;
use crate::overlay::{
    launcher, start_overlay, CopyModeParams, CopyOverlay, LauncherArgs, LauncherFlags,
    QuickSelectOverlay,
};
use crate::gui_api::guiwin::GuiWin;
use crate::resize_increment_calculator::ResizeIncrementCalculator;
use crate::scrollbar::*;
use crate::selection::Selection;
use crate::shapecache::*;
use crate::tabbar::{TabBarItem, TabBarState};
use crate::termwindow::background::{
    load_background_image, reload_background_image, LoadedBackgroundLayer,
};
use crate::termwindow::keyevent::{KeyTableArgs, KeyTableState};
use crate::termwindow::modal::Modal;
use crate::termwindow::render::paint::AllowImage;
use crate::termwindow::render::{
    CachedLineState, LineQuadCacheKey, LineQuadCacheValue, LineToEleShapeCacheKey,
    LineToElementShapeItem,
};
use crate::termwindow::webgpu::WebGpuState;
use ::wezterm_term::input::{ClickPosition, MouseButton as TMB};
use ::window::*;
use anyhow::{anyhow, ensure, Context};
use config::keyassignment::{
    ClipboardCopyDestination, Confirmation, KeyAssignment, LauncherActionArgs, PaneDirection,
    Pattern, PromptInputLine, QuickSelectArguments, SpawnCommand, SplitSize,
};
use config::window::WindowLevel;
use config::{
    configuration, AudibleBell, ConfigHandle, Dimension, DimensionContext, FrontEndSelection,
    GeometryOrigin, GuiPosition, TermConfig,
};
use lfucache::*;
use mux::pane::{
    CachePolicy, Pane, PaneId, Pattern as MuxPattern, PerformAssignmentResult,
};
use mux::renderable::RenderableDimensions;
use mux::tab::{
    PositionedPane, PositionedSplit, SplitDirection, SplitRequest, SplitSize as MuxSplitSize, Tab,
    TabId,
};
use mux::window::WindowId as MuxWindowId;
use mux::{Mux, MuxNotification};
use mux_funcs::MuxPane;
use smol::channel::Sender;
use smol::Timer;
use std::cell::{Cell, RefCell, RefMut};
use std::collections::{HashMap, LinkedList};
use std::ops::Add;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use termwiz::hyperlink::Hyperlink;
use termwiz::surface::SequenceNo;
use wezterm_dynamic::Value;
use wezterm_font::FontConfiguration;
use wezterm_term::color::ColorPalette;
use wezterm_term::input::LastMouseClick;
use wezterm_term::{Alert, Progress, StableRowIndex, TerminalConfiguration, TerminalSize};

pub mod background;
pub mod box_model;
pub mod charselect;
pub mod clipboard;
pub mod keyevent;
pub mod modal;
mod mouseevent;
pub mod palette;
pub mod paneselect;
mod prevcursor;
pub mod render;
pub mod resize;
mod selection;
pub mod spawn;
pub mod webgpu;
use crate::spawn::SpawnWhere;
use prevcursor::PrevCursorPos;

const ATLAS_SIZE: usize = 128;

lazy_static::lazy_static! {
    static ref WINDOW_CLASS: Mutex<String> = Mutex::new(wezterm_gui_subcommands::DEFAULT_WINDOW_CLASS.to_owned());
    static ref POSITION: Mutex<Option<GuiPosition>> = Mutex::new(None);
}

pub const ICON_DATA: &'static [u8] = include_bytes!("../../../../assets/icon/terminal.png");

pub fn set_window_position(pos: GuiPosition) {
    POSITION.lock().unwrap().replace(pos);
}

pub fn set_window_class(cls: &str) {
    *WINDOW_CLASS.lock().unwrap() = cls.to_owned();
}

pub fn get_window_class() -> String {
    WINDOW_CLASS.lock().unwrap().clone()
}

/// Starter config written the first time someone opens their settings
/// without having a config file yet. Every line is commented out, so the
/// file it creates is equivalent to having no config at all -- opening
/// your settings must never change how the terminal behaves, it just gives
/// you somewhere to start typing.
const STARTER_CONFIG: &str = "\
## OnlyTerm configuration.
##
## This is a ktav document: `key: value` pairs, one per line, no quotes
## around values. Lines starting with ## are comments. The file is
## re-read automatically whenever you save it.
##
## Everything below is commented out, i.e. this file currently changes
## nothing. Uncomment a line and save to try it.
##
## Full reference: https://wezterm.org/config/files.html

## font_size: 12.0
## color_scheme: Catppuccin Mocha

## A font, plus the fallbacks to use for glyphs it doesn't cover. Note
## that a backslash starts an escape sequence, so Windows paths need to
## be written with forward slashes or doubled backslashes.
## font: { font: [{ family: JetBrains Mono }, { family: Miriam Mono CLM }] }

## initial_cols: 120
## initial_rows: 28
## enable_scroll_bar: true
## scrollback_lines: 10000
";

/// Open the user's config file in whatever the OS associates with it,
/// creating it from `STARTER_CONFIG` first if they don't have one yet.
///
/// Uses `Config::config_file_path` rather than the *loaded* config's path
/// on purpose: a config that exists but fails to parse is exactly the one
/// you want this to open, and in that state the running config is the
/// built-in default, which knows nothing about the file.
fn open_config_file() -> anyhow::Result<()> {
    use anyhow::Context;

    let path = config::Config::config_file_path();

    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating config directory {}", parent.display()))?;
        }
        std::fs::write(&path, STARTER_CONFIG)
            .with_context(|| format!("creating config file {}", path.display()))?;
        log::info!("created a starter config at {}", path.display());
    }

    wezterm_open_url::open_text_file(&path);
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MouseCapture {
    UI,
    TerminalPane(PaneId),
}

/// Type used together with Window::notify to do something in the
/// context of the window-specific event loop
pub enum TermWindowNotif {
    InvalidateShapeCache,
    PerformAssignment {
        pane_id: PaneId,
        assignment: KeyAssignment,
        tx: Option<Sender<anyhow::Result<()>>>,
    },
    CancelOverlayForPane(PaneId),
    CancelOverlayForTab {
        tab_id: TabId,
        pane_id: Option<PaneId>,
    },
    MuxNotification(MuxNotification),
    EmitStatusUpdate,
    Apply(Box<dyn FnOnce(&mut TermWindow) + Send + Sync>),
    SwitchToMuxWindow(MuxWindowId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UIItemType {
    TabBar(TabBarItem),
    CloseTab(usize),
    AboveScrollThumb,
    ScrollThumb,
    BelowScrollThumb,
    Split(PositionedSplit),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UIItem {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub item_type: UIItemType,
}

impl UIItem {
    pub fn hit_test(&self, x: isize, y: isize) -> bool {
        x >= self.x as isize
            && x <= (self.x + self.width) as isize
            && y >= self.y as isize
            && y <= (self.y + self.height) as isize
    }
}

#[derive(Clone, Default)]
pub struct SemanticZoneCache {
    seqno: SequenceNo,
    zones: Vec<StableRowIndex>,
}

pub struct OverlayState {
    pub pane: Arc<dyn Pane>,
    pub key_table_state: KeyTableState,
}

#[derive(Default)]
pub struct PaneState {
    /// If is_some(), the top row of the visible screen.
    /// Otherwise, the viewport is at the bottom of the
    /// scrollback.
    viewport: Option<StableRowIndex>,
    selection: Selection,
    /// If is_some(), rather than display the actual tab
    /// contents, we're overlaying a little internal application
    /// tab.  We'll also route input to it.
    pub overlay: Option<OverlayState>,

    bell_start: Option<Instant>,
    pub mouse_terminal_coords: Option<(ClickPosition, StableRowIndex)>,
}

/// Data used when synchronously formatting pane and window titles
#[derive(Debug, Clone)]
pub struct TabInformation {
    pub tab_id: TabId,
    pub tab_index: usize,
    pub is_active: bool,
    pub active_pane: Option<PaneInformation>,
    pub tab_title: String,
}

/// Data used when synchronously formatting pane and window titles
#[derive(Debug, Clone)]
pub struct PaneInformation {
    pub is_active: bool,
    pub is_zoomed: bool,
    pub title: String,
    pub progress: Progress,
    /// The active pane's current working directory, as reported by the
    /// pane's `get_current_working_dir`, rendered as a plain string.
    /// `None` if the pane hasn't reported a cwd yet.
    pub current_working_dir: Option<String>,
}

#[derive(Default)]
pub struct TabState {
    /// If is_some(), rather than display the actual tab
    /// contents, we're overlaying a little internal application
    /// tab.  We'll also route input to it.
    pub overlay: Option<OverlayState>,
}

/// Tracks the in-flight/pending state of named window events
/// (`emit_window_event`/`schedule_window_event`/`finish_window_event`).
/// We don't want to queue more than 1 event of a given name at a time,
/// so we use this enum to allow for at most 1 executing
/// and 1 pending event.
#[derive(Copy, Clone, Debug)]
enum EventState {
    /// The event is not running
    None,
    /// The event is running
    InProgress,
    /// The event is running, and we have another one ready to
    /// run once it completes
    InProgressWithQueued(Option<PaneId>),
}

pub struct TermWindow {
    pub window: Option<Window>,
    pub config: ConfigHandle,
    pub config_overrides: wezterm_dynamic::Value,
    os_parameters: Option<parameters::Parameters>,
    /// When we most recently received keyboard focus
    pub focused: Option<Instant>,
    fonts: Rc<FontConfiguration>,
    /// Window dimensions and dpi
    pub dimensions: Dimensions,
    pub window_state: WindowState,
    pub resizes_pending: usize,
    is_repaint_pending: bool,
    pending_scale_changes: LinkedList<resize::ScaleChange>,
    /// Terminal dimensions
    terminal_size: TerminalSize,
    pub mux_window_id: MuxWindowId,
    pub mux_window_id_for_subscriptions: Arc<Mutex<MuxWindowId>>,
    /// `true` when the mux subscription must be unsubscribed from.
    /// This is done asynchronously to avoid races between mux events.
    mux_subscription_dead: Arc<AtomicBool>,
    pub render_metrics: RenderMetrics,
    render_state: Option<RenderState>,
    input_map: InputMap,
    /// If is_some, the LEADER modifier is active until the specified instant.
    leader_is_down: Option<std::time::Instant>,
    dead_key_status: DeadKeyStatus,
    key_table_state: KeyTableState,
    show_tab_bar: bool,
    show_scroll_bar: bool,
    tab_bar: TabBarState,
    fancy_tab_bar: Option<box_model::ComputedElement>,
    pub right_status: String,
    pub left_status: String,
    last_ui_item: Option<UIItem>,
    /// Tracks whether the current mouse-down event is part of click-focus.
    /// If so, we ignore mouse events until released
    is_click_to_focus_window: bool,
    /// Coordinates of a button-down event that arrived while the window was
    /// still becoming focused (see `focused`). Some window managers/OSes
    /// (observed on Windows; <https://github.com/wezterm/wezterm/issues/2414>,
    /// <https://github.com/wezterm/wezterm/issues/5309>) synthesize an extra
    /// `WM_MOUSEMOVE` for the same coordinates immediately after the
    /// activating click is delivered. If forwarded to the pane, that
    /// synthetic, zero-motion Move is misread by mouse-reporting-aware
    /// programs (e.g. tmux) as a real drag and can clobber their selection/
    /// clipboard state. We remember the activating click's coordinates here
    /// and suppress exactly one immediately-following Move that lands on the
    /// same spot; any real motion or the next press/release clears it.
    suppress_move_after_focus_click: Option<(isize, isize)>,
    last_mouse_coords: (usize, i64),
    window_drag_position: Option<MouseEvent>,
    current_mouse_event: Option<MouseEvent>,
    prev_cursor: PrevCursorPos,
    last_scroll_info: RenderableDimensions,

    tab_state: RefCell<HashMap<TabId, TabState>>,
    pane_state: RefCell<HashMap<PaneId, PaneState>>,
    semantic_zones: HashMap<PaneId, SemanticZoneCache>,

    window_background: Vec<LoadedBackgroundLayer>,

    current_modifier_and_leds: (Modifiers, KeyboardLedStatus),
    current_mouse_buttons: Vec<MousePress>,
    current_mouse_capture: Option<MouseCapture>,

    opengl_info: Option<String>,

    /// Keeps track of double and triple clicks
    last_mouse_click: Option<LastMouseClick>,

    /// The URL over which we are currently hovering
    current_highlight: Option<Arc<Hyperlink>>,

    quad_generation: usize,
    shape_generation: usize,
    shape_cache: RefCell<LfuCache<ShapeCacheKey, anyhow::Result<Rc<Vec<ShapedInfo>>>>>,
    line_to_ele_shape_cache: RefCell<LfuCache<LineToEleShapeCacheKey, LineToElementShapeItem>>,

    line_state_cache: RefCell<LfuCacheU64<Arc<CachedLineState>>>,
    next_line_state_id: u64,

    line_quad_cache: RefCell<LfuCache<LineQuadCacheKey, LineQuadCacheValue>>,

    last_status_call: Instant,
    cursor_blink_state: RefCell<ColorEase>,
    blink_state: RefCell<ColorEase>,
    rapid_blink_state: RefCell<ColorEase>,

    palette: Option<ColorPalette>,

    ui_items_scratch: Vec<UIItem>,
    ui_items: arc_swap::ArcSwap<Vec<UIItem>>,
    dragging: Option<(UIItem, MouseEvent)>,

    modal: RefCell<Option<Rc<dyn Modal>>>,

    event_states: HashMap<String, EventState>,
    pub current_event: Option<Value>,
    has_animation: RefCell<Option<Instant>>,
    /// We use this to attempt to do something reasonable
    /// if we run out of texture space
    allow_images: AllowImage,
    scheduled_animation: RefCell<Option<Instant>>,
    /// Task #271: dedup/coalesce state for the unconditional (focus-independent)
    /// follow-up repaint scheduled when the per-tab frame-build budget
    /// (`tab_frame_build_budget_ms`) trips for a pane. `has_animation`/
    /// `scheduled_animation` above only get consumed by `paint_impl` when
    /// `self.focused.is_some()`, so a budget-trip on an unfocused window
    /// would otherwise never get a follow-up frame and the skipped rows
    /// would stay blank until the window is focused again. This field lets
    /// `schedule_budget_repaint` avoid stacking a redundant timer/notify
    /// when one is already pending, the same way `scheduled_animation`
    /// dedups for the focused/animation case.
    scheduled_budget_repaint: RefCell<Option<Instant>>,

    created: Instant,

    pub last_frame_duration: Duration,
    last_fps_check_time: Instant,
    num_frames: usize,
    pub fps: f32,

    connection_name: String,

    gl: Option<Rc<glium::backend::Context>>,
    webgpu: Option<Arc<WebGpuState>>,
    render_thread: Option<crate::renderthread::RenderThreadHandle>,
    /// One-shot guard for the render-thread hang supervisor (see
    /// `schedule_render_thread_hang_check`): set to `true` the moment this
    /// window has been torn down for an observed render-thread hang, so a
    /// supervision tick that fires after teardown was already kicked off
    /// (a race between the scheduled timer and the close completing) is a
    /// no-op instead of double-closing the window.
    render_thread_hang_handled: Cell<bool>,
    /// Structural (not timing-based) guard against more than one concurrent
    /// hang-check timer chain existing for this window (task #287).
    ///
    /// `schedule_render_thread_hang_check` -> `Timer::at` -> `notify` ->
    /// `check_render_thread_hang_tick` -> (re-)`schedule_render_thread_hang_check`
    /// is meant to be a single self-rearming chain per window, terminated
    /// only when a tick observes `render_thread_hang_handled == true` or
    /// `render_thread.is_none()`. Relying on that alone is timing-dependent:
    /// `handle_render_error_recovery` can set `render_thread_hang_handled`
    /// to suppress the *current* chain's next tick, but
    /// `finish_renderer_rebuild` later resets it back to `false` and starts
    /// a brand-new chain via `schedule_render_thread_hang_check`. If the
    /// old chain's already-pending timer tick fires *after* that reset
    /// (rather than during the rebuild window, as it normally does, since a
    /// rebuild takes ~2.3-2.9s against a ~2s poll interval) it observes a
    /// healthy `render_thread_hang_handled == false` +
    /// `render_thread == Some(...)` and re-arms itself too, leaving two
    /// timer chains running in parallel for the same window (and this can
    /// recur on each subsequent hang episode).
    ///
    /// This flag makes single-chained-ness structural instead: set to
    /// `true` the moment `schedule_render_thread_hang_check` actually arms
    /// a new timer, cleared to `false` at the very top of
    /// `check_render_thread_hang_tick` before any other logic runs.
    /// `schedule_render_thread_hang_check` early-returns without arming a
    /// new timer if this is already `true`, so even if
    /// `finish_renderer_rebuild` asks for a new chain while an old tick is
    /// still in flight, at most one chain is ever pending.
    hang_check_scheduled: Cell<bool>,
    /// Timestamps of recent in-place renderer rebuilds performed by
    /// `check_render_thread_hang_tick` in response to an observed hang, most
    /// recent last. This is the circuit breaker: `MAX_REBUILDS_PER_WINDOW`
    /// rebuilds within `REBUILD_WINDOW` of each other means the renderer
    /// keeps re-hanging immediately after every rebuild (a fundamentally
    /// broken adapter/driver/device, not a one-off transient stall), so we
    /// give up on rebuilding and fall back to the old destructive
    /// close-the-window behavior instead of looping forever. Entries older
    /// than `REBUILD_WINDOW` are pruned on every check, so this never grows
    /// unbounded across a long-lived window's lifetime.
    rebuild_attempts: RefCell<Vec<Instant>>,
    /// Per-window relay for `begin_opengl_fallback` / `finish_opengl_fallback`
    /// (task #255, race fixed in task #265): carries the
    /// `anyhow::Result<Rc<glium::backend::Context>>` produced by
    /// `Window::enable_opengl()` across the `TermWindowNotif::Apply`
    /// re-entry boundary.
    ///
    /// `Rc<glium::backend::Context>` is not `Send`/`Sync` (it wraps
    /// thread-affine GL context state), so it cannot be captured directly
    /// in an `Apply` closure, which `notify()` requires to be `Send + Sync`
    /// (that bound exists because `notify()` is a generic API callable from
    /// any thread, e.g. a background render thread posting back to the GUI
    /// thread). In this specific call chain the value never actually
    /// leaves the GUI thread -- `promise::spawn::spawn` is `spawn_local`
    /// (see `crates/promise/src/spawn.rs`), so both the write (in
    /// `begin_opengl_fallback`'s spawned task) and the read (in
    /// `finish_opengl_fallback`, reached via the `notify()` dispatch)
    /// happen on this same thread.
    ///
    /// This used to be a single process-wide `thread_local!` slot, keyed by
    /// nothing at all. That was a real bug (task #265): if two *different*
    /// windows both trip the render-rebuild circuit breaker around the same
    /// time (the expected case for a process-wide broken GPU
    /// adapter/driver), their `begin_opengl_fallback` spawned tasks can both
    /// be in flight before either `finish_opengl_fallback` runs (each
    /// `enable_opengl().await` yields the executor at least once, and
    /// `Window::notify` merely enqueues a `spawn_into_main_thread` task
    /// rather than dispatching synchronously), so window B's write could
    /// silently clobber window A's result in the shared cell, handing
    /// window A a `glium::backend::Context` that belongs to window B's
    /// HWND/device context and leaving window B's slot empty (which then
    /// reads back a spurious "relay was empty" error and destroys window
    /// B's tabs/panes even though B's own OpenGL init never failed).
    ///
    /// Scoping this to an `Rc<RefCell<...>>` owned by (and captured from)
    /// this specific `TermWindow`/`Window` eliminates the cross-window
    /// aliasing entirely: each window's spawned fallback task closes over
    /// its own `Rc` clone, so N windows falling back concurrently each get
    /// their own slot with no possibility of clobbering or starving each
    /// other. A single slot (rather than a queue) remains sufficient
    /// *per window*: `render_thread_hang_handled`'s one-shot guard still
    /// ensures `begin_opengl_fallback` isn't re-entered for a given window
    /// while a previous attempt's result hasn't been consumed yet.
    opengl_fallback_relay: Rc<RefCell<Option<anyhow::Result<Rc<::window::glium::backend::Context>>>>>,
    /// Set once (task #267) by `finish_opengl_fallback` the moment this
    /// window has *successfully* completed its one-shot OpenGL fallback
    /// (task #255): this window is done trying WebGpu again, permanently,
    /// for the rest of its lifetime -- by design, not as a transient state.
    ///
    /// Checked at the very top of `handle_render_error_recovery` (and
    /// `check_render_thread_hang_tick`, which reaches the same shared
    /// `attempt_renderer_rebuild_or_close`): once set, no recovery-triggering
    /// event (hang, surface error, device-lost) may begin a renderer rebuild
    /// for this window ever again. Without this flag, a stale/late
    /// device-lost event from the old, now-abandoned WebGpu device (see
    /// `WebGpuState::mark_stale`'s doc comment) that arrives more than
    /// `REBUILD_WINDOW` after the fallback completed would find
    /// `rebuild_attempts` pruned back down to nothing, pass the circuit
    /// breaker's count check, and tear down this window's healthy, currently
    /// in-use OpenGL `render_state`/`gl` to restart the WebGpu rebuild dance
    /// on a window that has permanently moved off WebGpu -- exactly the
    /// failure task #255's fallback exists to avoid.
    permanently_on_opengl: Cell<bool>,
    config_subscription: Option<config::ConfigSubscription>,
}

impl TermWindow {
    fn load_os_parameters(&mut self) {
        if let Some(ref window) = self.window {
            self.os_parameters = match window.get_os_parameters(&self.config, self.window_state) {
                Ok(os_parameters) => os_parameters,
                Err(err) => {
                    log::warn!("Error while getting OS parameters: {:#}", err);
                    None
                }
            };
        }
    }

    // OnlyTerm: never prompt on window close - close-confirmation overlays
    // are removed entirely, not just defaulted off via config.
    fn close_requested(&mut self, window: &Window) {
        let mux = Mux::get();
        mux.kill_window(self.mux_window_id);
        window.close();
        front_end().forget_known_window(window);
    }

    fn focus_changed(&mut self, focused: bool, window: &Window) {
        log::trace!("Setting focus to {:?}", focused);
        self.focused = if focused { Some(Instant::now()) } else { None };
        self.quad_generation += 1;
        self.load_os_parameters();

        if self.focused.is_none() {
            self.last_mouse_click = None;
            self.current_mouse_buttons.clear();
            self.current_mouse_capture = None;
            self.is_click_to_focus_window = false;
            self.suppress_move_after_focus_click = None;

            for state in self.pane_state.borrow_mut().values_mut() {
                state.mouse_terminal_coords.take();
            }
        }

        // Reset the cursor blink phase
        self.prev_cursor.bump();

        // force cursor to be repainted
        window.invalidate();

        if let Some(pane) = self.get_active_pane_or_overlay() {
            pane.focus_changed(focused);
        }

        self.update_title();
    }

    fn created(&mut self, ctx: RenderContext) -> anyhow::Result<()> {
        self.render_state = None;

        let render_info = ctx.renderer_info();
        self.opengl_info.replace(render_info.clone());

        match RenderState::new(ctx, &self.fonts, &self.render_metrics, ATLAS_SIZE) {
            Ok(render_state) => {
                log::debug!(
                    "OpenGL initialized! {} wezterm version: {}",
                    render_info,
                    config::wezterm_version(),
                );
                self.render_state.replace(render_state);
            }
            Err(err) => {
                log::error!("failed to create RenderState: {}", err);
                return Err(err).context(format!(
                    "failed to create RenderState for {}",
                    render_info
                ));
            }
        }

        Ok(())
    }
}

impl TermWindow {
    pub async fn new_window(mux_window_id: MuxWindowId) -> anyhow::Result<()> {
        let config = configuration();
        let dpi = config.dpi.unwrap_or_else(|| ::window::default_dpi()) as usize;
        let fontconfig = Rc::new(FontConfiguration::new(Some(config.clone()), dpi)?);

        let mux = Mux::get();
        let size = match mux.get_active_tab_for_window(mux_window_id) {
            Some(tab) => tab.get_size(),
            None => {
                log::debug!("new_window has no tabs... yet?");
                Default::default()
            }
        };
        let physical_rows = size.rows as usize;
        let physical_cols = size.cols as usize;

        let render_metrics = RenderMetrics::new(&fontconfig)?;
        log::trace!("using render_metrics {:#?}", render_metrics);

        // Initially we have only a single tab, so take that into account
        // for the tab bar state.
        let show_tab_bar = config.enable_tab_bar && !config.hide_tab_bar_if_only_one_tab;
        let tab_bar_height = if show_tab_bar {
            Self::tab_bar_pixel_height_impl(&config, &fontconfig, &render_metrics)? as usize
        } else {
            0
        };

        let terminal_size = TerminalSize {
            rows: physical_rows,
            cols: physical_cols,
            pixel_width: (render_metrics.cell_size.width as usize * physical_cols),
            pixel_height: (render_metrics.cell_size.height as usize * physical_rows),
            dpi: dpi as u32,
        };

        if terminal_size != size {
            // DPI is different from the default assumed DPI when the mux
            // created the pty. We need to inform the kernel of the revised
            // pixel geometry now
            log::trace!(
                "Initial geometry was {:?} but dpi-adjusted geometry \
                        is {:?}; update the kernel pixel geometry for the ptys!",
                size,
                terminal_size,
            );
            if let Some(window) = mux.get_window(mux_window_id) {
                for tab in window.iter() {
                    tab.resize(terminal_size);
                }
            };
        }

        let h_context = DimensionContext {
            dpi: dpi as f32,
            pixel_max: terminal_size.pixel_width as f32,
            pixel_cell: render_metrics.cell_size.width as f32,
        };
        let padding_left = config.window_padding.left.evaluate_as_pixels(h_context) as usize;
        let padding_right = resize::effective_right_padding(&config, h_context) as usize;
        let v_context = DimensionContext {
            dpi: dpi as f32,
            pixel_max: terminal_size.pixel_height as f32,
            pixel_cell: render_metrics.cell_size.height as f32,
        };
        let padding_top = config.window_padding.top.evaluate_as_pixels(v_context) as usize;
        let padding_bottom = config.window_padding.bottom.evaluate_as_pixels(v_context) as usize;

        let mut dimensions = Dimensions {
            pixel_width: (terminal_size.pixel_width + padding_left + padding_right) as usize,
            pixel_height: ((terminal_size.rows * render_metrics.cell_size.height as usize)
                + padding_top
                + padding_bottom) as usize
                + tab_bar_height,
            dpi,
        };

        let border = Self::get_os_border_impl(&None, &config, &dimensions, &render_metrics);

        dimensions.pixel_height += (border.top + border.bottom).get() as usize;
        dimensions.pixel_width += (border.left + border.right).get() as usize;

        let window_background = load_background_image(&config, &dimensions, &render_metrics);

        log::trace!(
            "TermWindow::new_window called with mux_window_id {} {:?} {:?}",
            mux_window_id,
            terminal_size,
            dimensions
        );

        let render_state = None;

        let connection_name = Connection::get().unwrap().name();

        let myself = Self {
            created: Instant::now(),
            connection_name,
            last_fps_check_time: Instant::now(),
            num_frames: 0,
            last_frame_duration: Duration::ZERO,
            fps: 0.,
            config_subscription: None,
            os_parameters: None,
            gl: None,
            webgpu: None,
            render_thread: None,
            render_thread_hang_handled: Cell::new(false),
            hang_check_scheduled: Cell::new(false),
            rebuild_attempts: RefCell::new(Vec::new()),
            opengl_fallback_relay: Rc::new(RefCell::new(None)),
            permanently_on_opengl: Cell::new(false),
            window: None,
            window_background,
            config: config.clone(),
            config_overrides: wezterm_dynamic::Value::default(),
            palette: None,
            focused: None,
            mux_window_id,
            mux_window_id_for_subscriptions: Arc::new(Mutex::new(mux_window_id)),
            mux_subscription_dead: Arc::new(AtomicBool::new(false)),
            fonts: Rc::clone(&fontconfig),
            render_metrics,
            dimensions,
            window_state: WindowState::default(),
            resizes_pending: 0,
            is_repaint_pending: false,
            pending_scale_changes: LinkedList::new(),
            terminal_size,
            render_state,
            input_map: InputMap::new(&config),
            leader_is_down: None,
            dead_key_status: DeadKeyStatus::None,
            show_tab_bar,
            show_scroll_bar: config.enable_scroll_bar,
            tab_bar: TabBarState::default(),
            fancy_tab_bar: None,
            right_status: String::new(),
            left_status: String::new(),
            last_mouse_coords: (0, -1),
            suppress_move_after_focus_click: None,
            window_drag_position: None,
            current_mouse_event: None,
            current_modifier_and_leds: Default::default(),
            prev_cursor: PrevCursorPos::new(),
            last_scroll_info: RenderableDimensions::default(),
            tab_state: RefCell::new(HashMap::new()),
            pane_state: RefCell::new(HashMap::new()),
            current_mouse_buttons: vec![],
            current_mouse_capture: None,
            last_mouse_click: None,
            current_highlight: None,
            quad_generation: 0,
            shape_generation: 0,
            shape_cache: RefCell::new(LfuCache::new(
                "shape_cache.hit.rate",
                "shape_cache.miss.rate",
                |config| config.shape_cache_size,
                &config,
            )),
            line_state_cache: RefCell::new(LfuCacheU64::new(
                "line_state_cache.hit.rate",
                "line_state_cache.miss.rate",
                |config| config.line_state_cache_size,
                &config,
            )),
            next_line_state_id: 0,
            line_quad_cache: RefCell::new(LfuCache::new(
                "line_quad_cache.hit.rate",
                "line_quad_cache.miss.rate",
                |config| config.line_quad_cache_size,
                &config,
            )),
            line_to_ele_shape_cache: RefCell::new(LfuCache::new(
                "line_to_ele_shape_cache.hit.rate",
                "line_to_ele_shape_cache.miss.rate",
                |config| config.line_to_ele_shape_cache_size,
                &config,
            )),
            last_status_call: Instant::now(),
            cursor_blink_state: RefCell::new(ColorEase::new(
                config.cursor_blink_rate,
                config.cursor_blink_ease_in,
                config.cursor_blink_rate,
                config.cursor_blink_ease_out,
                None,
            )),
            blink_state: RefCell::new(ColorEase::new(
                config.text_blink_rate,
                config.text_blink_ease_in,
                config.text_blink_rate,
                config.text_blink_ease_out,
                None,
            )),
            rapid_blink_state: RefCell::new(ColorEase::new(
                config.text_blink_rate_rapid,
                config.text_blink_rapid_ease_in,
                config.text_blink_rate_rapid,
                config.text_blink_rapid_ease_out,
                None,
            )),
            event_states: HashMap::new(),
            current_event: None,
            has_animation: RefCell::new(None),
            scheduled_animation: RefCell::new(None),
            scheduled_budget_repaint: RefCell::new(None),
            allow_images: AllowImage::Yes,
            semantic_zones: HashMap::new(),
            ui_items_scratch: vec![],
            ui_items: arc_swap::ArcSwap::new(std::sync::Arc::new(Vec::new())),
            dragging: None,
            last_ui_item: None,
            is_click_to_focus_window: false,
            key_table_state: KeyTableState::default(),
            modal: RefCell::new(None),
            opengl_info: None,
        };

        let tw = Rc::new(RefCell::new(myself));
        let tw_event = Rc::clone(&tw);

        let mut x = None;
        let mut y = None;
        let mut origin = GeometryOrigin::default();

        if let Some(position) = mux
            .get_window(mux_window_id)
            .and_then(|window| window.get_initial_position().clone())
            .or_else(|| POSITION.lock().unwrap().take())
        {
            x.replace(position.x);
            y.replace(position.y);
            origin = position.origin;
        }

        let geometry = RequestedWindowGeometry {
            width: Dimension::Pixels(dimensions.pixel_width as f32),
            height: Dimension::Pixels(dimensions.pixel_height as f32),
            x,
            y,
            origin,
        };
        log::trace!("{:?}", geometry);

        let window = Window::new_window(
            &get_window_class(),
            "OnlyTerm",
            geometry,
            Some(&config),
            Rc::clone(&fontconfig),
            move |event, window| {
                let mut tw = tw_event.borrow_mut();
                if let Err(err) = tw.dispatch_window_event(event, window) {
                    log::error!("dispatch_window_event: {:#}", err);
                }
            },
        )
        .await?;
        tw.borrow_mut().window.replace(window.clone());

        Self::apply_icon(&window)?;

        let config_subscription = config::subscribe_to_config_reload({
            let window = window.clone();
            move || {
                window.notify(TermWindowNotif::Apply(Box::new(|tw| {
                    tw.config_was_reloaded()
                })));
                true
            }
        });

        // Attempt WebGpu first (if requested) so that we can fall back to
        // OpenGL when adapter/device creation fails, rather than hard-failing
        // window creation. Only construct `gl` afterwards, and only if
        // WebGpu wasn't requested or just failed.
        let mut webgpu = None;
        if config.front_end == FrontEndSelection::WebGpu {
            match WebGpuState::new(&window, dimensions, &config).await {
                Ok(state) => {
                    webgpu.replace(Arc::new(state));
                }
                Err(err) => {
                    // WebGpu adapter/device creation can fail in RDP
                    // sessions, on old/software-only GPUs, in VMs without
                    // GPU passthrough, or due to driver mismatches. Rather
                    // than failing to open the window at all, fall back to
                    // OpenGL below.
                    log::error!(
                        "Failed to initialize WebGpu ({:#}); falling back to OpenGL rendering",
                        err
                    );
                }
            }
        }

        let gl = if webgpu.is_none() {
            Some(window.enable_opengl().await?)
        } else {
            None
        };

        {
            let mut myself = tw.borrow_mut();
            myself.config_subscription.replace(config_subscription);
            if config.use_resize_increments {
                window.set_resize_increments(
                    ResizeIncrementCalculator {
                        x: myself.render_metrics.cell_size.width as u16,
                        y: myself.render_metrics.cell_size.height as u16,
                        padding_left: padding_left,
                        padding_top: padding_top,
                        padding_right: padding_right,
                        padding_bottom: padding_bottom,
                        border: border,
                        tab_bar_height: tab_bar_height,
                    }
                    .into(),
                );
            }

            if let Some(gl) = gl {
                myself.gl.replace(Rc::clone(&gl));
                myself.created(RenderContext::Glium(Rc::clone(&gl)))?;
            }
            if let Some(webgpu) = webgpu {
                myself.webgpu.replace(Arc::clone(&webgpu));
                myself.created(RenderContext::WebGpu(Arc::clone(&webgpu)))?;

                if config.webgpu_render_thread {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let in_flight = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let repaint_pending = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let window_destroyed = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let submit_started_at = Arc::new(parking_lot::Mutex::new(None));
                    let seed = crate::renderthread::RenderThreadSeed {
                        window: window.clone(),
                        webgpu: Arc::clone(&webgpu),
                        rx,
                        in_flight,
                        repaint_pending,
                        window_destroyed,
                        submit_started_at,
                    };
                    myself.render_thread = crate::renderthread::RenderThreadHandle::spawn(
                        seed,
                        tx,
                        myself.mux_window_id,
                    );
                    if myself.render_thread.is_some() {
                        myself.schedule_render_thread_hang_check(&window);
                    }
                }
            }
            myself.load_os_parameters();
            window.show();
            if config.start_maximized {
                window.maximize();
            }
            myself.subscribe_to_pane_updates();
        }

        crate::update::start_update_checker();
        front_end().record_known_window(window, mux_window_id);

        Ok(())
    }

    /// Schedules the next tick of this window's render-thread hang
    /// supervisor. Self-rearming: each tick either closes the window (if its
    /// render thread is hung) or calls this again to schedule the next tick,
    /// exactly like `scheduled_animation`'s `Timer::at` + `notify` pattern in
    /// `paint_impl` reschedules itself.
    ///
    /// Only ever called (initially from `new_window`, then from
    /// `finish_renderer_rebuild` after a successful rebuild, and recursively
    /// from `check_render_thread_hang_tick`) while running on the GUI thread
    /// -- `promise::spawn::spawn` is GUI-thread-only (it uses `spawn_local`
    /// under the hood), which holds here since all call sites already run
    /// on the GUI thread.
    ///
    /// Guarded by `hang_check_scheduled` (task #287): if a chain is already
    /// pending for this window, this is a no-op rather than arming a second,
    /// concurrent chain. See that field's doc comment for the race this
    /// closes. The guard is set here, at the point a new timer is actually
    /// armed, and cleared at the very top of `check_render_thread_hang_tick`
    /// -- i.e. it tracks "is a tick currently in flight for this window",
    /// not "has a chain ever been started".
    fn schedule_render_thread_hang_check(&self, window: &Window) {
        if self.hang_check_scheduled.get() {
            // A chain is already pending (its timer tick hasn't fired yet);
            // do not start a second, concurrent chain. See this call's
            // doc comment and `hang_check_scheduled`'s own doc comment.
            return;
        }
        self.hang_check_scheduled.set(true);

        // Poll at a fraction of the hang threshold, the same style as
        // `window::os::windows::watchdog`'s `poll_interval = (threshold /
        // 4).max(Duration::from_millis(50))`. This check is cheaper than the
        // GUI watchdog's (just a `Mutex<Option<Instant>>` read, no syscalls),
        // so a smaller minimum is fine, but we still don't want a
        // misconfigured (very low) threshold to turn into a busy-poll.
        let threshold =
            Duration::from_millis(config::configuration().render_thread_hang_threshold_ms);
        let poll_interval = (threshold / 2).max(Duration::from_millis(500));
        let next_check = Instant::now() + poll_interval;

        let window = window.clone();
        promise::spawn::spawn(async move {
            Timer::at(next_check).await;
            let win = window.clone();
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.check_render_thread_hang_tick(&win);
            })));
        })
        .detach();
    }

    /// Circuit breaker thresholds for the in-place renderer rebuild
    /// performed by `check_render_thread_hang_tick`. If rebuilding the
    /// renderer doesn't actually fix things -- the GPU/driver/adapter is
    /// fundamentally broken rather than having suffered a one-off transient
    /// stall -- the render thread will simply hang again almost
    /// immediately after each rebuild. `3` rebuilds within `30` seconds is
    /// enough slack for a couple of unlucky-but-unrelated stalls (e.g. two
    /// independent brief driver hiccups minutes apart would never trip
    /// this), while still catching an immediate re-hang loop quickly: three
    /// full rebuild-and-rehang cycles within half a minute is well outside
    /// what a real transient stall looks like.
    const MAX_REBUILDS_PER_WINDOW: usize = 3;
    const REBUILD_WINDOW: Duration = Duration::from_secs(30);

    /// One tick of the render-thread hang supervisor: if this window's
    /// render thread appears hung, rebuild the renderer in place (new
    /// WebGpu device/surface, new render thread) so the window and all its
    /// tabs/panes survive -- unless the circuit breaker has tripped, in
    /// which case fall back to the old destructive close. Otherwise re-arms
    /// for another tick. See `schedule_render_thread_hang_check` for the
    /// scheduling half.
    fn check_render_thread_hang_tick(&mut self, window: &Window) {
        // Clear the "a chain is pending" guard before any other logic in
        // this tick runs (task #287): this tick *is* that pending chain
        // firing, so from this point on `schedule_render_thread_hang_check`
        // must be willing to arm a fresh timer again -- whether that
        // happens below (the `!hung` re-arm path) or later, from
        // `finish_renderer_rebuild` once an in-place rebuild triggered by
        // this same tick completes. Clearing it late (or conditionally)
        // would reopen the race this flag exists to close: a rebuild
        // finishing and calling `schedule_render_thread_hang_check` while
        // this flag was still `true` would be wrongly suppressed, leaving
        // this window with no supervisor at all.
        self.hang_check_scheduled.set(false);

        // Sweep any WebGpu child HWNDs retired by an earlier
        // `begin_renderer_rebuild` (task #283): destroys the ones whose
        // paired `Weak<WebGpuState>` has since hit zero strong references
        // (i.e. the old render thread has actually returned), leaving any
        // others -- still possibly referenced by a render thread that
        // hasn't unwedged yet -- in place for the next tick. Runs
        // unconditionally, before the early-return guards below, so this
        // ~2s cadence is what actually reclaims a retired HWND promptly
        // instead of leaving it until the window closes; see
        // `Window::sweep_retired_webgpu_children`'s doc comment for the
        // full rationale, including why leaving one unswept is never a
        // leak (the top-level window's own `WS_CHILD` teardown is a
        // backstop).
        #[cfg(windows)]
        window.sweep_retired_webgpu_children();

        if self.render_thread_hang_handled.get() {
            // Already rebuilding/closing this window for a hang detected on
            // an earlier tick; a tick that fires after that (a race between
            // the scheduled timer and the rebuild/close actually completing)
            // must be a no-op, not a double-rebuild or double-close.
            return;
        }
        let hung = match self.render_thread.as_ref() {
            Some(rt) => rt.render_thread_is_hung(),
            None => {
                // Render thread is gone (e.g. window already tearing down);
                // nothing left to supervise.
                return;
            }
        };
        if !hung {
            self.schedule_render_thread_hang_check(window);
            return;
        }

        self.attempt_renderer_rebuild_or_close(
            window,
            "this window's render thread appears stuck inside a GPU submit/reconfigure \
             call (not the whole app -- just this window's GPU driver call)",
            "this window's render thread has hung and been rebuilt",
            "gui.render_thread.window_renderer_rebuilt",
        );
    }

    /// Re-entry point (via `TermWindowNotif::Apply`) for render-error
    /// recovery signals raised from the render thread that aren't a plain
    /// hang: a `wgpu::SurfaceError` variant other than `Lost`/`Outdated`
    /// (see `renderthread::submit_one_frame`'s `other` branch), or a genuine
    /// wgpu device-lost event (see `WebGpuState::new`'s
    /// `set_device_lost_callback` registration). Both signals mean this
    /// window's GPU device/surface is in a broken state that a fresh
    /// `submit_frame` call won't recover from on its own, so this reuses
    /// exactly the same in-place rebuild (and circuit breaker) that
    /// `check_render_thread_hang_tick` uses for a stuck render thread --
    /// from the GUI thread's point of view, "the renderer needs rebuilding"
    /// is the same recovery action regardless of which symptom (hang vs.
    /// repeated surface error vs. device-lost) triggered it.
    ///
    /// Unlike `check_render_thread_hang_tick`, this does not check
    /// `render_thread_is_hung()` (the render thread may not be hung at all
    /// -- `submit_frame` returned promptly with an error, or the device-lost
    /// callback fired inline during a call that itself returned) and does
    /// not self-reschedule (it's not a polling loop; each call corresponds
    /// to one observed error event). It still honors
    /// `render_thread_hang_handled` as a one-shot-per-episode guard, exactly
    /// like the hang path, so a burst of repeated `SurfaceError::Other`
    /// values across several frames (or an error arriving while a
    /// hang-triggered rebuild is already in flight) collapses into a single
    /// rebuild attempt rather than one per event.
    pub(crate) fn handle_render_error_recovery(&mut self, window: &Window, reason: &str) {
        if self.render_thread_hang_handled.get() {
            // A rebuild (or close) for an earlier episode -- hang or error --
            // is already in flight; this event is redundant.
            return;
        }
        self.attempt_renderer_rebuild_or_close(
            window,
            reason,
            "this window's renderer has failed and been rebuilt",
            "gui.render_thread.window_renderer_rebuilt_after_error",
        );
    }

    /// Shared circuit-breaker bookkeeping + rebuild-or-close decision, used
    /// by both the render-thread hang supervisor
    /// (`check_render_thread_hang_tick`) and the render-error recovery entry
    /// point (`handle_render_error_recovery`). Callers are responsible for
    /// their own "should we even consider recovering right now" checks
    /// (hang detection, one-shot guard) before calling this; this function
    /// always sets the one-shot guard, records an attempt, and either
    /// rebuilds or closes.
    ///
    /// `log_reason` describes what was observed (used in the "rebuilding..."
    /// log line); `circuit_breaker_log_reason` is the shorter phrase used in
    /// the circuit-breaker-tripped log line; `rebuilt_metric` is the counter
    /// incremented when a rebuild is actually attempted, so the two call
    /// sites (hang vs. error recovery) remain distinguishable in metrics
    /// even though they now share this implementation.
    fn attempt_renderer_rebuild_or_close(
        &mut self,
        window: &Window,
        log_reason: &str,
        circuit_breaker_log_reason: &str,
        rebuilt_metric: &'static str,
    ) {
        if self.permanently_on_opengl.get() {
            // This window has already successfully, permanently fallen back
            // to OpenGL (task #255/#267): it is done trying WebGpu again by
            // design, so no recovery-triggering event -- including a late
            // device-lost callback from the old, now-abandoned WebGpu device
            // (see `WebGpuState::mark_stale`) that arrives well after the
            // fallback completed, once `rebuild_attempts` has been pruned
            // back down and would otherwise pass the circuit breaker's count
            // check -- may begin a new renderer rebuild attempt and tear
            // down this window's healthy, currently in-use OpenGL
            // `render_state`/`gl`. Just log and drop it.
            log::debug!(
                "{}; ignoring -- this window has already permanently fallen back to \
                 OpenGL rendering and will not attempt a WebGpu rebuild again",
                log_reason,
            );
            return;
        }

        // Set the one-shot guard immediately: everything below this point
        // (the circuit breaker check, the async rebuild, the fallback close)
        // must not race with another recovery trigger for this same
        // episode. It gets reset to `false` once a rebuild actually
        // succeeds (see `finish_renderer_rebuild`), so a *later*, separate
        // failure can also be recovered from -- this is "one-shot per
        // episode", not "one-shot ever".
        self.render_thread_hang_handled.set(true);

        let now = Instant::now();
        {
            let mut attempts = self.rebuild_attempts.borrow_mut();
            attempts.retain(|t| now.duration_since(*t) < Self::REBUILD_WINDOW);
            attempts.push(now);
        }
        let attempts_in_window = self.rebuild_attempts.borrow().len();

        if attempts_in_window > Self::MAX_REBUILDS_PER_WINDOW {
            log::error!(
                "{} {} times in the last {:?}; giving up on rebuilding WebGpu (the \
                 GPU/driver/adapter looks fundamentally broken, not just transiently \
                 stuck); attempting a one-time fallback to OpenGL for this window \
                 before resorting to closing it",
                circuit_breaker_log_reason,
                attempts_in_window,
                Self::REBUILD_WINDOW,
            );
            metrics::counter!("gui.render_thread.rebuild_circuit_breaker_tripped").increment(1);
            // No additional explicit backoff/delay is inserted here on
            // purpose: the circuit breaker already spaces out WebGpu
            // rebuild attempts by however long each rebuild-then-rehang
            // cycle actually takes in practice (observed ~2.3-2.9s per
            // cycle during task #253's manual testing, driven by real
            // HWND recreation + WebGpuState::new work, not a busy loop).
            // A synthetic sleep here would only add user-visible lag to a
            // window that's already visibly degraded, with no reliability
            // benefit -- the OpenGL fallback below is a one-shot attempt,
            // not something that benefits from spacing against itself.
            self.begin_opengl_fallback(window);
            return;
        }

        log::error!(
            "{}; rebuilding this window's renderer in place (attempt {} of {} allowed \
             within {:?}) so its tabs/panes survive",
            log_reason,
            attempts_in_window,
            Self::MAX_REBUILDS_PER_WINDOW,
            Self::REBUILD_WINDOW,
        );
        metrics::counter!(rebuilt_metric).increment(1);

        self.begin_renderer_rebuild(window);
    }

    /// The destructive fallback: kill this window's panes (and their child
    /// processes) before destroying the OS window, otherwise the
    /// shells/programs running in them are orphaned with no controlling
    /// terminal left. This is the same sequence `close_requested` and the
    /// original (pre-#253) hang handler used; it's now the true last resort,
    /// reached only when the in-place WebGpu rebuild's circuit breaker trips
    /// *and* the OpenGL fallback (`begin_opengl_fallback`, task #255) also
    /// fails.
    fn close_window_for_unrecoverable_render_hang(&mut self, window: &Window) {
        let mux = Mux::get();
        mux.kill_window(self.mux_window_id);
        window.close();
        front_end().forget_known_window(window);
        metrics::counter!("gui.render_thread.window_closed_for_hang").increment(1);
    }

    /// Last-resort renderer fallback (task #255), tried once the WebGpu
    /// in-place rebuild circuit breaker has tripped (`MAX_REBUILDS_PER_WINDOW`
    /// rebuilds within `REBUILD_WINDOW` all failed to produce a
    /// working/stable renderer): abandon WebGpu entirely for this window and
    /// switch it to OpenGL, exactly like the startup-time
    /// WebGpu->OpenGL fallback in `new_window` (task #221.8) does when
    /// `WebGpuState::new` fails on initial window creation. The difference
    /// here is that this window already has a live `RenderState`/render
    /// thread built on WebGpu that must be torn down first, and success
    /// permanently downgrades the window for the rest of its lifetime
    /// (there's no further attempt to go back to WebGpu).
    ///
    /// `enable_opengl` is `async`, so -- exactly like `begin_renderer_rebuild`
    /// bridges the async `WebGpuState::new` call -- the rest of the work
    /// happens in a spawned task that re-enters via `TermWindowNotif::Apply`
    /// (see `finish_opengl_fallback`). Unlike the WebGpu rebuild, no
    /// WebGpu-child-HWND recreation dance is needed first: `enable_opengl`
    /// (all platforms) operates on the main window/surface directly and
    /// doesn't touch `webgpu_child_hwnd` at all.
    fn begin_opengl_fallback(&mut self, window: &Window) {
        // Abandon the old WebGpu render thread and GPU resources, the same
        // careful way `begin_renderer_rebuild` does: shutdown (detach, don't
        // join -- a stuck driver call must not block the GUI thread), then
        // drop render_state before the device/surface it was built from.
        if let Some(rt) = self.render_thread.take() {
            rt.shutdown();
        }
        self.render_state.take();
        self.gl.take();
        // Mark the outgoing device stale (task #267) before dropping it: its
        // `set_device_lost_callback` closure can't be unregistered, so a
        // late device-lost event firing after this window has moved to
        // OpenGL must be able to tell it's no longer current and no-op
        // instead of dragging this window back into the WebGpu rebuild
        // dance. See `WebGpuState::mark_stale`'s doc comment.
        if let Some(webgpu) = self.webgpu.take() {
            webgpu.mark_stale();
        }

        let window_for_async = window.clone();
        // Clone this window's own relay slot (not a process-global one --
        // see the field's doc comment and task #265) so the spawned task
        // below can hand the result back to *this* window specifically,
        // with no risk of a concurrently-falling-back sibling window
        // clobbering or stealing it.
        let relay = Rc::clone(&self.opengl_fallback_relay);

        promise::spawn::spawn(async move {
            let result = window_for_async.enable_opengl().await;
            let win = window_for_async.clone();
            // `Rc<glium::backend::Context>` is not `Send`/`Sync` (unlike
            // `Arc<WebGpuState>` in the WebGpu rebuild path), but
            // `TermWindowNotif::Apply` requires its closure to be
            // `Send + Sync` -- `notify()`'s signature is generic over any
            // caller thread, even though in practice `promise::spawn::spawn`
            // is `spawn_local` and this closure only ever runs back on this
            // same GUI thread. Rather than reach for `unsafe impl Send`
            // (unsound in general, and unjustifiable here since
            // `glium::backend::Context` is genuinely thread-affine GL
            // state), relay the `Rc` itself through this window's own
            // GUI-thread-only relay slot (see `opengl_fallback_relay`'s doc
            // comment) and send only a `Send`-safe ok/err signal through
            // `notify`.
            relay.borrow_mut().replace(result);
            window_for_async.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.finish_opengl_fallback(&win);
            })));
        })
        .detach();
    }

    /// Re-entry point (via `TermWindowNotif::Apply`) once the async half of
    /// the OpenGL fallback (`enable_opengl`) has resolved.
    ///
    /// On success: rebuild `RenderState` against the new OpenGL context and
    /// log clearly that this window has permanently downgraded away from
    /// WebGpu, since -- per task #222's documented architectural decision --
    /// OpenGL rendering on this window is now synchronous on the GUI thread
    /// with none of WebGpu's render-thread/hang-detection/rebuild isolation.
    /// No render thread is (re-)armed and `schedule_render_thread_hang_check`
    /// is never scheduled again for this window: there's no render thread
    /// left to supervise. This is a deliberate, accepted tradeoff (a
    /// strictly less resilient renderer, chosen over closing the window and
    /// losing its tabs' child processes).
    ///
    /// On failure: this really is the end of the road -- both WebGpu and
    /// OpenGL failed to produce a working renderer for this window -- so
    /// fall back to the destructive close.
    ///
    /// Takes no `result` parameter: the actual `Rc<GliumContext>` (or
    /// error) was stashed in this window's own `opengl_fallback_relay` by
    /// `begin_opengl_fallback`'s spawned task just before this was
    /// `notify()`-ed, since a non-`Send` `Rc` cannot ride along in the
    /// `TermWindowNotif::Apply` closure itself. See that relay's doc
    /// comment for why this is safe (GUI-thread-only, and -- since task
    /// #265 -- per-window, so a concurrently-falling-back sibling window
    /// can never clobber or empty this window's slot).
    fn finish_opengl_fallback(&mut self, window: &Window) {
        let result = self.opengl_fallback_relay.borrow_mut().take().unwrap_or_else(|| {
            Err(anyhow::anyhow!(
                "internal error: opengl_fallback_relay was empty in finish_opengl_fallback"
            ))
        });
        let gl = match result {
            Ok(gl) => gl,
            Err(err) => {
                log::error!(
                    "OpenGL fallback also failed for this window after the WebGpu rebuild \
                     circuit breaker tripped ({:#}); no renderer is available, closing this \
                     window",
                    err
                );
                metrics::counter!("gui.render_thread.opengl_fallback_failed").increment(1);
                self.close_window_for_unrecoverable_render_hang(window);
                return;
            }
        };

        self.gl.replace(Rc::clone(&gl));
        if let Err(err) = self.created(RenderContext::Glium(Rc::clone(&gl))) {
            log::error!(
                "OpenGL fallback's RenderState build failed for this window ({:#}); no \
                 renderer is available, closing this window",
                err
            );
            metrics::counter!("gui.render_thread.opengl_fallback_failed").increment(1);
            self.close_window_for_unrecoverable_render_hang(window);
            return;
        }

        // Success: this window is now permanently on OpenGL. No render
        // thread to spawn/supervise for it any more (WebGpu-only
        // machinery), so -- unlike `finish_renderer_rebuild` -- there is no
        // `schedule_render_thread_hang_check` call here.
        self.render_thread_hang_handled.set(false);

        // This window has genuinely, successfully moved to OpenGL for good
        // (task #267): from this point on, no recovery-triggering event --
        // including a late device-lost callback from the old, now-abandoned
        // WebGpu device (see `WebGpuState::mark_stale`) -- may start a new
        // WebGpu rebuild attempt for this window. See
        // `attempt_renderer_rebuild_or_close`'s guard and this flag's own
        // doc comment.
        self.permanently_on_opengl.set(true);

        // One-shot sweep of any WebGpu child HWNDs retired by the failed
        // rebuild attempts that led to this fallback (task #292, issue 2):
        // once this window is permanently on OpenGL, `schedule_render_thread_hang_check`
        // is never armed again (see this function's own doc comment), so the
        // `check_render_thread_hang_tick` cadence that normally sweeps
        // `retired_webgpu_children` promptly (task #283) will never run
        // again for this window either -- without this call, up to
        // `MAX_REBUILDS_PER_WINDOW` retired HWNDs from the rebuild attempts
        // above would sit hidden-but-technically-alive until the window
        // itself closes (still not a real leak, since `hwnd`'s own
        // `WS_CHILD` teardown is a backstop -- see
        // `retired_webgpu_children`'s doc comment -- just not reclaimed
        // promptly). A single sweep here is sufficient and nothing needs to
        // keep polling afterward: WebGpu rebuilds have stopped for this
        // window permanently, so no *new* HWNDs will ever be retired after
        // this point. Safe to call even if nothing was retired (plain
        // no-op per `sweep_retired_webgpu_children`'s doc comment).
        #[cfg(windows)]
        window.sweep_retired_webgpu_children();

        window.invalidate();

        log::warn!(
            "this window has permanently fallen back to OpenGL rendering after repeated \
             WebGpu renderer rebuild failures (circuit breaker tripped {} times within \
             {:?}); OpenGL rendering on this window is synchronous on the GUI thread with \
             none of WebGpu's render-thread isolation/hang-detection/rebuild machinery \
             (see task #222) -- this is an accepted resilience tradeoff to keep the window \
             and its tabs' processes alive rather than closing it",
            Self::MAX_REBUILDS_PER_WINDOW,
            Self::REBUILD_WINDOW,
        );
        metrics::counter!("gui.render_thread.window_fallen_back_to_opengl").increment(1);
    }

    /// Kick off the async half of the in-place renderer rebuild (abandoning
    /// the old render thread and dropping the old GPU resources are cheap
    /// and synchronous, so they happen here; `WebGpuState::new` is `async`,
    /// so the rest is done in a spawned task, mirroring the established
    /// pattern in `schedule_render_thread_hang_check` for bridging sync
    /// code -> async GUI-thread-only work -> re-entry via
    /// `TermWindowNotif::Apply`).
    fn begin_renderer_rebuild(&mut self, window: &Window) {
        // Grab the outgoing render thread's teardown sentinel (task #292)
        // *before* taking/shutting it down below: `RenderThreadHandle::
        // teardown_sentinel` only exists on the handle itself, so this has
        // to happen while `self.render_thread` still holds it. See that
        // method's doc comment for why this -- not a `Weak` obtained by
        // downgrading `self.webgpu` -- is the correct signal for
        // `recreate_webgpu_child_window`/`sweep_retired_webgpu_children` to
        // poll: a `Weak<WebGpuState>`'s strong count can read zero while
        // `WebGpuState::drop` (and the `wgpu::Surface`/DXGI swapchain
        // teardown inside it) is still running on the render thread, since
        // `Arc::drop` decrements the strong count before running the
        // value's own `drop_in_place`. The sentinel instead only reports
        // zero strong references once the render thread has fully returned
        // from `render_thread_loop`, strictly after every `Arc<WebGpuState>`
        // clone it held has already been dropped.
        //
        // Only actually consumed on Windows (`recreate_webgpu_child_window`
        // below), since that's the only platform with a dedicated WebGpu
        // child HWND to retire in the first place; the `#[allow]` avoids an
        // unused-variable warning on other platforms where it's computed
        // but never read.
        #[allow(unused_variables)]
        let old_webgpu_weak: std::sync::Weak<dyn std::any::Any + Send + Sync> = self
            .render_thread
            .as_ref()
            .map(|rt| rt.teardown_sentinel())
            .unwrap_or_else(|| {
                std::sync::Weak::<()>::new() as std::sync::Weak<dyn std::any::Any + Send + Sync>
            });

        // Step 1: abandon the old render thread. Detach, don't join --
        // exactly like the `Destroyed` handler: a stuck GPU driver call
        // can't freeze the GUI thread, so blocking here via `.join()` would
        // defeat the whole purpose of having a separate render thread.
        // Sending `Shutdown` (which also sets `window_destroyed` on the
        // shared flag) is enough to let the thread's `recv()` loop end on
        // its own, whenever the driver call it may currently be stuck in
        // eventually returns.
        if let Some(rt) = self.render_thread.take() {
            rt.shutdown();
        }

        // Step 2: drop the old GPU resources in the same order the
        // `Destroyed` handler documents: render_state first (its Drop
        // deletes programs/buffers/textures/glyph atlas via the context),
        // then the context itself (gl) / device+surface (webgpu). The old
        // `webgpu` Arc may still be referenced by the just-shutdown render
        // thread until its `recv()` loop actually observes the disconnect,
        // but that's fine -- `Arc` keeps it alive until the last reference
        // (here, or on that thread) drops it, and the render thread will
        // never issue another GPU call against it once `window_destroyed`
        // is set (see `RenderThreadHandle::shutdown`'s doc comment).
        self.render_state.take();
        self.gl.take();
        // Mark the outgoing device stale (task #267) before dropping it, for
        // the same reason `begin_opengl_fallback` does: its
        // `set_device_lost_callback` closure keeps living (wgpu gives no way
        // to unregister it) for as long as the underlying `wgpu::Device`
        // handle does, so a *late* device-lost event from this now-abandoned
        // device must be able to tell it's stale and no-op, instead of
        // charging a spurious rebuild attempt against the freshly-rebuilt,
        // perfectly healthy device that replaces it below.
        if let Some(webgpu) = self.webgpu.take() {
            webgpu.mark_stale();
        }

        let window_for_async = window.clone();
        let dimensions = self.dimensions;
        let config = self.config.clone();

        promise::spawn::spawn(async move {
            // Step 3: retire the old WebGpu child HWND and create a fresh
            // one, *before* rebuilding `WebGpuState` below. This has to
            // happen ahead of the `WebGpuState::new` call, not after it:
            // `WebGpuState::new` picks whichever child HWND
            // `window.webgpu_child_hwnd()` currently returns, so rebuilding
            // the surface against the *old* child HWND (the one whose
            // swapchain may itself be the thing that's wedged) would defeat
            // the entire point of task #252's dedicated child HWND.
            //
            // This can't run synchronously back in `begin_renderer_rebuild`
            // (unlike steps 1-2 above): `Window::recreate_webgpu_child_window`
            // needs to borrow this window's `WindowInner`, but
            // `begin_renderer_rebuild` is always reached synchronously from
            // inside `notify()`'s dispatch, which is itself invoked from
            // `Connection::with_window_inner` while that exact `WindowInner`
            // is already mutably borrowed -- a synchronous re-borrow here
            // panics with "already mutably borrowed" (hit in this task's own
            // manual verification). `recreate_webgpu_child_window` is
            // `async` and internally defers its borrow via
            // `promise::spawn::spawn` for exactly this reason (see its doc
            // comment), so awaiting it here, one spawned task removed from
            // the original `notify()` call, is what actually avoids the
            // re-entrant borrow.
            #[cfg(windows)]
            if let Err(err) = window_for_async
                .recreate_webgpu_child_window(old_webgpu_weak)
                .await
            {
                let win = window_for_async.clone();
                window_for_async.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    tw.finish_renderer_rebuild(&win, Err(err));
                })));
                return;
            }

            let result = WebGpuState::new(&window_for_async, dimensions, &config).await;
            let win = window_for_async.clone();
            window_for_async.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.finish_renderer_rebuild(&win, result);
            })));
        })
        .detach();
    }

    /// Re-entry point (via `TermWindowNotif::Apply`) once the async half of
    /// the rebuild (`WebGpuState::new`) has resolved. On success, rebuilds
    /// `RenderState` against the new device and spawns a fresh render
    /// thread, mirroring `new_window`'s original setup sequence.
    ///
    /// There are two sequential failure points here -- `WebGpuState::new`
    /// itself failing, and (task #272) `self.created` (i.e. `RenderState::new`:
    /// shader compilation, glyph atlas allocation, etc.) failing even though
    /// `WebGpuState::new` just succeeded -- and both re-enter the same
    /// circuit-breaker-gated path (`attempt_renderer_rebuild_or_close`) that
    /// got us here, rather than closing the window immediately. Rationale
    /// (task #255, extended by #272): either failure is, from the circuit
    /// breaker's point of view, just another WebGpu rebuild attempt that
    /// didn't pan out -- exactly like a rebuild that "succeeds" (returns a
    /// device) but then immediately re-hangs, which the breaker already
    /// tolerates up to `MAX_REBUILDS_PER_WINDOW` times. Treating either of
    /// these failures differently (skip straight to close, bypassing the
    /// retry budget entirely) would be an inconsistency with no real
    /// justification: all three symptoms mean "this WebGpu attempt didn't
    /// produce a working renderer", and the breaker's 3-attempts/30s budget
    /// is exactly the mechanism designed to decide when to stop retrying and
    /// fall back. So a failure at either point counts as one attempt against
    /// that same budget, and re-calling `attempt_renderer_rebuild_or_close`
    /// naturally either retries WebGpu again (if attempts remain) or, once
    /// the breaker trips, falls back to OpenGL (see that function) before
    /// ever reaching the destructive close.
    fn finish_renderer_rebuild(
        &mut self,
        window: &Window,
        result: anyhow::Result<WebGpuState>,
    ) {
        // Opportunistic extra sweep (task #283): by the time `WebGpuState::new`
        // (real adapter/device/surface setup work, not instantaneous) has
        // resolved, the old render thread this rebuild abandoned has
        // usually already observed the shutdown signal and returned,
        // dropping its `Arc<WebGpuState>`. Sweeping here means the common
        // case -- a healthy render thread that wasn't really wedged, just
        // slow -- gets its retired HWND destroyed right away instead of
        // waiting for the next ~2s `check_render_thread_hang_tick`. Not
        // load-bearing: if the old thread is still wedged, this is a
        // harmless no-op and the next tick (or eventually window close)
        // will catch it once it's safe.
        #[cfg(windows)]
        window.sweep_retired_webgpu_children();

        let webgpu = match result {
            Ok(state) => Arc::new(state),
            Err(err) => {
                // Same failure modes `WebGpuState::new` can hit at initial
                // window creation: RDP session, no GPU passthrough in a VM,
                // a driver mismatch, etc. Retry through the circuit
                // breaker (see doc comment above) instead of closing
                // immediately; `render_thread_hang_handled` is still `true`
                // from the original `attempt_renderer_rebuild_or_close`
                // call that led here, so this is safe to re-enter directly
                // without re-checking it.
                log::error!(
                    "failed to rebuild WebGpu renderer after a render-thread hang ({:#}); \
                     retrying through the rebuild circuit breaker",
                    err
                );
                metrics::counter!("gui.render_thread.rebuild_failed").increment(1);
                self.attempt_renderer_rebuild_or_close(
                    window,
                    "the previous WebGpu rebuild attempt itself failed to create a device/surface",
                    "the WebGpu rebuild attempt has failed",
                    "gui.render_thread.window_renderer_rebuilt",
                );
                return;
            }
        };

        // The WebGpu child HWND was already retired and a fresh one
        // recreated (task #283 onward: the old HWND is *retired*, not
        // destroyed here -- it's swept later once the outgoing render
        // thread's `WebGpuState` has actually dropped, see
        // `sweep_retired_webgpu_children` above) via the spawned task in
        // `begin_renderer_rebuild` that awaits
        // `recreate_webgpu_child_window`, before `WebGpuState::new` was even
        // called. So the surface/device just resolved above already targets
        // the fresh child HWND. Nothing left to do for the HWND here.
        self.webgpu.replace(Arc::clone(&webgpu));
        if let Err(err) = self.created(RenderContext::WebGpu(Arc::clone(&webgpu))) {
            // Same reasoning as the `WebGpuState::new` failure arm above
            // (task #272): the device/surface rebuild itself just
            // succeeded, so this is a `RenderState::new` failure (shader
            // compilation, glyph atlas allocation, etc.) on top of a
            // healthy device -- still just another WebGpu rebuild attempt
            // that didn't pan out, from the circuit breaker's point of
            // view. Retry through it instead of closing immediately, so a
            // transient failure right after a device rebuild gets the same
            // retry/OpenGL-fallback chance as any other rebuild hiccup.
            // `self.created` already reset `self.render_state` to `None` on
            // this failure, and the next `begin_renderer_rebuild` (if the
            // breaker allows another attempt) will mark this `self.webgpu`
            // stale and clear it before creating a fresh one, so no stale
            // partial state is left around for a subsequent attempt to trip
            // over.
            log::error!(
                "failed to rebuild RenderState after a successful WebGpu device/surface \
                 rebuild ({:#}); retrying through the rebuild circuit breaker",
                err
            );
            metrics::counter!("gui.render_thread.rebuild_failed").increment(1);
            self.attempt_renderer_rebuild_or_close(
                window,
                "RenderState build failed after a successful device/surface rebuild",
                "the WebGpu rebuild attempt has failed",
                "gui.render_thread.window_renderer_rebuilt",
            );
            return;
        }

        let config = config::configuration();
        if config.webgpu_render_thread {
            let (tx, rx) = std::sync::mpsc::channel();
            let in_flight = Arc::new(AtomicBool::new(false));
            let repaint_pending = Arc::new(AtomicBool::new(false));
            let window_destroyed = Arc::new(AtomicBool::new(false));
            let submit_started_at = Arc::new(parking_lot::Mutex::new(None));
            let seed = crate::renderthread::RenderThreadSeed {
                window: window.clone(),
                webgpu: Arc::clone(&webgpu),
                rx,
                in_flight,
                repaint_pending,
                window_destroyed,
                submit_started_at,
            };
            self.render_thread =
                crate::renderthread::RenderThreadHandle::spawn(seed, tx, self.mux_window_id);
            if self.render_thread.is_some() {
                self.schedule_render_thread_hang_check(window);
            }
        }

        // The rebuild succeeded and a fresh render thread (if configured)
        // is running: re-arm the one-shot guard so a later, separate hang
        // on this same window can also be recovered from.
        self.render_thread_hang_handled.set(false);

        // The old frame's content is gone (new device, new/blank surface);
        // force a full repaint rather than waiting for the next organic
        // invalidate.
        window.invalidate();

        log::info!(
            "successfully rebuilt this window's WebGpu renderer in place after a \
             render-thread hang; window and all its tabs/panes survived"
        );
    }

    fn dispatch_window_event(
        &mut self,
        event: WindowEvent,
        window: &Window,
    ) -> anyhow::Result<bool> {
        log::debug!("{event:?}");
        match event {
            WindowEvent::Destroyed => {
                // Ensure that we cancel any overlays we had running, so
                // that the mux can empty out, otherwise the mux keeps
                // the TermWindow alive via the frontend even though
                // the window is gone and we'll linger forever.
                // <https://github.com/wezterm/wezterm/issues/3522>
                self.clear_all_overlays();
                // Drop OpenGL/render resources while the window surface is still
                // alive, before NSView dealloc invalidates the GPU drawable.
                // If deferred until TermWindow::drop, glium's Drop impls
                // (RawProgram, Context, VertexBuffer, etc.) call make_current
                // which triggers NSOpenGLContext update on a stale IOSurface,
                // causing SIGABRT.
                // Order matters: render_state first (its Drop deletes programs,
                // buffers, textures via the context), then gl (drops the
                // context itself, which does FBO/VAO/sampler cleanup).
                self.render_state.take();
                self.gl.take();
                // Detach, don't join: the whole point of the render
                // thread is that a stuck GPU driver call can't freeze the
                // GUI thread, so blocking window-close on that same thread
                // via .join() would defeat the purpose. Sending Shutdown
                // (and, failing that, dropping the handle's Sender, which
                // disconnects the channel) is enough to let the thread's
                // recv() loop end on its own, whenever the driver call it
                // may currently be stuck in eventually returns.
                if let Some(rt) = self.render_thread.take() {
                    rt.shutdown();
                }
                Ok(false)
            }
            WindowEvent::CloseRequested => {
                self.close_requested(window);
                Ok(true)
            }
            WindowEvent::AppearanceChanged(appearance) => {
                log::debug!("Appearance is now {:?}", appearance);
                // This is a bit fugly; we get per-window notifications
                // for appearance changes which successfully updates the
                // per-window config, but we need to explicitly tell the
                // global config to reload, otherwise things that acces
                // the config via config::configuration() will see the
                // prior version of the config.
                // What's fugly about this is that we'll reload the
                // global config here once per window, which could
                // be nasty for folks with a lot of windows.
                // <https://github.com/wezterm/wezterm/issues/2295>
                config::reload();
                self.config_was_reloaded();
                Ok(true)
            }
            WindowEvent::PerformKeyAssignment(action) => {
                if let Some(pane) = self.get_active_pane_or_overlay() {
                    self.perform_key_assignment(&pane, &action)?;
                    window.invalidate();
                }
                Ok(true)
            }
            WindowEvent::FocusChanged(focused) => {
                self.focus_changed(focused, window);
                Ok(true)
            }
            WindowEvent::MouseEvent(event) => {
                self.mouse_event_impl(event, window);
                Ok(true)
            }
            WindowEvent::MouseLeave => {
                self.mouse_leave_impl(window);
                Ok(true)
            }
            WindowEvent::Resized {
                dimensions,
                window_state,
                live_resizing,
            } => {
                self.resize(dimensions, window_state, window, live_resizing);
                Ok(true)
            }
            WindowEvent::SetInnerSizeCompleted => {
                self.resizes_pending -= 1;
                if self.is_repaint_pending {
                    self.is_repaint_pending = false;
                    if self.webgpu.is_some() {
                        self.do_paint_webgpu()?;
                    } else {
                        self.do_paint(window);
                    }
                }
                self.apply_pending_scale_changes();
                Ok(true)
            }
            WindowEvent::AdviseModifiersLedStatus(modifiers, leds) => {
                self.current_modifier_and_leds = (modifiers, leds);
                self.update_title();
                window.invalidate();
                Ok(true)
            }
            WindowEvent::RawKeyEvent(event) => {
                self.raw_key_event_impl(event, window);
                Ok(true)
            }
            WindowEvent::KeyEvent(event) => {
                self.key_event_impl(event, window);
                Ok(true)
            }
            WindowEvent::AdviseDeadKeyStatus(status) => {
                if self.config.debug_key_events {
                    log::info!("DeadKeyStatus now: {:?}", status);
                } else {
                    log::trace!("DeadKeyStatus now: {:?}", status);
                }
                self.dead_key_status = status;
                self.update_title();
                // Ensure that we repaint so that any composing
                // text is updated
                window.invalidate();
                Ok(true)
            }
            WindowEvent::NeedRepaint => {
                if self.resizes_pending > 0 {
                    self.is_repaint_pending = true;
                    Ok(true)
                } else if self.webgpu.is_some() {
                    self.do_paint_webgpu()
                } else {
                    Ok(self.do_paint(window))
                }
            }
            WindowEvent::Notification(item) => {
                if let Ok(notif) = item.downcast::<TermWindowNotif>() {
                    self.dispatch_notif(*notif, window)
                        .context("dispatch_notif")?;
                }
                Ok(true)
            }
            WindowEvent::DroppedString(text) => {
                let pane = match self.get_active_pane_or_overlay() {
                    Some(pane) => pane,
                    None => return Ok(true),
                };
                pane.send_paste(text.as_str())?;
                Ok(true)
            }
            WindowEvent::DroppedUrl(urls) => {
                let pane = match self.get_active_pane_or_overlay() {
                    Some(pane) => pane,
                    None => return Ok(true),
                };
                let urls = urls
                    .iter()
                    .map(|url| self.config.quote_dropped_files.escape(&url.to_string()))
                    .collect::<Vec<_>>()
                    .join(" ")
                    + " ";
                pane.send_paste(urls.as_str())?;
                Ok(true)
            }
            WindowEvent::DroppedFile(paths) => {
                let pane = match self.get_active_pane_or_overlay() {
                    Some(pane) => pane,
                    None => return Ok(true),
                };
                let paths = paths
                    .iter()
                    .map(|path| {
                        self.config
                            .quote_dropped_files
                            .escape(&path.to_string_lossy())
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
                    + " ";
                pane.send_paste(&paths)?;
                Ok(true)
            }
            WindowEvent::DraggedFile(_) => Ok(true),
        }
    }

    fn do_paint(&mut self, window: &Window) -> bool {
        let gl = match self.gl.as_ref() {
            Some(gl) => gl,
            None => return false,
        };

        if gl.is_context_lost() {
            log::error!("opengl context was lost; should reinit");
            // Mirror close_requested's teardown: kill this window's panes
            // (and their child processes) before destroying the OS window,
            // otherwise the shells/programs running in them are orphaned
            // with no controlling terminal left.
            let mux = Mux::get();
            mux.kill_window(self.mux_window_id);
            window.close();
            front_end().forget_known_window(window);
            return false;
        }

        let mut frame = glium::Frame::new(
            Rc::clone(&gl),
            (
                self.dimensions.pixel_width as u32,
                self.dimensions.pixel_height as u32,
            ),
        );
        self.paint_impl(&mut RenderFrame::Glium(&mut frame));
        window.finish_frame(frame).is_ok()
    }

    fn do_paint_webgpu(&mut self) -> anyhow::Result<bool> {
        let dims = self.dimensions;
        self.resize_webgpu_surface(dims);
        match self.do_paint_webgpu_impl() {
            Ok(ok) => Ok(ok),
            Err(err) => {
                match err.downcast_ref::<wgpu::SurfaceError>() {
                    // Note: with a render thread active, `do_paint_webgpu_impl`
                    // (via `paint_impl` -> `call_draw` -> `call_draw_webgpu`,
                    // see 221.5) never actually returns a `SurfaceError` --
                    // frames are handed off to `send_frame` and this always
                    // returns `Ok(())`. So this retry branch is effectively
                    // dead code in render-thread mode; it remains the
                    // correct/only recovery path when the render thread is
                    // inactive (flag off, non-Windows, or spawn failed), so
                    // it's left in place rather than removed.
                    Some(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        let dims = self.dimensions;
                        self.resize_webgpu_surface(dims);
                        return self.do_paint_webgpu_impl();
                    }
                    _ => {}
                }
                Err(err)
            }
        }
    }

    fn do_paint_webgpu_impl(&mut self) -> anyhow::Result<bool> {
        self.paint_impl(&mut RenderFrame::WebGpu);
        Ok(true)
    }

    fn dispatch_notif(&mut self, notif: TermWindowNotif, window: &Window) -> anyhow::Result<()> {
        match notif {
            TermWindowNotif::InvalidateShapeCache => {
                self.shape_generation += 1;
                self.shape_cache.borrow_mut().clear();
                self.invalidate_modal();
                window.invalidate();
            }
            TermWindowNotif::PerformAssignment {
                pane_id,
                assignment,
                tx,
            } => {
                let mux = Mux::get();
                let result = || -> anyhow::Result<()> {
                    // The CopyMode overlay doesn't exist in the mux, but aliases
                    // itself with the overlaid pane's pane_id.
                    // So we do a bit of fancy footwork here to resolve the overlay
                    // and use that if it has the same pane_id, but otherwise fall
                    // back to what we get from the mux.
                    // <https://github.com/wezterm/wezterm/issues/3209>
                    let active_pane = self
                        .get_active_pane_or_overlay()
                        .ok_or_else(|| anyhow!("there is no active pane!?"))?;
                    let pane = if active_pane.pane_id() == pane_id {
                        active_pane
                    } else {
                        mux.get_pane(pane_id)
                            .ok_or_else(|| anyhow!("pane id {} is not valid", pane_id))?
                    };
                    self.perform_key_assignment(&pane, &assignment)
                        .context("perform_key_assignment")?;
                    Ok(())
                }();
                window.invalidate();
                if let Some(tx) = tx {
                    tx.try_send(result).ok();
                }
            }
            TermWindowNotif::CancelOverlayForPane(pane_id) => {
                self.cancel_overlay_for_pane(pane_id);
            }
            TermWindowNotif::CancelOverlayForTab { tab_id, pane_id } => {
                self.cancel_overlay_for_tab(tab_id, pane_id);
            }
            TermWindowNotif::MuxNotification(n) => match n {
                MuxNotification::Alert {
                    alert: Alert::SetUserVar { name, value },
                    pane_id,
                } => {
                    self.emit_user_var_event(pane_id, name, value);
                }
                MuxNotification::WindowTitleChanged { .. }
                | MuxNotification::Alert {
                    alert:
                        Alert::OutputSinceFocusLost
                        | Alert::CurrentWorkingDirectoryChanged
                        | Alert::WindowTitleChanged(_)
                        | Alert::TabTitleChanged(_)
                        | Alert::IconTitleChanged(_)
                        | Alert::Progress(_),
                    ..
                } => {
                    self.update_title();
                }
                MuxNotification::Alert {
                    alert: Alert::PaletteChanged,
                    pane_id,
                } => {
                    // Shape cache includes color information, so
                    // ensure that we invalidate that as part of
                    // this overall invalidation for the palette
                    self.dispatch_notif(TermWindowNotif::InvalidateShapeCache, window)?;
                    self.mux_pane_output_event(pane_id);
                }
                MuxNotification::Alert {
                    alert: Alert::Bell,
                    pane_id,
                } => {
                    if !self.window_contains_pane(pane_id) {
                        return Ok(());
                    }

                    match self.config.audible_bell {
                        AudibleBell::SystemBeep => {
                            Connection::get().expect("on main thread").beep();
                        }
                        AudibleBell::Disabled => {}
                    }

                    log::trace!("Ding! (this is the bell) in pane {}", pane_id);

                    let mut per_pane = self.pane_state(pane_id);
                    per_pane.bell_start.replace(Instant::now());
                    window.invalidate();
                }
                MuxNotification::Alert {
                    alert: Alert::ToastNotification { .. },
                    ..
                } => {}
                MuxNotification::TabAddedToWindow {
                    window_id: _,
                    tab_id,
                } => {
                    let mux = Mux::get();
                    let mut size = self.terminal_size;
                    if let Some(tab) = mux.get_tab(tab_id) {
                        // If we attached to a remote domain and loaded in
                        // a tab async, we need to fixup its size, either
                        // by resizing it or resizes ourselves.
                        // The strategy here is to adjust both by taking
                        // the maximal size in both horizontal and vertical
                        // dimensions and applying that. In practice that
                        // means that a new local client will resize larger
                        // to adjust to the size of an existing client.
                        let tab_size = tab.get_size();
                        size.rows = size.rows.max(tab_size.rows);
                        size.cols = size.cols.max(tab_size.cols);

                        if size.rows != self.terminal_size.rows
                            || size.cols != self.terminal_size.cols
                            || size.pixel_width != self.terminal_size.pixel_width
                            || size.pixel_height != self.terminal_size.pixel_height
                        {
                            self.set_window_size(size, window)?;
                        } else if tab_size.dpi == 0 {
                            log::debug!("fixup dpi in newly added tab");
                            tab.resize(self.terminal_size);
                        }
                    }
                }
                MuxNotification::PaneOutput(pane_id) => {
                    self.mux_pane_output_event(pane_id);
                }
                MuxNotification::WindowInvalidated(_) => {
                    window.invalidate();
                    self.update_title_post_status();
                }
                MuxNotification::WindowRemoved(_window_id) => {
                    // Handled by frontend
                }
                MuxNotification::AssignClipboard { .. } => {
                    // Handled by frontend
                }
                MuxNotification::SaveToDownloads { .. } => {
                    // Handled by frontend
                }
                MuxNotification::PaneFocused(_) => {
                    // Also handled by clientpane
                    self.update_title_post_status();
                }
                MuxNotification::TabReflowed(_) => {
                    // Also handled by wezterm-client
                    self.update_title_post_status();
                }
                MuxNotification::TabTitleChanged { .. } => {
                    self.update_title_post_status();
                }
                MuxNotification::PaneAdded(_)
                | MuxNotification::WorkspaceRenamed { .. }
                | MuxNotification::PaneRemoved(_)
                | MuxNotification::WindowWorkspaceChanged(_)
                | MuxNotification::ActiveWorkspaceChanged(_)
                | MuxNotification::Empty
                | MuxNotification::WindowCreated(_) => {}
            },
            TermWindowNotif::EmitStatusUpdate => {
                self.emit_status_event();
            }
            TermWindowNotif::Apply(func) => {
                func(self);
            }
            TermWindowNotif::SwitchToMuxWindow(mux_window_id) => {
                self.mux_window_id = mux_window_id;
                *self.mux_window_id_for_subscriptions.lock().unwrap() = mux_window_id;

                self.clear_all_overlays();
                self.current_highlight.take();
                self.invalidate_fancy_tab_bar();
                self.invalidate_modal();

                let mux = Mux::get();
                if let Some(window) = mux.get_window(self.mux_window_id) {
                    for tab in window.iter() {
                        tab.resize(self.terminal_size);
                    }
                };
                self.update_title();
                window.invalidate();
            }
        }

        Ok(())
    }

    fn set_inner_size(&mut self, window: &Window, width: usize, height: usize) {
        self.resizes_pending += 1;
        window.set_inner_size(width, height);
    }

    /// Take care to remove our panes from the mux, otherwise
    /// we can leave the mux with no windows but some panes
    /// and it won't believe that we are empty.
    fn clear_all_overlays(&mut self) {
        let overlay_panes_to_cancel = self
            .pane_state
            .borrow()
            .iter()
            .filter_map(|(_, state)| state.overlay.as_ref().map(|overlay| overlay.pane.pane_id()))
            .collect::<Vec<_>>();

        for pane_id in overlay_panes_to_cancel {
            self.cancel_overlay_for_pane(pane_id);
        }

        let tab_overlays_to_cancel = self
            .tab_state
            .borrow()
            .iter()
            .filter_map(|(tab_id, state)| state.overlay.as_ref().map(|_| *tab_id))
            .collect::<Vec<_>>();

        for tab_id in tab_overlays_to_cancel {
            self.cancel_overlay_for_tab(tab_id, None);
        }

        self.pane_state.borrow_mut().clear();
        self.tab_state.borrow_mut().clear();
    }

    fn apply_icon(window: &Window) -> anyhow::Result<()> {
        let image = image::load_from_memory(ICON_DATA)?.into_rgba8();
        let (width, height) = image.dimensions();
        window.set_icon(Image::with_rgba32(
            width as usize,
            height as usize,
            width as usize * 4,
            image.as_raw(),
        ));
        Ok(())
    }

    fn schedule_status_update(&self) {
        if let Some(window) = self.window.as_ref() {
            window.notify(TermWindowNotif::EmitStatusUpdate);
        }
    }

    fn is_pane_visible(&mut self, pane_id: PaneId) -> bool {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return false,
        };

        let tab_id = tab.tab_id();
        if let Some(tab_overlay) = self
            .tab_state(tab_id)
            .overlay
            .as_ref()
            .map(|overlay| overlay.pane.clone())
        {
            return tab_overlay.pane_id() == pane_id;
        }

        tab.contains_pane(pane_id)
    }

    fn mux_pane_output_event(&mut self, pane_id: PaneId) {
        metrics::histogram!("mux.pane_output_event.rate").record(1.);
        if self.is_pane_visible(pane_id) {
            if let Some(ref win) = self.window {
                win.invalidate();
            }
        }
    }

    fn mux_pane_output_event_callback(
        n: MuxNotification,
        window: &Window,
        mux_window_id: MuxWindowId,
        dead: &Arc<AtomicBool>,
    ) -> bool {
        if dead.load(Ordering::Relaxed) {
            // Subscription cancelled asynchronously
            return false;
        }

        match n {
            MuxNotification::Alert {
                pane_id,
                alert:
                    Alert::OutputSinceFocusLost
                    | Alert::CurrentWorkingDirectoryChanged
                    | Alert::WindowTitleChanged(_)
                    | Alert::TabTitleChanged(_)
                    | Alert::IconTitleChanged(_)
                    | Alert::Progress(_)
                    | Alert::SetUserVar { .. }
                    | Alert::Bell,
            }
            | MuxNotification::PaneFocused(pane_id)
            | MuxNotification::PaneRemoved(pane_id)
            | MuxNotification::PaneOutput(pane_id) => {
                // Check window validity and propagate to the window event handler
                // that will do the full pane visibility check.
                let mux = Mux::get();
                if mux.get_window(mux_window_id).is_none() {
                    // If the window is not found, the mux_window_id may be stale during
                    // a workspace switch - skip this notif but keep the subscription.
                    // (next notifs should finish the workspace switch & reconcile the state)
                    return true;
                }
                let _ = pane_id;
            }
            MuxNotification::PaneAdded(_pane_id) => {
                // If some other client spawns a pane inside this window, this
                // gives us an opportunity to attach it to the clipboard.
                let mux = Mux::get();
                return mux.get_window(mux_window_id).is_some();
            }
            MuxNotification::TabAddedToWindow { window_id, .. }
            | MuxNotification::WindowTitleChanged { window_id, .. }
            | MuxNotification::WindowInvalidated(window_id) => {
                if window_id != mux_window_id {
                    return true;
                }
            }
            MuxNotification::WindowRemoved(window_id) => {
                if window_id != mux_window_id {
                    return true;
                }
                // The removed window matches our current mux_window_id.
                // During workspace switches, mux_window_id may be stale.
                // Skip this notification but keep the subscription alive.
                // (next notifs should finish the workspace switch & reconcile the state)
                return true;
            }
            MuxNotification::TabReflowed(tab_id)
            | MuxNotification::TabTitleChanged { tab_id, .. } => {
                let mux = Mux::get();
                if mux.window_containing_tab(tab_id) == Some(mux_window_id) {
                    // fall through
                } else {
                    return true;
                }
            }
            MuxNotification::Alert {
                alert: Alert::ToastNotification { .. },
                ..
            }
            | MuxNotification::AssignClipboard { .. }
            | MuxNotification::SaveToDownloads { .. }
            | MuxNotification::WindowCreated(_)
            | MuxNotification::ActiveWorkspaceChanged(_)
            | MuxNotification::WorkspaceRenamed { .. }
            | MuxNotification::Empty
            | MuxNotification::WindowWorkspaceChanged(_) => return true,
            MuxNotification::Alert {
                alert: Alert::PaletteChanged { .. },
                ..
            } => {
                // fall through
            }
        }

        window.notify(TermWindowNotif::MuxNotification(n));

        true
    }

    fn subscribe_to_pane_updates(&self) {
        let window = self.window.clone().expect("window to be valid on startup");
        let mux_window_id = Arc::clone(&self.mux_window_id_for_subscriptions);
        let mux = Mux::get();
        let dead = Arc::clone(&self.mux_subscription_dead);
        mux.subscribe(move |n| {
            if dead.load(Ordering::Relaxed) {
                // Unsubscribe this handler from the mux
                return false;
            }
            let mux_window_id = *mux_window_id.lock().unwrap();
            let window = window.clone();
            let dead = dead.clone();
            promise::spawn::spawn_into_main_thread(async move {
                Self::mux_pane_output_event_callback(n, &window, mux_window_id, &dead)
            })
            .detach();
            true
        });
    }

    fn emit_status_event(&mut self) {
        // update-right-status/update-status events were dispatched to rhai
        // handlers; with the scripting layer removed they have no consumers.
    }

    /// Named window events (`window-resized`, the generic `EmitEvent(name)`
    /// key assignment, etc.) used to be dispatched here to a rhai handler
    /// registered via `wezterm.on(name, ...)`. With the scripting layer
    /// removed there is no handler registry left to receive them, so this
    /// just immediately marks the event as finished (`again: false`, the
    /// same outcome `do_event` used to report when no rhai config was
    /// loaded) without spawning anything. The `EventState`
    /// queueing/de-duplication in [`Self::emit_window_event`] and
    /// [`Self::finish_window_event`] is kept as-is so a queued re-entrant
    /// call is still drained correctly.
    fn schedule_window_event(&mut self, name: &str, _pane_id: Option<PaneId>) {
        self.finish_window_event(name, false);
    }

    /// Called as part of finishing up a window event dispatch.
    /// If again==false it means that there isn't a handler
    /// to execute against, so we should just mark as done.
    /// Otherwise, if there is a queued item, schedule it now.
    fn finish_window_event(&mut self, name: &str, again: bool) {
        let state = self
            .event_states
            .entry(name.to_string())
            .or_insert(EventState::None);
        if again {
            match state {
                EventState::InProgress => {
                    *state = EventState::None;
                }
                EventState::InProgressWithQueued(pane) => {
                    let pane = *pane;
                    *state = EventState::InProgress;
                    self.schedule_window_event(name, pane);
                }
                EventState::None => {}
            }
        } else {
            *state = EventState::None;
        }
    }

    pub fn emit_window_event(&mut self, name: &str, pane_id: Option<PaneId>) {
        if self.get_active_pane_or_overlay().is_none() || self.window.is_none() {
            return;
        }

        let state = self
            .event_states
            .entry(name.to_string())
            .or_insert(EventState::None);
        match state {
            EventState::InProgress => {
                // Flag that we want to run again when the currently
                // executing event calls finish_window_event().
                *state = EventState::InProgressWithQueued(pane_id);
                return;
            }
            EventState::InProgressWithQueued(other_pane) => {
                // We've already got one copy executing and another
                // pending dispatch, so don't queue another.
                if pane_id != *other_pane {
                    log::warn!(
                        "Cannot queue {} event for pane {:?}, as \
                         there is already an event queued for pane {:?} \
                         in the same window",
                        name,
                        pane_id,
                        other_pane
                    );
                }
                return;
            }
            EventState::None => {
                // Nothing pending, so schedule a call now
                *state = EventState::InProgress;
                self.schedule_window_event(name, pane_id);
            }
        }
    }

    fn check_for_dirty_lines_and_invalidate_selection(&mut self, pane: &Arc<dyn Pane>) {
        let dims = pane.get_dimensions();
        let viewport = self
            .get_viewport(pane.pane_id())
            .unwrap_or(dims.physical_top);
        let visible_range = viewport..viewport + dims.viewport_rows as StableRowIndex;
        let seqno = self.selection(pane.pane_id()).seqno;
        let dirty = pane.get_changed_since(visible_range, seqno);

        if dirty.is_empty() {
            return;
        }
        if pane.downcast_ref::<CopyOverlay>().is_none()
            && pane.downcast_ref::<QuickSelectOverlay>().is_none()
        {
            // If any of the changed lines intersect with the
            // selection, then we need to clear the selection, but not
            // when the search overlay is active; the search overlay
            // marks lines as dirty to force invalidate them for
            // highlighting purpose but also manipulates the selection
            // and we want to allow it to retain the selection it made!

            let clear_selection =
                if let Some(selection_range) = self.selection(pane.pane_id()).range.as_ref() {
                    let selection_rows = selection_range.rows();
                    selection_rows.into_iter().any(|row| dirty.contains(row))
                } else {
                    false
                };

            if clear_selection {
                self.selection(pane.pane_id()).range.take();
                self.selection(pane.pane_id()).origin.take();
                self.selection(pane.pane_id()).seqno = pane.get_current_seqno();
            }
        }
    }
}

impl TermWindow {
    fn palette(&mut self) -> &ColorPalette {
        if self.palette.is_none() {
            self.palette
                .replace(config::TermConfig::new().color_palette());
        }
        self.palette.as_ref().unwrap()
    }

    pub fn config_was_reloaded(&mut self) {
        log::debug!(
            "config was reloaded, overrides: {:?}",
            self.config_overrides
        );
        self.key_table_state.clear_stack();
        self.connection_name = Connection::get().unwrap().name();
        let config = match config::overridden_config(&self.config_overrides) {
            Ok(config) => config,
            Err(err) => {
                log::error!(
                    "Failed to apply config overrides to window: {:#}: {:?}",
                    err,
                    self.config_overrides
                );
                configuration()
            }
        };
        self.config = config.clone();
        self.palette.take();

        let mux = Mux::get();
        let window = match mux.get_window(self.mux_window_id) {
            Some(window) => window,
            _ => return,
        };
        if window.len() == 1 {
            self.show_tab_bar = config.enable_tab_bar && !config.hide_tab_bar_if_only_one_tab;
        } else {
            self.show_tab_bar = config.enable_tab_bar;
        }
        *self.cursor_blink_state.borrow_mut() = ColorEase::new(
            config.cursor_blink_rate,
            config.cursor_blink_ease_in,
            config.cursor_blink_rate,
            config.cursor_blink_ease_out,
            None,
        );
        *self.blink_state.borrow_mut() = ColorEase::new(
            config.text_blink_rate,
            config.text_blink_ease_in,
            config.text_blink_rate,
            config.text_blink_ease_out,
            None,
        );
        *self.rapid_blink_state.borrow_mut() = ColorEase::new(
            config.text_blink_rate_rapid,
            config.text_blink_rapid_ease_in,
            config.text_blink_rate_rapid,
            config.text_blink_rapid_ease_out,
            None,
        );

        self.show_scroll_bar = config.enable_scroll_bar;
        self.shape_generation += 1;
        {
            let mut shape_cache = self.shape_cache.borrow_mut();
            shape_cache.update_config(&config);
            shape_cache.clear();
        }
        self.line_state_cache.borrow_mut().update_config(&config);
        self.line_quad_cache.borrow_mut().update_config(&config);
        self.line_to_ele_shape_cache
            .borrow_mut()
            .update_config(&config);
        self.fancy_tab_bar.take();
        self.invalidate_fancy_tab_bar();
        self.invalidate_modal();
        self.input_map = InputMap::new(&config);
        self.leader_is_down = None;
        self.render_state.as_mut().map(|rs| rs.config_changed());
        let dimensions = self.dimensions;

        if let Err(err) = self.fonts.config_changed(&config) {
            log::error!("Failed to load font configuration: {:#}", err);
        }

        if let Some(window) = mux.get_window(self.mux_window_id) {
            let term_config: Arc<dyn TerminalConfiguration> =
                Arc::new(TermConfig::with_config(config.clone()));
            for tab in window.iter() {
                for pane in tab.iter_panes_ignoring_zoom() {
                    pane.pane.set_config(Arc::clone(&term_config));
                }
            }
            for state in self.pane_state.borrow().values() {
                if let Some(overlay) = &state.overlay {
                    overlay.pane.set_config(Arc::clone(&term_config));
                }
            }
            for state in self.tab_state.borrow().values() {
                if let Some(overlay) = &state.overlay {
                    overlay.pane.set_config(Arc::clone(&term_config));
                }
            }
        }

        if let Some(window) = self.window.as_ref().map(|w| w.clone()) {
            self.load_os_parameters();
            self.apply_scale_change(&dimensions, self.fonts.get_font_scale());
            self.apply_dimensions(&dimensions, None, &window);
            window.config_did_change(&config);
            window.invalidate();
        }

        // Do this after we've potentially adjusted scaling based on config/padding
        // and window size
        self.window_background = reload_background_image(
            &config,
            &self.window_background,
            &self.dimensions,
            &self.render_metrics,
        );

        self.invalidate_modal();
    }

    fn invalidate_modal(&mut self) {
        if let Some(modal) = self.get_modal() {
            modal.reconfigure(self);
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    pub fn cancel_modal(&self) {
        self.modal.borrow_mut().take();
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub fn set_modal(&self, modal: Rc<dyn Modal>) {
        self.modal.borrow_mut().replace(modal);
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    fn get_modal(&self) -> Option<Rc<dyn Modal>> {
        self.modal.borrow().as_ref().map(|m| Rc::clone(&m))
    }

    fn update_scrollbar(&mut self) {
        if !self.show_scroll_bar {
            return;
        }

        let tab = match self.get_active_pane_or_overlay() {
            Some(tab) => tab,
            None => return,
        };

        let render_dims = tab.get_dimensions();
        if render_dims == self.last_scroll_info {
            return;
        }

        self.last_scroll_info = render_dims;

        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Called by various bits of code to update the title bar.
    /// Let's also trigger the status event so that it can choose
    /// to update the right-status.
    fn update_title(&mut self) {
        self.schedule_status_update();
        self.update_title_impl();
    }

    fn window_contains_pane(&mut self, pane_id: PaneId) -> bool {
        let mux = Mux::get();

        let (_domain, window_id, _tab_id) = match mux.resolve_pane_id(pane_id) {
            Some(tuple) => tuple,
            None => return false,
        };

        return window_id == self.mux_window_id;
    }

    fn emit_user_var_event(&mut self, pane_id: PaneId, _name: String, _value: String) {
        if !self.window_contains_pane(pane_id) {
            return;
        }
        self.update_title();
    }

    /// Called by window:set_right_status after the status has
    /// been updated; let's update the bar
    pub fn update_title_post_status(&mut self) {
        self.update_title_impl();
    }

    fn update_title_impl(&mut self) {
        let mux = Mux::get();
        let window = match mux.get_window(self.mux_window_id) {
            Some(window) => window,
            _ => return,
        };
        let tabs = self.get_tab_information();
        let panes = self.get_pane_information();
        let active_tab = tabs.iter().find(|t| t.is_active).cloned();
        let active_pane = panes.iter().find(|p| p.is_active).cloned();

        let border = self.get_os_border();
        let tab_bar_height = self.tab_bar_pixel_height().unwrap_or(0.);
        let tab_bar_y = if self.config.tab_bar_at_bottom {
            ((self.dimensions.pixel_height as f32) - (tab_bar_height + border.bottom.get() as f32))
                .max(0.)
        } else {
            border.top.get() as f32
        };

        let tab_bar_height = self.tab_bar_pixel_height().unwrap_or(0.);

        let hovering_in_tab_bar = match &self.current_mouse_event {
            Some(event) => {
                let mouse_y = event.coords.y as f32;
                mouse_y >= tab_bar_y as f32 && mouse_y < tab_bar_y as f32 + tab_bar_height
            }
            None => false,
        };

        let new_tab_bar = TabBarState::new(
            self.dimensions.pixel_width / self.render_metrics.cell_size.width as usize,
            if hovering_in_tab_bar {
                Some(self.last_mouse_coords.0)
            } else {
                None
            },
            &tabs,
            &panes,
            self.config.resolved_palette.tab_bar.as_ref(),
            &self.config,
            &self.left_status,
            &self.right_status,
            self.render_metrics.cell_size.width as f32,
            self.os_parameters.as_ref(),
        );
        if new_tab_bar != self.tab_bar {
            self.tab_bar = new_tab_bar;
            self.invalidate_fancy_tab_bar();
            self.invalidate_modal();
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }

        let num_tabs = window.len();
        if num_tabs == 0 {
            return;
        }
        drop(window);

        let title = if let (Some(pos), Some(tab)) = (active_pane, active_tab) {
            // Mirrors compute_tab_title's fallback (crate::tabbar):
            // prefer the cwd basename over the pane's own title (the
            // running program's name) when configured to do so, so
            // the window title tracks `cd` the same way the tab
            // title does.
            let pane_title = if self.config.use_cwd_basename_as_tab_title {
                match &pos.current_working_dir {
                    Some(cwd) if !cwd.is_empty() => {
                        crate::tabbar::basename_of_path(cwd)
                    }
                    _ => pos.title.clone(),
                }
            } else {
                pos.title.clone()
            };
            if num_tabs == 1 {
                format!(
                    "{}{}",
                    if pos.is_zoomed { "[Z] " } else { "" },
                    pane_title
                )
            } else {
                format!(
                    "{}[{}/{}] {}",
                    if pos.is_zoomed { "[Z] " } else { "" },
                    tab.tab_index + 1,
                    num_tabs,
                    pane_title
                )
            }
        } else {
            "".to_string()
        };

        if let Some(window) = self.window.as_ref() {
            window.set_title(&title);

            let show_tab_bar = if num_tabs == 1 {
                self.config.enable_tab_bar && !self.config.hide_tab_bar_if_only_one_tab
            } else {
                self.config.enable_tab_bar
            };

            // If the number of tabs changed and caused the tab bar to
            // hide/show, then we'll need to resize things.  It is simplest
            // to piggy back on the config reloading code for that, so that
            // is what we're doing.
            if show_tab_bar != self.show_tab_bar {
                self.config_was_reloaded();
            }
        }
        self.schedule_next_status_update();
    }

    fn schedule_next_status_update(&mut self) {
        if let Some(window) = self.window.as_ref() {
            let now = Instant::now();
            if self.last_status_call <= now {
                let interval = Duration::from_millis(self.config.status_update_interval);
                let target = now + interval;
                self.last_status_call = target;

                let window = window.clone();
                promise::spawn::spawn(async move {
                    Timer::at(target).await;
                    window.notify(TermWindowNotif::EmitStatusUpdate);
                })
                .detach();
            }
        }
    }

    fn update_text_cursor(&mut self, pos: &PositionedPane) {
        if let Some(win) = self.window.as_ref() {
            let cursor = pos.pane.get_cursor_position();
            let top = pos.pane.get_dimensions().physical_top;
            let tab_bar_height = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
                self.tab_bar_pixel_height().unwrap()
            } else {
                0.0
            };
            let (padding_left, padding_top) = self.padding_left_top();

            let r = Rect::new(
                Point::new(
                    (((cursor.x + pos.left) as isize).max(0) * self.render_metrics.cell_size.width)
                        .add(padding_left as isize),
                    ((cursor.y + pos.top as isize - top).max(0)
                        * self.render_metrics.cell_size.height)
                        .add(tab_bar_height as isize)
                        .add(padding_top as isize),
                ),
                self.render_metrics.cell_size,
            );
            win.set_text_cursor_position(r);
        }
    }

    fn activate_window(&mut self, window_idx: usize) -> anyhow::Result<()> {
        let windows = front_end().gui_windows();
        if let Some(win) = windows.get(window_idx) {
            win.window.focus();
        }
        Ok(())
    }

    fn activate_window_relative(&mut self, delta: isize, wrap: bool) -> anyhow::Result<()> {
        let windows = front_end().gui_windows();
        let my_idx = windows
            .iter()
            .position(|w| Some(&w.window) == self.window.as_ref())
            .ok_or_else(|| anyhow!("I'm not in the window list!?"))?;

        let idx = my_idx as isize + delta;

        let idx = if wrap {
            let idx = if idx < 0 {
                windows.len() as isize + idx
            } else {
                idx
            };
            idx as usize % windows.len()
        } else {
            if idx < 0 {
                0
            } else if idx >= windows.len() as isize {
                windows.len().saturating_sub(1)
            } else {
                idx as usize
            }
        };

        if let Some(win) = windows.get(idx) {
            win.window.focus();
        }

        Ok(())
    }

    fn activate_tab(&mut self, tab_idx: isize) -> anyhow::Result<()> {
        let mux = Mux::get();
        let mut window = mux
            .get_window_mut(self.mux_window_id)
            .ok_or_else(|| anyhow!("no such window"))?;

        // This logic is coupled with the CliSubCommand::ActivateTab
        // logic in wezterm/src/main.rs. If you update this, update that!
        let max = window.len();

        let tab_idx = if tab_idx < 0 {
            max.saturating_sub(tab_idx.abs() as usize)
        } else {
            tab_idx as usize
        };

        if tab_idx < max {
            window.save_and_then_set_active(tab_idx);

            drop(window);

            if let Some(pane) = self.get_active_pane_or_overlay() {
                pane.focus_changed(true);
            }

            self.update_title();
            self.update_scrollbar();
        }
        Ok(())
    }

    fn activate_tab_relative(&mut self, delta: isize, wrap: bool) -> anyhow::Result<()> {
        let mux = Mux::get();
        let window = mux
            .get_window(self.mux_window_id)
            .ok_or_else(|| anyhow!("no such window"))?;

        let max = window.len();
        ensure!(max > 0, "no more tabs");

        // This logic is coupled with the CliSubCommand::ActivateTab
        // logic in wezterm/src/main.rs. If you update this, update that!
        let active = window.get_active_idx() as isize;
        let tab = active + delta;
        let tab = if wrap {
            let tab = if tab < 0 { max as isize + tab } else { tab };
            (tab as usize % max) as isize
        } else {
            if tab < 0 {
                0
            } else if tab >= max as isize {
                max as isize - 1
            } else {
                tab
            }
        };
        drop(window);
        self.activate_tab(tab)
    }

    fn activate_last_tab(&mut self) -> anyhow::Result<()> {
        let mux = Mux::get();
        let window = mux
            .get_window(self.mux_window_id)
            .ok_or_else(|| anyhow!("no such window"))?;

        let last_idx = window.get_last_active_idx();
        drop(window);
        match last_idx {
            Some(idx) => self.activate_tab(idx as isize),
            None => Ok(()),
        }
    }

    fn move_tab(&mut self, tab_idx: usize) -> anyhow::Result<()> {
        let mux = Mux::get();
        let mut window = mux
            .get_window_mut(self.mux_window_id)
            .ok_or_else(|| anyhow!("no such window"))?;

        let max = window.len();
        ensure!(max > 0, "no more tabs");

        let active = window.get_active_idx();

        ensure!(tab_idx < max, "cannot move a tab out of range");

        let tab_inst = window.remove_by_idx(active);
        window.insert(tab_idx, &tab_inst);
        window.set_active_without_saving(tab_idx);

        drop(window);
        self.update_title();
        self.update_scrollbar();

        Ok(())
    }

    fn show_input_selector(&mut self, args: &config::keyassignment::InputSelector) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        // Ignore any current overlay: we're going to cancel it out below
        // and we don't want this new one to reference that cancelled pane
        let pane = match self.get_active_pane_no_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let args = args.clone();

        let gui_win = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        let (overlay, future) = match start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::selector::selector(term, args, gui_win, pane)
        }) {
            Ok(res) => res,
            Err(err) => {
                log::error!("Failed to show selector overlay: {err:#}");
                return;
            }
        };
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    fn show_prompt_input_line(&mut self, args: &PromptInputLine) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let args = args.clone();

        let gui_win = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        let (overlay, future) = match start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::prompt::show_line_prompt_overlay(term, args, gui_win, pane)
        }) {
            Ok(res) => res,
            Err(err) => {
                log::error!("Failed to show prompt input line overlay: {err:#}");
                return;
            }
        };
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    fn show_confirmation(&mut self, args: &Confirmation) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let args = args.clone();

        let gui_win = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        let (overlay, future) = match start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::confirm::show_confirmation_overlay(term, args, gui_win, pane)
        }) {
            Ok(res) => res,
            Err(err) => {
                log::error!("Failed to show confirmation overlay: {err:#}");
                return;
            }
        };
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    fn show_debug_overlay(&mut self) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        let gui_win = GuiWin::new(self);

        let opengl_info = self.opengl_info.as_deref().unwrap_or("Unknown").to_string();
        let connection_info = self.connection_name.clone();

        let (overlay, future) = match start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::show_debug_overlay(term, gui_win, opengl_info, connection_info)
        }) {
            Ok(res) => res,
            Err(err) => {
                log::error!("Failed to show debug overlay: {err:#}");
                return;
            }
        };
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    fn show_tab_navigator(&mut self) {
        let mux = Mux::get();
        let active_tab_idx = match mux.get_window(self.mux_window_id) {
            Some(mux_window) => mux_window.get_active_idx(),
            None => return,
        };
        let title = "Tab Navigator".to_string();
        let args = LauncherActionArgs {
            title: Some(title),
            flags: LauncherFlags::TABS,
            help_text: None,
            fuzzy_help_text: None,
            alphabet: None,
        };
        self.show_launcher_impl(args, active_tab_idx);
    }

    fn show_launcher(&mut self) {
        let title = "Launcher".to_string();
        let args = LauncherActionArgs {
            title: Some(title),
            flags: LauncherFlags::LAUNCH_MENU_ITEMS
                | LauncherFlags::WORKSPACES
                | LauncherFlags::DOMAINS
                | LauncherFlags::KEY_ASSIGNMENTS
                | LauncherFlags::COMMANDS,
            help_text: None,
            fuzzy_help_text: None,
            alphabet: None,
        };
        self.show_launcher_impl(args, 0);
    }

    fn show_launcher_impl(&mut self, args: LauncherActionArgs, initial_choice_idx: usize) {
        let mux_window_id = self.mux_window_id;
        let window = self.window.as_ref().unwrap().clone();

        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let domain_id_of_current_pane = tab
            .get_active_pane()
            .expect("tab has no panes!")
            .domain_id();
        let pane_id = pane.pane_id();
        let tab_id = tab.tab_id();
        let title = args.title.unwrap();
        let flags = args.flags;
        let help_text = args.help_text.unwrap_or(
            "Select an item and press Enter=launch  \
             Esc=cancel  /=filter"
                .to_string(),
        );
        let fuzzy_help_text = args
            .fuzzy_help_text
            .unwrap_or("Fuzzy matching: ".to_string());

        let config = &self.config;
        let alphabet = args.alphabet.unwrap_or(config.launcher_alphabet.clone());

        promise::spawn::spawn(async move {
            let args = LauncherArgs::new(
                &title,
                flags,
                mux_window_id,
                pane_id,
                domain_id_of_current_pane,
                &help_text,
                &fuzzy_help_text,
                &alphabet,
            )
            .await;

            let win = window.clone();
            win.notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                let mux = Mux::get();
                if let Some(tab) = mux.get_tab(tab_id) {
                    let window = window.clone();
                    let (overlay, future) =
                        match start_overlay(term_window, &tab, move |_tab_id, term| {
                            launcher(args, term, window, initial_choice_idx)
                        }) {
                            Ok(res) => res,
                            Err(err) => {
                                log::error!("Failed to show launcher overlay: {err:#}");
                                return;
                            }
                        };

                    term_window.assign_overlay(tab_id, overlay);
                    promise::spawn::spawn(future).detach();
                }
            })));
        })
        .detach();
    }

    /// Returns the Prompt semantic zones
    fn get_semantic_prompt_zones(&mut self, pane: &Arc<dyn Pane>) -> &[StableRowIndex] {
        let cache = self
            .semantic_zones
            .entry(pane.pane_id())
            .or_insert_with(SemanticZoneCache::default);

        let seqno = pane.get_current_seqno();
        if cache.seqno != seqno {
            let zones = pane.get_semantic_zones().unwrap_or_else(|_| vec![]);
            let mut zones: Vec<StableRowIndex> = zones
                .into_iter()
                .filter_map(|zone| {
                    if zone.semantic_type == wezterm_term::SemanticType::Prompt {
                        Some(zone.start_y)
                    } else {
                        None
                    }
                })
                .collect();
            // dedup to avoid issues where both left and right prompts are
            // defined: we only care if there were 1+ prompts on a line,
            // not about how many prompts are on a line.
            // <https://github.com/wezterm/wezterm/issues/1121>
            zones.dedup();
            cache.zones = zones;
            cache.seqno = seqno;
        }
        &cache.zones
    }

    fn scroll_to_prompt(&mut self, amount: isize, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        let dims = pane.get_dimensions();
        let position = self
            .get_viewport(pane.pane_id())
            .unwrap_or(dims.physical_top);
        let zone = {
            let zones = self.get_semantic_prompt_zones(&pane);
            let idx = match zones.binary_search(&position) {
                Ok(idx) | Err(idx) => idx,
            };
            let idx = ((idx as isize) + amount).max(0) as usize;
            zones.get(idx).cloned()
        };
        if let Some(zone) = zone {
            self.set_viewport(pane.pane_id(), Some(zone), dims);
        }

        if let Some(win) = self.window.as_ref() {
            win.invalidate();
        }
        Ok(())
    }

    fn scroll_by_page(&mut self, amount: f64, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        let dims = pane.get_dimensions();
        let position = self
            .get_viewport(pane.pane_id())
            .unwrap_or(dims.physical_top) as f64
            + (amount * dims.viewport_rows as f64);
        self.set_viewport(pane.pane_id(), Some(position as isize), dims);
        if let Some(win) = self.window.as_ref() {
            win.invalidate();
        }
        Ok(())
    }

    fn scroll_by_current_event_wheel_delta(&mut self, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        if let Some(event) = &self.current_mouse_event {
            let amount = match event.kind {
                MouseEventKind::VertWheel(amount) => -amount,
                _ => return Ok(()),
            };
            self.scroll_by_line(amount.into(), pane)?;
        }
        Ok(())
    }

    fn scroll_by_line(&mut self, amount: isize, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        let dims = pane.get_dimensions();
        let position = self
            .get_viewport(pane.pane_id())
            .unwrap_or(dims.physical_top)
            .saturating_add(amount);
        self.set_viewport(pane.pane_id(), Some(position), dims);
        if let Some(win) = self.window.as_ref() {
            win.invalidate();
        }
        Ok(())
    }

    fn move_tab_relative(&mut self, delta: isize) -> anyhow::Result<()> {
        let mux = Mux::get();
        let window = mux
            .get_window(self.mux_window_id)
            .ok_or_else(|| anyhow!("no such window"))?;

        let max = window.len();
        ensure!(max > 0, "no more tabs");

        let active = window.get_active_idx();
        let tab = active as isize + delta;
        let tab = if tab < 0 {
            0usize
        } else if tab >= max as isize {
            max - 1
        } else {
            tab as usize
        };

        drop(window);
        self.move_tab(tab)
    }

    pub fn perform_key_assignment(
        &mut self,
        pane: &Arc<dyn Pane>,
        assignment: &KeyAssignment,
    ) -> anyhow::Result<PerformAssignmentResult> {
        use KeyAssignment::*;

        if let Some(modal) = self.get_modal() {
            if modal.perform_assignment(assignment, self) {
                return Ok(PerformAssignmentResult::Handled);
            }
        }

        match pane.perform_assignment(assignment) {
            PerformAssignmentResult::Unhandled => {}
            result => return Ok(result),
        }

        let window = self.window.as_ref().map(|w| w.clone());

        match assignment {
            ActivateKeyTable {
                name,
                timeout_milliseconds,
                replace_current,
                one_shot,
                until_unknown,
                prevent_fallback,
            } => {
                anyhow::ensure!(
                    self.input_map.has_table(name),
                    "ActivateKeyTable: no key_table named {}",
                    name
                );
                self.key_table_state.activate(KeyTableArgs {
                    name,
                    timeout_milliseconds: *timeout_milliseconds,
                    replace_current: *replace_current,
                    one_shot: *one_shot,
                    until_unknown: *until_unknown,
                    prevent_fallback: *prevent_fallback,
                });
                self.update_title();
            }
            PopKeyTable => {
                self.key_table_state.pop();
                self.update_title();
            }
            ClearKeyTableStack => {
                self.key_table_state.clear_stack();
                self.update_title();
            }
            Multiple(actions) => {
                for a in actions {
                    self.perform_key_assignment(pane, a)?;
                }
            }
            SpawnTab(spawn_where) => {
                self.spawn_tab(spawn_where);
            }
            SpawnWindow => {
                self.spawn_command(&SpawnCommand::default(), SpawnWhere::NewWindow);
            }
            SpawnCommandInNewTab(spawn) => {
                self.spawn_command(spawn, SpawnWhere::NewTab);
            }
            SpawnCommandInNewWindow(spawn) => {
                self.spawn_command(spawn, SpawnWhere::NewWindow);
            }
            SplitHorizontal(spawn) => {
                log::trace!("SplitHorizontal {:?}", spawn);
                self.spawn_command(
                    spawn,
                    SpawnWhere::SplitPane(SplitRequest {
                        direction: SplitDirection::Horizontal,
                        target_is_second: true,
                        size: MuxSplitSize::Percent(50),
                        top_level: false,
                    }),
                );
            }
            SplitVertical(spawn) => {
                log::trace!("SplitVertical {:?}", spawn);
                self.spawn_command(
                    spawn,
                    SpawnWhere::SplitPane(SplitRequest {
                        direction: SplitDirection::Vertical,
                        target_is_second: true,
                        size: MuxSplitSize::Percent(50),
                        top_level: false,
                    }),
                );
            }
            ToggleFullScreen => {
                self.window.as_ref().unwrap().toggle_fullscreen();
            }
            ToggleAlwaysOnTop => {
                let window = self.window.clone().unwrap();
                let current_level = self.window_state.as_window_level();

                match current_level {
                    WindowLevel::AlwaysOnTop => {
                        window.set_window_level(WindowLevel::Normal);
                    }
                    WindowLevel::AlwaysOnBottom | WindowLevel::Normal => {
                        window.set_window_level(WindowLevel::AlwaysOnTop);
                    }
                }
            }
            ToggleAlwaysOnBottom => {
                let window = self.window.clone().unwrap();
                let current_level = self.window_state.as_window_level();

                match current_level {
                    WindowLevel::AlwaysOnBottom => {
                        window.set_window_level(WindowLevel::Normal);
                    }
                    WindowLevel::AlwaysOnTop | WindowLevel::Normal => {
                        window.set_window_level(WindowLevel::AlwaysOnBottom);
                    }
                }
            }
            SetWindowLevel(level) => {
                let window = self.window.clone().unwrap();
                window.set_window_level(level.clone());
            }
            CopyTo(dest) => {
                let text = self.selection_text(pane);
                self.copy_to_clipboard(*dest, text);
            }
            CopySelectionOrInterrupt => {
                let text = self.selection_text(pane);
                if !text.is_empty() {
                    self.copy_to_clipboard(ClipboardCopyDestination::Clipboard, text);
                    self.clear_selection(pane);
                } else {
                    // Route through whatever keyboard protocol the app in
                    // the pane has negotiated (win32-input-mode or kitty),
                    // rather than always writing the legacy `\x03` byte: an
                    // app that asked for eg. win32-input-mode (confirmed via
                    // live escape-sequence capture to be what Codex CLI
                    // negotiates) expects Ctrl+C in that app's requested
                    // form and may not treat a bare `\x03` as an interrupt
                    // while that mode is active.
                    let event = KeyEvent {
                        key: KeyCode::Char('c'),
                        modifiers: Modifiers::CTRL,
                        leds: KeyboardLedStatus::empty(),
                        repeat_count: 1,
                        key_is_down: true,
                        raw: Some(RawKeyEvent {
                            key: KeyCode::Char('c'),
                            modifiers: Modifiers::CTRL,
                            leds: KeyboardLedStatus::empty(),
                            phys_code: Some(PhysKeyCode::C),
                            raw_code: 0x43, // VK_C
                            #[cfg(windows)]
                            scan_code: 0x2e,
                            repeat_count: 1,
                            key_is_down: true,
                            handled: Handled::new(),
                        }),
                        #[cfg(windows)]
                        win32_uni_char: None,
                    };
                    match self.encode_via_negotiated_protocol(pane, &event) {
                        Some(encoded) => {
                            pane.writer().write_all(encoded.as_bytes()).ok();
                        }
                        None => {
                            pane.writer().write_all(b"\x03").ok();
                        }
                    }
                }
            }
            SendEnterOrNewline(mods) => {
                // See `CopySelectionOrInterrupt` above: route through
                // whatever keyboard protocol the app has negotiated so
                // eg. Codex CLI (which negotiates win32-input-mode via
                // DECSET, confirmed via live escape-sequence capture --
                // it never attempts kitty keyboard protocol) gets the
                // modified-Enter form it expects, instead of a hardcoded
                // '\n' unconditionally masking the chord from ever
                // reaching that negotiation. Only apps that haven't
                // negotiated such a protocol get the '\n' fallback, since
                // that's the best a plain/legacy app can do with this
                // chord.
                let event = KeyEvent {
                    key: KeyCode::Char('\r'),
                    modifiers: *mods,
                    leds: KeyboardLedStatus::empty(),
                    repeat_count: 1,
                    key_is_down: true,
                    raw: Some(RawKeyEvent {
                        key: KeyCode::Char('\r'),
                        modifiers: *mods,
                        leds: KeyboardLedStatus::empty(),
                        phys_code: Some(PhysKeyCode::Return),
                        raw_code: 0x0d, // VK_RETURN
                        #[cfg(windows)]
                        scan_code: 0x1c,
                        repeat_count: 1,
                        key_is_down: true,
                        handled: Handled::new(),
                    }),
                    #[cfg(windows)]
                    win32_uni_char: None,
                };
                match self.encode_via_negotiated_protocol(pane, &event) {
                    Some(encoded) => {
                        pane.writer().write_all(encoded.as_bytes()).ok();
                    }
                    None => {
                        pane.writer().write_all(b"\n").ok();
                    }
                }
            }
            CopyTextTo { text, destination } => {
                self.copy_to_clipboard(*destination, text.clone());
            }
            PasteFrom(source) => {
                self.paste_from_clipboard(pane, *source);
            }
            ActivateTabRelative(n) => {
                self.activate_tab_relative(*n, true)?;
            }
            ActivateTabRelativeNoWrap(n) => {
                self.activate_tab_relative(*n, false)?;
            }
            ActivateLastTab => self.activate_last_tab()?,
            DecreaseFontSize => self.decrease_font_size(),
            IncreaseFontSize => self.increase_font_size(),
            ResetFontSize => self.reset_font_size(),
            ResetFontAndWindowSize => {
                if let Some(w) = window.as_ref() {
                    self.reset_font_and_window_size(&w)?
                }
            }
            ActivateTab(n) => {
                self.activate_tab(*n)?;
            }
            ActivateWindow(n) => {
                self.activate_window(*n)?;
            }
            ActivateWindowRelative(n) => {
                self.activate_window_relative(*n, true)?;
            }
            ActivateWindowRelativeNoWrap(n) => {
                self.activate_window_relative(*n, false)?;
            }
            SendString(s) => pane.writer().write_all(s.as_bytes())?,
            SendKey(key) => {
                use keyevent::Key;
                let mods = key.mods;
                if let Key::Code(key) = self.win_key_code_to_termwiz_key_code(
                    &key.key.resolve(self.config.key_map_preference),
                ) {
                    pane.key_down(key, mods)?;
                }
            }
            Hide => {
                if let Some(w) = window.as_ref() {
                    w.hide();
                }
            }
            Show => {
                if let Some(w) = window.as_ref() {
                    w.show();
                }
            }
            CloseCurrentTab { confirm } => self.close_current_tab(*confirm),
            CloseCurrentPane { confirm } => self.close_current_pane(*confirm),
            Nop | DisableDefaultAssignment => {}
            ReloadConfiguration => config::reload(),
            MoveTab(n) => self.move_tab(*n)?,
            MoveTabRelative(n) => self.move_tab_relative(*n)?,
            ScrollByPage(n) => self.scroll_by_page(**n, pane)?,
            ScrollByLine(n) => self.scroll_by_line(*n, pane)?,
            ScrollByCurrentEventWheelDelta => self.scroll_by_current_event_wheel_delta(pane)?,
            ScrollToPrompt(n) => self.scroll_to_prompt(*n, pane)?,
            ScrollToTop => self.scroll_to_top(pane),
            ScrollToBottom => self.scroll_to_bottom(pane),
            ShowTabNavigator => self.show_tab_navigator(),
            ShowDebugOverlay => self.show_debug_overlay(),
            ShowLauncher => self.show_launcher(),
            ShowLauncherArgs(args) => {
                let title = args.title.clone().unwrap_or("Launcher".to_string());
                let args = LauncherActionArgs {
                    title: Some(title),
                    flags: args.flags,
                    help_text: args.help_text.clone(),
                    fuzzy_help_text: args.fuzzy_help_text.clone(),
                    alphabet: args.alphabet.clone(),
                };
                self.show_launcher_impl(args, 0);
            }
            HideApplication => {
                let con = Connection::get().expect("call on gui thread");
                con.hide_application();
            }
            // OnlyTerm: never prompt on quit - close-confirmation overlays
            // are removed entirely, not just defaulted off via config.
            QuitApplication => {
                log::info!("QuitApplication over here (window)");
                let con = Connection::get().expect("call on gui thread");
                con.terminate_message_loop();
            }
            SelectTextAtMouseCursor(mode) => self.select_text_at_mouse_cursor(*mode, pane),
            ExtendSelectionToMouseCursor(mode) => {
                self.extend_selection_at_mouse_cursor(*mode, pane)
            }
            ClearSelection => {
                self.clear_selection(pane);
            }
            StartWindowDrag => {
                self.window_drag_position = self.current_mouse_event.clone();
            }
            OpenLinkAtMouseCursor => {
                self.do_open_link_at_mouse_cursor(pane);
            }
            CopyLinkAtMouseCursor(destination) => {
                // Right-click's default binding. If there's a hyperlink
                // under the cursor, copy its URL (existing behavior).
                // Otherwise, if there's a text selection, copy it to the
                // clipboard and clear the selection - matching the same
                // copy-then-clear pattern as CTRL+C's
                // CopySelectionOrInterrupt. Unlike left-click's
                // CompleteSelectionOrOpenLinkAtMouseCursor, this is an
                // explicit action to end the selection, so clearing here
                // is correct.
                if self.current_highlight.is_some() {
                    self.do_copy_link_at_mouse_cursor(*destination);
                } else {
                    let text = self.selection_text(pane);
                    if !text.is_empty() {
                        self.copy_to_clipboard(*destination, text);
                        self.clear_selection(pane);
                        if let Some(window) = self.window.as_ref() {
                            window.invalidate();
                        }
                    }
                }
            }
            EmitEvent(name) => {
                self.emit_window_event(name, None);
            }
            CompleteSelectionOrOpenLinkAtMouseCursor(dest) => {
                // Releasing the mouse button after a drag-select must leave
                // the selection visible: it should only go away when the
                // user clicks elsewhere (handled by `begin()` resetting the
                // range on the next mouse-down), right-clicks it (copies and
                // clears, see CopyLinkAtMouseCursor above), or presses
                // Ctrl+C (CopySelectionOrInterrupt). So, unlike those two,
                // this handler must not clear the selection itself.
                let text = self.selection_text(pane);
                if !text.is_empty() {
                    self.copy_to_clipboard(*dest, text);
                    let window = self.window.as_ref().unwrap();
                    window.invalidate();
                } else {
                    self.do_open_link_at_mouse_cursor(pane);
                }
            }
            CompleteSelection(dest) => {
                let text = self.selection_text(pane);
                if !text.is_empty() {
                    self.copy_to_clipboard(*dest, text);
                    let window = self.window.as_ref().unwrap();
                    window.invalidate();
                }
            }
            ClearScrollback(erase_mode) => {
                pane.erase_scrollback(*erase_mode);
                let window = self.window.as_ref().unwrap();
                window.invalidate();
            }
            Search(pattern) => {
                if let Some(pane) = self.get_active_pane_or_overlay() {
                    let mut replace_current = false;
                    if let Some(existing) = pane.downcast_ref::<CopyOverlay>() {
                        let mut params = existing.get_params();
                        params.editing_search = true;
                        if !pattern.is_empty() {
                            params.pattern = self.resolve_search_pattern(pattern.clone(), &pane);
                        }
                        existing.apply_params(params);
                        replace_current = true;
                    } else {
                        let search = CopyOverlay::with_pane(
                            self,
                            &pane,
                            CopyModeParams {
                                pattern: self.resolve_search_pattern(pattern.clone(), &pane),
                                editing_search: true,
                            },
                        )?;
                        self.assign_overlay_for_pane(pane.pane_id(), search);
                    }
                    self.pane_state(pane.pane_id())
                        .overlay
                        .as_mut()
                        .map(|overlay| {
                            overlay.key_table_state.activate(KeyTableArgs {
                                name: "search_mode",
                                timeout_milliseconds: None,
                                replace_current,
                                one_shot: false,
                                until_unknown: false,
                                prevent_fallback: false,
                            });
                        });
                }
            }
            QuickSelect => {
                if let Some(pane) = self.get_active_pane_no_overlay() {
                    let qa = QuickSelectOverlay::with_pane(
                        self,
                        &pane,
                        &QuickSelectArguments::default(),
                    );
                    self.assign_overlay_for_pane(pane.pane_id(), qa);
                }
            }
            QuickSelectArgs(args) => {
                if let Some(pane) = self.get_active_pane_no_overlay() {
                    let qa = QuickSelectOverlay::with_pane(self, &pane, args);
                    self.assign_overlay_for_pane(pane.pane_id(), qa);
                }
            }
            ActivateCopyMode => {
                if let Some(pane) = self.get_active_pane_or_overlay() {
                    let mut replace_current = false;
                    if let Some(existing) = pane.downcast_ref::<CopyOverlay>() {
                        let mut params = existing.get_params();
                        params.editing_search = false;
                        existing.apply_params(params);
                        replace_current = true;
                    } else {
                        let copy = CopyOverlay::with_pane(
                            self,
                            &pane,
                            CopyModeParams {
                                pattern: MuxPattern::default(),
                                editing_search: false,
                            },
                        )?;
                        self.assign_overlay_for_pane(pane.pane_id(), copy);
                    }
                    self.pane_state(pane.pane_id())
                        .overlay
                        .as_mut()
                        .map(|overlay| {
                            overlay.key_table_state.activate(KeyTableArgs {
                                name: "copy_mode",
                                timeout_milliseconds: None,
                                replace_current,
                                one_shot: false,
                                until_unknown: false,
                                prevent_fallback: false,
                            });
                        });
                }
            }
            AdjustPaneSize(direction, amount) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };

                let tab_id = tab.tab_id();

                if self.tab_state(tab_id).overlay.is_none() {
                    tab.adjust_pane_size(*direction, *amount);
                }
            }
            ActivatePaneByIndex(index) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };

                let tab_id = tab.tab_id();

                if self.tab_state(tab_id).overlay.is_none() {
                    let panes = tab.iter_panes();
                    if panes.iter().position(|p| p.index == *index).is_some() {
                        tab.set_active_idx(*index);
                    }
                }
            }
            ActivatePaneDirection(direction) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };

                let tab_id = tab.tab_id();

                if self.tab_state(tab_id).overlay.is_none() {
                    tab.activate_pane_direction(*direction);
                }
            }
            TogglePaneZoomState => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };
                tab.toggle_zoom();
            }
            SetPaneZoomState(zoomed) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };
                tab.set_zoomed(*zoomed);
            }
            SwitchWorkspaceRelative(delta) => {
                let mux = Mux::get();
                let workspace = mux.active_workspace();
                let workspaces = mux.iter_workspaces();
                let idx = workspaces.iter().position(|w| *w == workspace).unwrap_or(0);
                let new_idx = idx as isize + delta;
                let new_idx = if new_idx < 0 {
                    workspaces.len() as isize + new_idx
                } else {
                    new_idx
                };
                let new_idx = new_idx as usize % workspaces.len();
                if let Some(w) = workspaces.get(new_idx) {
                    front_end().switch_workspace(w);
                }
            }
            SwitchToWorkspace { name, spawn } => {
                let activity = crate::Activity::new();
                let mux = Mux::get();
                let name = name
                    .as_ref()
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| mux.generate_workspace_name());
                let switcher = crate::frontend::WorkspaceSwitcher::new(&name);
                mux.set_active_workspace(&name);

                if mux.iter_windows_in_workspace(&name).is_empty() {
                    let spawn = spawn.as_ref().map(|s| s.clone()).unwrap_or_default();
                    let size = self.terminal_size;
                    let term_config = Arc::new(TermConfig::with_config(self.config.clone()));
                    let src_window_id = self.mux_window_id;

                    promise::spawn::spawn(async move {
                        if let Err(err) = crate::spawn::spawn_command_internal(
                            spawn,
                            SpawnWhere::NewWindow,
                            size,
                            Some(src_window_id),
                            term_config,
                        )
                        .await
                        {
                            log::error!("Failed to spawn: {:#}", err);
                        }
                        switcher.do_switch();
                        drop(activity);
                    })
                    .detach();
                } else {
                    switcher.do_switch();
                }
            }
            DetachDomain(domain) => {
                let domain = Mux::get().resolve_spawn_tab_domain(Some(pane.pane_id()), domain)?;
                domain.detach()?;
            }
            AttachDomain(domain) => {
                let window = self.mux_window_id;
                let domain = domain.to_string();
                let dpi = self.dimensions.dpi as u32;

                promise::spawn::spawn(async move {
                    let mux = Mux::get();
                    let domain = mux
                        .get_domain_by_name(&domain)
                        .ok_or_else(|| anyhow!("{} is not a valid domain name", domain))?;
                    domain.attach(Some(window)).await?;

                    let have_panes_in_domain = mux
                        .iter_panes()
                        .iter()
                        .any(|p| p.domain_id() == domain.domain_id());

                    if !have_panes_in_domain {
                        let config = config::configuration();
                        let _tab = domain
                            .spawn(
                                config.initial_size(
                                    dpi,
                                    Some(crate::cell_pixel_dims(&config, dpi as f64)?),
                                ),
                                None,
                                None,
                                window,
                            )
                            .await?;
                    }

                    Result::<(), anyhow::Error>::Ok(())
                })
                .detach();
            }
            CopyMode(_) => {
                // NOP here; handled by the overlay directly
            }
            RotatePanes(direction) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };
                let tab_id = tab.tab_id();
                let direction = *direction;
                promise::spawn::spawn(async move {
                    let mux = Mux::get();
                    if let Err(err) = mux.rotate_panes(tab_id, direction).await {
                        log::error!("Unable to rotate panes: {:#}", err);
                    }
                })
                .detach()
            }
            SplitPane(split) => {
                log::trace!("SplitPane {:?}", split);
                self.spawn_command(
                    &split.command,
                    SpawnWhere::SplitPane(SplitRequest {
                        direction: match split.direction {
                            PaneDirection::Down | PaneDirection::Up => SplitDirection::Vertical,
                            PaneDirection::Left | PaneDirection::Right => {
                                SplitDirection::Horizontal
                            }
                            PaneDirection::Next | PaneDirection::Prev => {
                                log::error!(
                                    "Invalid direction {:?} for SplitPane",
                                    split.direction
                                );
                                return Ok(PerformAssignmentResult::Handled);
                            }
                        },
                        target_is_second: match split.direction {
                            PaneDirection::Down | PaneDirection::Right => true,
                            PaneDirection::Up | PaneDirection::Left => false,
                            PaneDirection::Next | PaneDirection::Prev => unreachable!(),
                        },
                        size: match split.size {
                            SplitSize::Percent(n) => MuxSplitSize::Percent(n),
                            SplitSize::Cells(n) => MuxSplitSize::Cells(n),
                        },
                        top_level: split.top_level,
                    }),
                );
            }
            PaneSelect(args) => {
                let modal = crate::termwindow::paneselect::PaneSelector::new(self, args);
                self.set_modal(Rc::new(modal));
            }
            CharSelect(args) => {
                let modal = crate::termwindow::charselect::CharSelector::new(self, args);
                self.set_modal(Rc::new(modal));
            }
            ResetTerminal => {
                pane.perform_actions(vec![termwiz::escape::Action::Esc(
                    termwiz::escape::Esc::Code(termwiz::escape::EscCode::FullReset),
                )]);
            }
            OpenUri(link) => {
                wezterm_open_url::open_url(link);
            }
            OpenConfigFile => {
                if let Err(err) = open_config_file() {
                    log::error!("OpenConfigFile: {err:#}");
                    wezterm_toast_notification::persistent_toast_notification(
                        "OnlyTerm",
                        &format!("Couldn't open the config file: {err:#}"),
                    );
                }
            }
            ActivateCommandPalette => {
                let modal = crate::termwindow::palette::CommandPalette::new(self);
                self.set_modal(Rc::new(modal));
            }
            PromptInputLine(args) => self.show_prompt_input_line(args),
            InputSelector(args) => self.show_input_selector(args),
            Confirmation(args) => self.show_confirmation(args),
        };
        Ok(PerformAssignmentResult::Handled)
    }

    fn do_open_link_at_mouse_cursor(&self, _pane: &Arc<dyn Pane>) {
        // They clicked on a link, so let's open it!
        // We need to ensure that we spawn the `open` call outside of the context
        // of our window loop; on Windows it can cause a panic due to
        // triggering our WndProc recursively.
        if let Some(link) = self.current_highlight.as_ref().cloned() {
            let link = link.uri().to_string();
            promise::spawn::spawn(async move {
                log::info!("clicking {}", link);
                wezterm_open_url::open_url(&link);
            })
            .detach();
        }
    }

    fn do_copy_link_at_mouse_cursor(&self, destination: ClipboardCopyDestination) {
        // Right-click on a hyperlink copies its URL instead of opening it;
        // see `hyperlink_click_action` for the pure decision logic.
        if let Some(link) = self.current_highlight.as_ref().cloned() {
            self.copy_to_clipboard(destination, link.uri().to_string());
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }
    // OnlyTerm: never prompt on pane/tab close, regardless of the
    // `confirm` argument any caller passes (default keybindings, mouse
    // clicks on a tab's close button, etc.) - close-confirmation overlays
    // for panes/tabs are removed entirely, not just defaulted off.
    fn close_current_pane(&mut self, _confirm: bool) {
        let mux_window_id = self.mux_window_id;
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(mux_window_id) {
            Some(tab) => tab,
            None => return,
        };
        let pane = match tab.get_active_pane() {
            Some(p) => p,
            None => return,
        };

        mux.remove_pane(pane.pane_id());
    }

    fn close_specific_tab(&mut self, tab_idx: usize, _confirm: bool) {
        let mux = Mux::get();
        let mux_window_id = self.mux_window_id;
        let mux_window = match mux.get_window(mux_window_id) {
            Some(w) => w,
            None => return,
        };

        let tab = match mux_window.get_by_idx(tab_idx) {
            Some(tab) => Arc::clone(tab),
            None => return,
        };
        drop(mux_window);

        mux.remove_tab(tab.tab_id());
    }

    fn close_current_tab(&mut self, _confirm: bool) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };
        mux.remove_tab(tab.tab_id());
    }

    pub fn pane_state(&self, pane_id: PaneId) -> RefMut<'_, PaneState> {
        RefMut::map(self.pane_state.borrow_mut(), |state| {
            state.entry(pane_id).or_insert_with(PaneState::default)
        })
    }

    pub fn tab_state(&self, tab_id: TabId) -> RefMut<'_, TabState> {
        RefMut::map(self.tab_state.borrow_mut(), |state| {
            state.entry(tab_id).or_insert_with(TabState::default)
        })
    }

    /// Resize overlays to match their corresponding tab/pane dimensions
    pub fn resize_overlays(&self) {
        let mux = Mux::get();
        for (_, state) in self.tab_state.borrow().iter() {
            if let Some(overlay) = state.overlay.as_ref().map(|o| &o.pane) {
                overlay.resize(self.terminal_size).ok();
            }
        }
        for (pane_id, state) in self.pane_state.borrow().iter() {
            if let Some(overlay) = state.overlay.as_ref().map(|o| &o.pane) {
                if let Some(pane) = mux.get_pane(*pane_id) {
                    let dims = pane.get_dimensions();
                    overlay
                        .resize(TerminalSize {
                            cols: dims.cols,
                            rows: dims.viewport_rows,
                            dpi: self.terminal_size.dpi,
                            pixel_height: (self.terminal_size.pixel_height
                                / self.terminal_size.rows)
                                * dims.viewport_rows,
                            pixel_width: (self.terminal_size.pixel_width / self.terminal_size.cols)
                                * dims.cols,
                        })
                        .ok();
                }
            }
        }
    }

    pub fn get_viewport(&self, pane_id: PaneId) -> Option<StableRowIndex> {
        self.pane_state(pane_id).viewport
    }

    pub fn set_viewport(
        &mut self,
        pane_id: PaneId,
        position: Option<StableRowIndex>,
        dims: RenderableDimensions,
    ) {
        let pos = match position {
            Some(pos) => {
                // Drop out of scrolling mode if we're off the bottom
                if pos >= dims.physical_top {
                    None
                } else {
                    Some(pos.max(dims.scrollback_top))
                }
            }
            None => None,
        };

        let mut state = self.pane_state(pane_id);
        if pos != state.viewport {
            state.viewport = pos;

            // This is a bit gross.  If we add other overlays that need this information,
            // this should get extracted out into a trait
            if let Some(overlay) = state.overlay.as_ref() {
                if let Some(copy) = overlay.pane.downcast_ref::<CopyOverlay>() {
                    copy.viewport_changed(pos);
                } else if let Some(qs) = overlay.pane.downcast_ref::<QuickSelectOverlay>() {
                    qs.viewport_changed(pos);
                }
            }
        }
        self.window.as_ref().unwrap().invalidate();
    }

    fn maybe_scroll_to_bottom_for_input(&mut self, pane: &Arc<dyn Pane>) {
        if self.config.scroll_to_bottom_on_input {
            self.scroll_to_bottom(pane);
        }
    }

    fn scroll_to_top(&mut self, pane: &Arc<dyn Pane>) {
        let dims = pane.get_dimensions();
        self.set_viewport(pane.pane_id(), Some(dims.scrollback_top), dims);
    }

    fn scroll_to_bottom(&mut self, pane: &Arc<dyn Pane>) {
        self.pane_state(pane.pane_id()).viewport = None;
    }

    fn get_active_pane_no_overlay(&self) -> Option<Arc<dyn Pane>> {
        let mux = Mux::get();
        mux.get_active_tab_for_window(self.mux_window_id)
            .and_then(|tab| tab.get_active_pane())
    }

    /// Returns a Pane that we can interact with; this will typically be
    /// the active tab for the window, but if the window has a tab-wide
    /// overlay (such as the launcher / tab navigator),
    /// then that will be returned instead.  Otherwise, if the pane has
    /// an active overlay (such as search or copy mode) then that will
    /// be returned.
    pub fn get_active_pane_or_overlay(&self) -> Option<Arc<dyn Pane>> {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return None,
        };

        let tab_id = tab.tab_id();

        if let Some(tab_overlay) = self
            .tab_state(tab_id)
            .overlay
            .as_ref()
            .map(|overlay| overlay.pane.clone())
        {
            Some(tab_overlay)
        } else {
            let pane = tab.get_active_pane()?;
            let pane_id = pane.pane_id();
            self.pane_state(pane_id)
                .overlay
                .as_ref()
                .map(|overlay| overlay.pane.clone())
                .or_else(|| Some(pane))
        }
    }

    fn get_splits(&mut self) -> Vec<PositionedSplit> {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return vec![],
        };

        let tab_id = tab.tab_id();

        if self.tab_state(tab_id).overlay.is_some() {
            vec![]
        } else {
            tab.iter_splits()
        }
    }

    fn pos_pane_to_pane_info(pos: &PositionedPane) -> PaneInformation {
        PaneInformation {
            is_active: pos.is_active,
            is_zoomed: pos.is_zoomed,
            title: pos.pane.get_title(),
            progress: pos.pane.get_progress(),
            current_working_dir: pos
                .pane
                .get_current_working_dir(CachePolicy::AllowStale)
                .map(|url| url.to_string()),
        }
    }

    fn get_tab_information(&mut self) -> Vec<TabInformation> {
        let mux = Mux::get();
        let window = match mux.get_window(self.mux_window_id) {
            Some(window) => window,
            _ => return vec![],
        };
        let tab_index = window.get_active_idx();

        window
            .iter()
            .enumerate()
            .map(|(idx, tab)| {
                let panes = self.get_pos_panes_for_tab(tab);

                TabInformation {
                    tab_index: idx,
                    tab_id: tab.tab_id(),
                    is_active: tab_index == idx,
                    tab_title: tab.get_title(),
                    active_pane: panes
                        .iter()
                        .find(|p| p.is_active)
                        .map(Self::pos_pane_to_pane_info),
                }
            })
            .collect()
    }

    fn get_pane_information(&self) -> Vec<PaneInformation> {
        self.get_panes_to_render()
            .iter()
            .map(Self::pos_pane_to_pane_info)
            .collect()
    }

    fn get_pos_panes_for_tab(&self, tab: &Arc<Tab>) -> Vec<PositionedPane> {
        let tab_id = tab.tab_id();

        if let Some(pane) = self
            .tab_state(tab_id)
            .overlay
            .as_ref()
            .map(|overlay| overlay.pane.clone())
        {
            let size = tab.get_size();
            vec![PositionedPane {
                index: 0,
                is_active: true,
                is_zoomed: false,
                left: 0,
                top: 0,
                width: size.cols as _,
                height: size.rows as _,
                pixel_width: size.cols as usize * self.render_metrics.cell_size.width as usize,
                pixel_height: size.rows as usize * self.render_metrics.cell_size.height as usize,
                pane,
            }]
        } else {
            let mut panes = tab.iter_panes();
            for p in &mut panes {
                if let Some(overlay) = self.pane_state(p.pane.pane_id()).overlay.as_ref() {
                    p.pane = Arc::clone(&overlay.pane);
                }
            }
            panes
        }
    }

    fn get_panes_to_render(&self) -> Vec<PositionedPane> {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return vec![],
        };

        self.get_pos_panes_for_tab(&tab)
    }

    /// if pane_id.is_none(), removes any overlay for the specified tab.
    /// Otherwise: if the overlay is the specified pane for that tab, remove it.
    fn cancel_overlay_for_tab(&mut self, tab_id: TabId, pane_id: Option<PaneId>) {
        if pane_id.is_some() {
            let current = self
                .tab_state(tab_id)
                .overlay
                .as_ref()
                .map(|o| o.pane.pane_id());
            if current != pane_id {
                return;
            }
        }
        if let Some(overlay) = self.tab_state(tab_id).overlay.take() {
            Mux::get().remove_pane(overlay.pane.pane_id());
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub fn schedule_cancel_overlay(window: Window, tab_id: TabId, pane_id: Option<PaneId>) {
        window.notify(TermWindowNotif::CancelOverlayForTab { tab_id, pane_id });
    }

    fn cancel_overlay_for_pane(&mut self, pane_id: PaneId) {
        if let Some(overlay) = self.pane_state(pane_id).overlay.take() {
            // Ungh, when I built the CopyOverlay, its pane doesn't get
            // added to the mux and instead it reports the overlaid
            // pane id.  Take care to avoid killing ourselves off
            // when closing the CopyOverlay
            if pane_id != overlay.pane.pane_id() {
                Mux::get().remove_pane(overlay.pane.pane_id());
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub fn schedule_cancel_overlay_for_pane(window: Window, pane_id: PaneId) {
        window.notify(TermWindowNotif::CancelOverlayForPane(pane_id));
    }

    pub fn assign_overlay_for_pane(&mut self, pane_id: PaneId, pane: Arc<dyn Pane>) {
        self.cancel_overlay_for_pane(pane_id);
        self.pane_state(pane_id).overlay.replace(OverlayState {
            pane,
            key_table_state: KeyTableState::default(),
        });
        self.update_title();
    }

    pub fn assign_overlay(&mut self, tab_id: TabId, overlay: Arc<dyn Pane>) {
        self.cancel_overlay_for_tab(tab_id, None);
        self.tab_state(tab_id).overlay.replace(OverlayState {
            pane: overlay,
            key_table_state: KeyTableState::default(),
        });
        self.update_title();
    }

    fn resolve_search_pattern(&self, pattern: Pattern, pane: &Arc<dyn Pane>) -> MuxPattern {
        match pattern {
            Pattern::CaseSensitiveString(s) => MuxPattern::CaseSensitiveString(s),
            Pattern::CaseInSensitiveString(s) => MuxPattern::CaseInSensitiveString(s),
            Pattern::Regex(s) => MuxPattern::Regex(s),
            Pattern::CurrentSelectionOrEmptyString => {
                let text = self.selection_text(pane);
                let first_line = text
                    .lines()
                    .next()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                MuxPattern::CaseSensitiveString(first_line)
            }
        }
    }
}

impl Drop for TermWindow {
    fn drop(&mut self) {
        // Mark the mux subscription as dead.
        // (will actually unsubscribe on the next notif from mux)
        self.mux_subscription_dead.store(true, Ordering::Relaxed);
        self.clear_all_overlays();
        // Defensive: normally WindowEvent::Destroyed already took and shut
        // down the render thread, but handle the case where it never
        // fired (e.g. an early construction failure). Detach, don't join -
        // see the comment at the Destroyed handler for why.
        if let Some(rt) = self.render_thread.take() {
            rt.shutdown();
        }
        if let Some(window) = self.window.take() {
            if let Some(fe) = try_front_end() {
                fe.forget_known_window(&window);
            }
        }
    }
}
