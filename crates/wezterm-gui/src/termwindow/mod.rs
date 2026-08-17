#![allow(clippy::range_plus_one)]
use super::renderstate::*;
use super::utilsprites::RenderMetrics;
use crate::colorease::ColorEase;
use crate::frontend::try_front_end;
use crate::gui_api::guiwin::GuiWin;
use crate::inputmap::InputMap;
use crate::overlay::CopyOverlay;
use crate::scrollbar::*;
use crate::selection::Selection;
use crate::shapecache::*;
use crate::tabbar::{TabBarItem, TabBarState};
use crate::termwindow::background::LoadedBackgroundLayer;
use crate::termwindow::keyevent::KeyTableState;
use crate::termwindow::modal::Modal;
use crate::termwindow::render::paint::AllowImage;
use crate::termwindow::render::{
    LineQuadCacheKey, LineQuadCacheValue, LineToEleShapeCacheKey, LineToElementShapeItem,
};
use crate::termwindow::webgpu::WebGpuState;
use ::wezterm_term::input::{ClickPosition, MouseButton as TMB};
use ::window::*;
use config::keyassignment::KeyAssignment;
use config::{ConfigHandle, DimensionContext, GuiPosition};
use lfucache::*;
use mux::pane::{Pane, PaneId};
use mux::renderable::RenderableDimensions;
use mux::tab::{PositionedSplit, TabId};
use mux::window::WindowId as MuxWindowId;
use mux::MuxNotification;
use smol::channel::Sender;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, LinkedList};
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
use wezterm_term::{Progress, StableRowIndex, TerminalSize};

pub(crate) mod actions;
pub mod background;
pub mod box_model;
pub mod charselect;
pub mod clipboard;
pub mod keyevent;
pub mod modal;
mod mouseevent;
pub mod newtab_options;
pub mod palette;
pub mod paneselect;
mod prevcursor;
pub mod render;
mod render_pipeline;
pub mod resize;
mod selection;
pub mod spawn;
pub mod webgpu;
mod window_handler;
use prevcursor::PrevCursorPos;

const ATLAS_SIZE: usize = 128;

lazy_static::lazy_static! {
    static ref WINDOW_CLASS: Mutex<String> = Mutex::new(wezterm_gui_subcommands::DEFAULT_WINDOW_CLASS.to_owned());
    static ref POSITION: Mutex<Option<GuiPosition>> = Mutex::new(None);
}

pub const ICON_DATA: &[u8] = include_bytes!("../../../../assets/icon/terminal.png");

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
// `PerformAssignment`'s inline `KeyAssignment` already dwarfed the other
// variants before `SpawnCommand::title` added one more `Option<String>` to
// it; boxing that one field to shrink this enum isn't worth doing for a
// notification type that isn't sent at any meaningful rate.
#[allow(clippy::large_enum_variant)]
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
    NewTabOptionRadio {
        group: NewTabOptionGroup,
        choice: usize,
    },
    NewTabOptionRun,
    /// The dialog's close cross. Dismisses the modal, exactly as Esc does.
    NewTabOptionClose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewTabOptionGroup {
    Shell,
    Elevation,
    Priority,
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
    pub is_elevated: bool,
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

    renderer_info: Option<String>,

    /// Keeps track of double and triple clicks
    last_mouse_click: Option<LastMouseClick>,

    /// The URL over which we are currently hovering
    current_highlight: Option<Arc<Hyperlink>>,

    quad_generation: usize,
    shape_generation: usize,
    shape_cache: RefCell<LfuCache<ShapeCacheKey, anyhow::Result<Rc<Vec<ShapedInfo>>>>>,
    line_to_ele_shape_cache: RefCell<LfuCache<LineToEleShapeCacheKey, LineToElementShapeItem>>,

    /// Task #439: Shape hash cache keyed by (pane_id, stable_row) instead of Line appdata.
    /// This survives Line cloning because it's owned by TermWindow, not the Line.
    /// The key includes pane_id so that multiple panes in a tab (and multiple tabs)
    /// don't share cached hashes for logically different content.
    shape_hash_cache: RefCell<LfuCache<render::ShapeHashCacheKey, render::ShapeHashEntry>>,

    line_quad_cache: RefCell<LfuCache<LineQuadCacheKey, LineQuadCacheValue>>,

    /// What was last actually emitted for each visible row slot of each pane.
    /// Not a content cache (that's `line_quad_cache`, keyed by content) --
    /// this answers "what is currently on screen at visual row N", which is
    /// the only thing that can be re-emitted when the frame-build budget
    /// defers a row's rebuild. See task #457 / the @oh design review this
    /// implements.
    retained_rows: RefCell<HashMap<mux::pane::PaneId, render::RetainedPaneRows>>,

    last_status_call: Instant,
    /// Rate-limit state for title/tab-bar rebuilds; see
    /// TITLE_UPDATE_MIN_INTERVAL in actions.rs.
    title_update_coalescer: actions::TitleUpdateCoalescer,
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

    /// Set the first time any pane in this window produces non-empty pty
    /// output (task #385). Used as a practical proxy for "the shell is
    /// alive and likely ready to accept input" -- there is no harder
    /// "ready for input" handshake available (the alternative, waiting for
    /// a DA1/DSR-style negotiation to complete, isn't something every shell
    /// or program running in the pane will ever emit) -- and forwarded via
    /// `WindowOps::notify_shell_ready` to gate the Windows placeholder
    /// spinner's cross-fade into the real terminal content (see
    /// `window::os::windows::window::WindowInner::start_placeholder_fade`,
    /// which also waits for the renderer to be ready; whichever of the two
    /// conditions is satisfied later is what actually starts the fade).
    /// Checked-then-set in `mux_pane_output_event` so the notification is
    /// only ever sent once per window; a no-op on non-Windows platforms and
    /// harmless if sent more than once, but there's no reason to.
    shell_output_seen: bool,

    /// Set once `paint_impl` has cleared the GDI startup placeholder itself
    /// (task #425). This only ever happens on the *synchronous* path --
    /// when there is no dedicated render thread (`self.render_thread` is
    /// `None`) -- where `call_draw` returning `Ok` really does mean
    /// `WebGpuState::submit_frame` already ran inline. On the
    /// WebGpu-render-thread path (the default on Windows; see `config::
    /// webgpu_render_thread`), `call_draw` only *enqueues* the frame via
    /// `RenderThreadHandle::send_frame` and returns immediately -- the real
    /// `submit_frame`/`present()` runs later, asynchronously, on the render
    /// thread. Clearing here based on that enqueue (rather than the actual
    /// present) left a gap where a second, overlapping OnlyTerm window's
    /// content could show through instead of this window's own (task
    /// #407), so on that path the clear is done by
    /// `renderthread.rs`'s `submit_one_frame` itself, right after its first
    /// successful `submit_frame` -- via its own one-shot flag local to the
    /// render thread, not this field. `created()` installing a working
    /// `RenderState` is *not* the same event as either of these: on Windows
    /// the render thread is spawned (and, before that, the GPU
    /// device/pipeline itself finishes initializing) strictly after
    /// `created()` returns, and even without a render thread the first
    /// `NeedRepaint`/`do_paint_webgpu` call still needs a `WM_PAINT` message
    /// the message loop hasn't dispatched yet at that point. Calling
    /// `Window::clear_placeholder_background` from `created()` itself (the
    /// pre-#425 behavior) tore down the GDI placeholder -- the only thing
    /// painting the window's client area -- before any real content was
    /// queued, leaving a gap where Windows/DXGI showed undefined swapchain
    /// contents (commonly a black flash) between the placeholder
    /// disappearing and the first real frame landing. Checked-then-set from
    /// `paint_impl` so the clear happens at most once per window on the
    /// synchronous path this field actually gates.
    placeholder_cleared: bool,

    pub last_frame_duration: Duration,
    last_fps_check_time: Instant,
    num_frames: usize,
    pub fps: f32,

    connection_name: String,

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
    config_subscription: Option<config::ConfigSubscription>,
    /// Frame signature for deduplicating identical consecutive frames (task #450).
    /// `None` means "no valid previous frame signature to compare against" -
    /// reset on window creation, resize, renderer rebuild, or atlas resize,
    /// since these events can change what "identical" even means.
    last_frame_signature: Option<u64>,
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
        // Defensive, same reasoning as the Destroyed handler: mark the
        // WebGpuState stale in case Destroyed never fired, so a late
        // device-lost event doesn't try to notify this dropped window.
        // `mark_stale` takes `&self`, not ownership -- deliberately not
        // `.take()`n here, so `webgpu` still drops in its normal
        // field-declaration order (after `render_state`, which is declared
        // earlier in the struct and whose Drop impl needs the device
        // `webgpu` owns to still be alive) instead of being dropped early,
        // right here, ahead of it.
        if let Some(webgpu) = self.webgpu.as_ref() {
            webgpu.mark_stale();
        }
        if let Some(window) = self.window.take() {
            if let Some(fe) = try_front_end() {
                fe.forget_known_window(&window);
            }
        }
    }
}
