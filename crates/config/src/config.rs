use crate::background::{BackgroundLayer, Gradient};
use crate::bell::{AudibleBell, EasingFunction, VisualBell};
use crate::color::{
    ColorSchemeFile, HsbTransform, Palette, SrgbaTuple, TabBarColor, TabBarColors, TabBarStyle,
    WindowFrameConfig,
};
use crate::daemon::DaemonOptions;
use crate::exec_domain::ExecDomain;
use crate::font::{
    AllowSquareGlyphOverflow, DisplayPixelGeometry, FontLocatorSelection, FontRasterizerSelection,
    FontShaperSelection, FreeTypeLoadFlags, FreeTypeLoadTarget, StyleRule, TextStyle,
};
use crate::frontend::FrontEndSelection;
use crate::keyassignment::{
    KeyAssignment, KeyTable, KeyTableEntry, KeyTables, MouseEventTrigger, SpawnCommand,
};
use crate::keys::{Key, LeaderKey, Mouse};
use crate::rhai_engine::{self, make_rhai_engine, RhaiConfigEngine};
use crate::units::Dimension;
use crate::unix::UnixDomain;
use crate::wsl::WslDomain;
use crate::{
    default_config_with_overrides_applied, default_one_point_oh, default_one_point_oh_f64,
    default_true, default_win32_acrylic_accent_color, CellWidth, GpuInfo,
    IntegratedTitleButtonColor, KeyMapPreference, LoadedConfig, MouseEventTriggerMods, RgbaColor,
    SerialDomain, SystemBackdrop, WebGpuPowerPreference, CONFIG_DIRS, CONFIG_FILE_OVERRIDE,
    CONFIG_OVERRIDES, CONFIG_SKIP, HOME_DIR,
};
use anyhow::Context;
use portable_pty::CommandBuilder;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;
use termwiz::hyperlink;
use termwiz::surface::CursorShape;
use wezterm_bidi::ParagraphDirectionHint;
use wezterm_config_derive::ConfigMeta;
use wezterm_dynamic::{FromDynamic, ToDynamic};
use wezterm_input_types::{
    IntegratedTitleButton, IntegratedTitleButtonAlignment, IntegratedTitleButtonStyle, Modifiers,
    UIKeyCapRendering, WindowDecorations,
};
use wezterm_term::TerminalSize;

#[derive(Debug, Clone, FromDynamic, ToDynamic, ConfigMeta)]
pub struct Config {
    /// The font size, measured in points
    #[dynamic(default = "default_font_size")]
    pub font_size: f64,

    #[dynamic(
        default = "default_one_point_oh_f64",
        validate = "validate_line_height"
    )]
    pub line_height: f64,

    #[dynamic(default = "default_one_point_oh_f64")]
    pub cell_width: f64,

    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub cursor_thickness: Option<Dimension>,

    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub underline_thickness: Option<Dimension>,

    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub underline_position: Option<Dimension>,

    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub strikethrough_position: Option<Dimension>,

    #[dynamic(default)]
    pub allow_square_glyphs_to_overflow_width: AllowSquareGlyphOverflow,

    #[dynamic(default)]
    pub window_decorations: WindowDecorations,

    #[dynamic(default = "default_integrated_title_buttons")]
    pub integrated_title_buttons: Vec<IntegratedTitleButton>,

    #[dynamic(default)]
    pub log_unknown_escape_sequences: bool,

    #[dynamic(default)]
    pub integrated_title_button_alignment: IntegratedTitleButtonAlignment,

    #[dynamic(default)]
    pub integrated_title_button_style: IntegratedTitleButtonStyle,

    #[dynamic(default)]
    pub integrated_title_button_color: IntegratedTitleButtonColor,

    /// When using FontKitXXX font systems, a set of directories to
    /// search ahead of the standard font locations for fonts.
    /// Relative paths are taken to be relative to the directory
    /// from which the config was loaded.
    #[dynamic(default)]
    pub font_dirs: Vec<PathBuf>,

    #[dynamic(default)]
    pub color_scheme_dirs: Vec<PathBuf>,

    /// The DPI to assume
    pub dpi: Option<f64>,

    #[dynamic(default)]
    pub dpi_by_screen: HashMap<String, f64>,

    /// The baseline font to use
    #[dynamic(default)]
    pub font: TextStyle,

    /// An optional set of style rules to select the font based
    /// on the cell attributes
    #[dynamic(default)]
    pub font_rules: Vec<StyleRule>,

    /// When true (the default), PaletteIndex 0-7 are shifted to
    /// bright when the font intensity is bold.  The brightening
    /// doesn't apply to text that is the default color.
    #[dynamic(default)]
    pub bold_brightens_ansi_colors: BoldBrightening,

    /// The color palette
    #[dynamic(default = "default_colors")]
    pub colors: Option<Palette>,

    #[dynamic(default)]
    pub switch_to_last_active_tab_when_closing_tab: bool,

    /// When true, launching a new wezterm instance will prefer
    /// to spawn a new tab into an existing instance.
    /// Otherwise, it will spawn a new window.
    #[dynamic(default)]
    pub prefer_to_spawn_tabs: bool,

    #[dynamic(default)]
    pub window_frame: WindowFrameConfig,

    /// Font to use for CharSelect
    #[dynamic(default)]
    pub char_select_font: Option<TextStyle>,

    #[dynamic(default = "default_char_select_font_size")]
    pub char_select_font_size: f64,

    #[dynamic(default = "default_char_select_fg_color")]
    pub char_select_fg_color: RgbaColor,

    #[dynamic(default = "default_char_select_bg_color")]
    pub char_select_bg_color: RgbaColor,

    /// Font to use for ActivateCommandPalette
    #[dynamic(default)]
    pub command_palette_font: Option<TextStyle>,

    #[dynamic(default = "default_command_palette_font_size")]
    pub command_palette_font_size: f64,

    pub command_palette_rows: Option<usize>,
    #[dynamic(default = "default_command_palette_fg_color")]
    pub command_palette_fg_color: RgbaColor,

    #[dynamic(default = "default_command_palette_bg_color")]
    pub command_palette_bg_color: RgbaColor,

    /// Font to use for PaneSelect
    #[dynamic(default)]
    pub pane_select_font: Option<TextStyle>,

    #[dynamic(default = "default_pane_select_font_size")]
    pub pane_select_font_size: f64,

    #[dynamic(default = "default_pane_select_fg_color")]
    pub pane_select_fg_color: RgbaColor,

    #[dynamic(default = "default_pane_select_bg_color")]
    pub pane_select_bg_color: RgbaColor,

    #[dynamic(default)]
    pub tab_bar_style: TabBarStyle,

    #[dynamic(default)]
    pub resolved_palette: Palette,

    /// Use a named color scheme rather than the palette specified
    /// by the colors setting.
    pub color_scheme: Option<String>,

    /// Named color schemes
    #[dynamic(default)]
    pub color_schemes: HashMap<String, Palette>,

    /// How many lines of scrollback you want to retain
    #[dynamic(
        default = "default_scrollback_lines",
        validate = "validate_scrollback_lines"
    )]
    pub scrollback_lines: usize,

    /// If no `prog` is specified on the command line, use this
    /// instead of running the user's shell.
    /// For example, to have `wezterm` always run `top` by default,
    /// you'd use this:
    ///
    /// ```toml
    /// default_prog = ["top"]
    /// ```
    ///
    /// `default_prog` is implemented as an array where the 0th element
    /// is the command to run and the rest of the elements are passed
    /// as the positional arguments to that command.
    #[dynamic(default = "windows_default_prog")]
    pub default_prog: Option<Vec<String>>,

    #[dynamic(default = "default_gui_startup_args")]
    pub default_gui_startup_args: Vec<String>,

    /// Specifies the default current working directory if none is specified
    /// through configuration or OSC 7 (see docs for `default_cwd` for more
    /// info!)
    pub default_cwd: Option<PathBuf>,

    #[dynamic(default)]
    pub exit_behavior: ExitBehavior,

    #[dynamic(default)]
    pub exit_behavior_messaging: ExitBehaviorMessaging,

    #[dynamic(default = "default_clean_exits")]
    pub clean_exit_codes: Vec<u32>,

    #[dynamic(default = "default_true")]
    pub detect_password_input: bool,

    /// Specifies a map of environment variables that should be set
    /// when spawning commands in the local domain.
    /// This is not used when working with remote domains.
    #[dynamic(default)]
    pub set_environment_variables: HashMap<String, String>,

    /// Specifies the height of a new window, expressed in character cells.
    #[dynamic(default = "default_initial_rows", validate = "validate_row_or_col")]
    pub initial_rows: u16,

    #[dynamic(default = "default_true")]
    pub enable_kitty_graphics: bool,
    /// Third attempt at defaulting this to true, so that apps requesting
    /// the kitty keyboard protocol at runtime (eg. Codex CLI, which needs
    /// it to disambiguate Ctrl+Enter/Shift+Enter from a plain Enter) get
    /// it without a config change. The first two attempts broke Ctrl+C:
    /// once an app enabled DISAMBIGUATE_ESCAPE_CODES, the terminal-side
    /// `CopySelectionOrInterrupt` binding (the default Ctrl+C action) kept
    /// writing the legacy `\x03` byte directly instead of the CSI-u form
    /// the app had asked for and was now expecting, so its own Ctrl+C
    /// handling in that mode never saw a recognized interrupt sequence.
    /// That binding now encodes through the pane's actual negotiated
    /// keyboard protocol (see `CopySelectionOrInterrupt` in
    /// `termwindow/mod.rs`) instead of hardcoding the legacy byte, which
    /// was the real root cause both previous attempts never pinned down.
    #[dynamic(default = "default_true")]
    pub enable_kitty_keyboard: bool,

    /// Whether the terminal should respond to requests to read the
    /// title string.
    /// Disabled by default for security concerns with shells that might
    /// otherwise attempt to execute the response.
    /// <https://marc.info/?l=bugtraq&m=104612710031920&w=2>
    #[dynamic(default)]
    pub enable_title_reporting: bool,

    /// Whether the terminal should respond to DECRQCRA checksum requests.
    /// Disabled by default as it allows programs to read screen contents.
    /// <https://vt100.net/docs/vt510-rm/DECRQCRA.html>
    #[dynamic(default)]
    pub enable_checksum_rectangular_area: bool,

    /// Specifies the width of a new window, expressed in character cells
    #[dynamic(default = "default_initial_cols", validate = "validate_row_or_col")]
    pub initial_cols: u16,

    #[dynamic(default = "default_hyperlink_rules")]
    pub hyperlink_rules: Vec<hyperlink::Rule>,

    /// What to set the TERM variable to
    #[dynamic(default = "default_term")]
    pub term: String,

    #[dynamic(default)]
    pub font_locator: FontLocatorSelection,
    #[dynamic(default)]
    pub font_rasterizer: FontRasterizerSelection,
    #[dynamic(default = "default_colr_rasterizer")]
    pub font_colr_rasterizer: FontRasterizerSelection,
    #[dynamic(default)]
    pub font_shaper: FontShaperSelection,

    #[dynamic(default)]
    pub display_pixel_geometry: DisplayPixelGeometry,
    #[dynamic(default)]
    pub freetype_load_target: FreeTypeLoadTarget,
    #[dynamic(default)]
    pub freetype_render_target: Option<FreeTypeLoadTarget>,
    #[dynamic(default)]
    pub freetype_load_flags: Option<FreeTypeLoadFlags>,

    /// Selects the freetype interpret version to use.
    /// Likely values are 35, 38 and 40 which have different
    /// characteristics with respective to subpixel hinting.
    /// See https://freetype.org/freetype2/docs/subpixel-hinting.html
    pub freetype_interpreter_version: Option<u32>,

    #[dynamic(default)]
    pub freetype_pcf_long_family_names: bool,

    /// Specify the features to enable when using harfbuzz for font shaping.
    /// There is some light documentation here:
    /// <https://harfbuzz.github.io/shaping-opentype-features.html>
    /// but it boils down to allowing opentype feature names to be specified
    /// using syntax similar to the CSS font-feature-settings options:
    /// <https://developer.mozilla.org/en-US/docs/Web/CSS/font-feature-settings>.
    /// The OpenType spec lists a number of features here:
    /// <https://docs.microsoft.com/en-us/typography/opentype/spec/featurelist>
    ///
    /// Options of likely interest will be:
    ///
    /// * `calt` - <https://docs.microsoft.com/en-us/typography/opentype/spec/features_ae#tag-calt>
    /// * `clig` - <https://docs.microsoft.com/en-us/typography/opentype/spec/features_ae#tag-clig>
    ///
    /// If you want to disable ligatures in most fonts, then you may want to
    /// use a setting like this:
    ///
    /// ```toml
    /// harfbuzz_features = ["calt=0", "clig=0", "liga=0"]
    /// ```
    ///
    /// Some fonts make available extended options via stylistic sets.
    /// If you use the [Fira Code font](https://github.com/tonsky/FiraCode),
    /// it lists available stylistic sets here:
    /// <https://github.com/tonsky/FiraCode/wiki/How-to-enable-stylistic-sets>
    ///
    /// and you can set them in wezterm:
    ///
    /// ```toml
    /// # Use this for a zero with a dot rather than a line through it
    /// # when using the Fira Code font
    /// harfbuzz_features = ["zero"]
    /// ```
    #[dynamic(default = "default_harfbuzz_features")]
    pub harfbuzz_features: Vec<String>,

    #[dynamic(default = "default_front_end")]
    pub front_end: FrontEndSelection,

    /// Whether to select the higher powered discrete GPU when
    /// the system has a choice of integrated or discrete.
    /// Defaults to low power.
    #[dynamic(default)]
    pub webgpu_power_preference: WebGpuPowerPreference,

    #[dynamic(default)]
    pub webgpu_force_fallback_adapter: bool,

    #[dynamic(default)]
    pub webgpu_preferred_adapter: Option<GpuInfo>,

    #[dynamic(default)]
    pub wsl_domains: Option<Vec<WslDomain>>,

    #[dynamic(default)]
    pub exec_domains: Vec<ExecDomain>,

    #[dynamic(default)]
    pub serial_ports: Vec<SerialDomain>,

    /// The set of unix domains
    #[dynamic(default = "UnixDomain::default_unix_domains")]
    pub unix_domains: Vec<UnixDomain>,

    /// Constrains the rate at which the multiplexer client will
    /// speculatively fetch line data.
    /// This helps to avoid saturating the link between the client
    /// and server if the server is dumping a large amount of output
    /// to the client.
    #[dynamic(default = "default_ratelimit_line_prefetches_per_second")]
    pub ratelimit_mux_line_prefetches_per_second: u32,

    /// The buffer size used by parse_buffered_data in the mux module.
    /// This should not be too large, otherwise the processing cost
    /// of applying a batch of actions to the terminal will be too
    /// high and the user experience will be laggy and less responsive.
    #[dynamic(default = "default_mux_output_parser_buffer_size")]
    pub mux_output_parser_buffer_size: usize,

    /// Applying a full `mux_output_parser_buffer_size`-sized batch of
    /// parsed actions to a pane's terminal model happens under a single
    /// mutex acquisition; for the default 128KiB buffer size that batch
    /// can be tens of thousands of actions, and applying all of them
    /// before releasing the lock has been measured to hold it for
    /// 40ms+, starving keyboard/mouse input and rendering (both of
    /// which block on the same lock) for that whole span.
    /// `mux_output_parser_chunk_size` bounds how many parsed actions are
    /// applied per lock acquisition: the pane splits a large batch into
    /// chunks of at most this many actions, releasing and re-acquiring
    /// the terminal lock between chunks so that input handling and
    /// rendering get a chance to run. Chunking only ever splits between
    /// whole, already-parsed `Action`s -- it never interrupts a single
    /// escape sequence -- so the final terminal state is identical to
    /// applying the whole batch at once.
    #[dynamic(default = "default_mux_output_parser_chunk_size")]
    pub mux_output_parser_chunk_size: usize,

    #[dynamic(default = "default_true")]
    pub mux_enable_ssh_agent: bool,

    #[dynamic(default)]
    pub default_ssh_auth_sock: Option<String>,

    /// How many ms to delay after reading a chunk of output
    /// in order to try to coalesce fragmented writes into
    /// a single bigger chunk of output and reduce the chances
    /// observing "screen tearing" with un-synchronized output
    #[dynamic(default = "default_mux_output_parser_coalesce_delay_ms")]
    pub mux_output_parser_coalesce_delay_ms: u64,

    /// How many ms a synchronized update (DEC private mode 2026) may
    /// hold back output before wezterm stops waiting for the closing
    /// sequence and applies the buffered output anyway, so that a
    /// stalled application cannot freeze the pane indefinitely
    #[dynamic(default = "default_mux_synchronized_output_timeout_ms")]
    pub mux_synchronized_output_timeout_ms: u64,

    #[dynamic(default = "default_mux_env_remove")]
    pub mux_env_remove: Vec<String>,

    #[dynamic(default)]
    pub keys: Vec<Key>,
    #[dynamic(default)]
    pub key_tables: HashMap<String, Vec<Key>>,

    #[dynamic(default = "default_bypass_mouse_reporting_modifiers")]
    pub bypass_mouse_reporting_modifiers: Modifiers,

    #[dynamic(default)]
    pub debug_key_events: bool,

    #[dynamic(default)]
    pub normalize_output_to_unicode_nfc: bool,

    #[dynamic(default)]
    pub disable_default_key_bindings: bool,
    pub leader: Option<LeaderKey>,

    #[dynamic(default = "default_num_alphabet")]
    pub launcher_alphabet: String,

    #[dynamic(default)]
    pub disable_default_quick_select_patterns: bool,
    #[dynamic(default)]
    pub quick_select_patterns: Vec<String>,
    #[dynamic(default = "default_alphabet")]
    pub quick_select_alphabet: String,
    #[dynamic(default)]
    pub quick_select_remove_styling: bool,

    #[dynamic(default)]
    pub mouse_bindings: Vec<Mouse>,
    #[dynamic(default)]
    pub disable_default_mouse_bindings: bool,

    #[dynamic(default)]
    pub daemon_options: DaemonOptions,

    #[dynamic(default)]
    pub send_composed_key_when_left_alt_is_pressed: bool,

    #[dynamic(default = "default_true")]
    pub send_composed_key_when_right_alt_is_pressed: bool,

    #[dynamic(default = "default_macos_forward_mods")]
    pub macos_forward_to_ime_modifier_mask: Modifiers,

    #[dynamic(default)]
    pub treat_left_ctrlalt_as_altgr: bool,

    /// If true, the `Backspace` and `Delete` keys generate `Delete` and `Backspace`
    /// keypresses, respectively, rather than their normal keycodes.
    /// On macOS the default for this is true because its Backspace key
    /// is labeled as Delete and things are backwards.
    #[dynamic(default = "default_swap_backspace_and_delete")]
    pub swap_backspace_and_delete: bool,

    /// If true, display the tab bar UI at the top of the window.
    /// The tab bar shows the titles of the tabs and which is the
    /// active tab.  Clicking on a tab activates it.
    #[dynamic(default = "default_true")]
    pub enable_tab_bar: bool,
    #[dynamic(default = "default_true")]
    pub use_fancy_tab_bar: bool,

    #[dynamic(default)]
    pub tab_bar_at_bottom: bool,

    #[dynamic(default = "default_true")]
    pub mouse_wheel_scrolls_tabs: bool,

    /// If true, tab bar titles are prefixed with the tab index
    #[dynamic(default = "default_true")]
    pub show_tab_index_in_tab_bar: bool,

    #[dynamic(default = "default_true")]
    pub show_tabs_in_tab_bar: bool,

    #[dynamic(default = "default_true")]
    pub show_new_tab_button_in_tab_bar: bool,

    #[dynamic(default = "default_true")]
    pub show_close_tab_button_in_tabs: bool,

    /// If true, show_tab_index_in_tab_bar uses a zero-based index.
    /// The default is false and the tab shows a one-based index.
    #[dynamic(default)]
    pub tab_and_split_indices_are_zero_based: bool,

    /// If true, the default tab title (used when the tab doesn't have an
    /// explicitly assigned title and no `format-tab-title` event handler
    /// is registered) is derived from the last path component (basename)
    /// of the active pane's current working directory, rather than from
    /// the pane's title (which is usually the running program's name).
    /// The same applies to the default window title (`format-window-title`
    /// fallback). OnlyTerm defaults this to true (upstream wezterm
    /// defaults to false) so the tab/window title tracks `cd` out of the
    /// box - it updates whenever the shell reports a new working directory
    /// (OSC 7), without needing a config-side event handler.
    #[dynamic(default = "default_true")]
    pub use_cwd_basename_as_tab_title: bool,

    /// If true, new GUI windows are maximized immediately after being
    /// shown. Upstream wezterm has no built-in option for this - users
    /// would normally add a `gui-startup` event handler calling
    /// `window:gui_window():maximize()`. OnlyTerm defaults this to true.
    #[dynamic(default = "default_true")]
    pub start_maximized: bool,

    /// Specifies the maximum width that a tab can have in the
    /// tab bar.  Defaults to 16 glyphs in width.
    #[dynamic(default = "default_tab_max_width")]
    pub tab_max_width: usize,

    /// If true, hide the tab bar if the window only has a single tab.
    #[dynamic(default)]
    pub hide_tab_bar_if_only_one_tab: bool,

    #[dynamic(default = "default_true")]
    pub enable_scroll_bar: bool,

    #[dynamic(
        try_from = "crate::units::PixelUnit",
        default = "default_min_scroll_bar_height"
    )]
    pub min_scroll_bar_height: Dimension,

    /// If false, do not try to use a Wayland protocol connection
    /// when starting the gui frontend, and instead use X11.
    /// This option is only considered on X11/Wayland systems and
    /// has no effect on macOS or Windows.
    /// The default is true.
    #[dynamic(default = "default_true")]
    pub enable_wayland: bool,
    #[dynamic(default)]
    pub enable_zwlr_output_manager: bool,

    /// Whether to prefer EGL over other GL implementations.
    /// EGL on Windows has jankier resize behavior than WGL (which
    /// is used if EGL is unavailable), but EGL survives graphics
    /// driver updates without breaking and losing your work.
    #[dynamic(default = "default_prefer_egl")]
    pub prefer_egl: bool,

    #[dynamic(default = "default_true")]
    pub custom_block_glyphs: bool,
    #[dynamic(default = "default_true")]
    pub anti_alias_custom_block_glyphs: bool,

    /// Controls the amount of padding to use around the terminal cell area
    #[dynamic(default)]
    pub window_padding: WindowPadding,

    #[dynamic(default)]
    pub window_content_alignment: WindowContentAlignment,

    /// Specifies the path to a background image attachment file.
    /// The file can be any image format that the rust `image`
    /// crate is able to identify and load.
    /// A window background image is rendered into the background
    /// of the window before any other content.
    ///
    /// The image will be scaled to fit the window.
    #[dynamic(default)]
    pub window_background_image: Option<PathBuf>,
    #[dynamic(default)]
    pub window_background_gradient: Option<Gradient>,
    #[dynamic(default)]
    pub window_background_image_hsb: Option<HsbTransform>,
    #[dynamic(default)]
    pub foreground_text_hsb: HsbTransform,

    #[dynamic(default)]
    pub background: Vec<BackgroundLayer>,

    /// Only works on MacOS
    #[dynamic(default)]
    pub macos_window_background_blur: i64,

    /// Only works on KDE Wayland
    #[dynamic(
        default,
        deprecated = "this option has been replaced with `wayland_window_background_blur` and will be removed in a future release"
    )]
    pub kde_window_background_blur: bool,

    /// Only works on Wayland compositors that support ext-background-effect-v1 protocol
    #[dynamic(default)]
    pub wayland_window_background_blur: bool,

    /// Only works on Windows
    #[dynamic(default)]
    pub win32_system_backdrop: SystemBackdrop,

    #[dynamic(default = "default_win32_acrylic_accent_color")]
    pub win32_acrylic_accent_color: RgbaColor,

    /// Specifies the alpha value to use when rendering the background
    /// of the window.  The background is taken either from the
    /// window_background_image, or if there is none, the background
    /// color of the cell in the current position.
    /// The default is 1.0 which is 100% opaque.  Setting it to a number
    /// between 0.0 and 1.0 will allow for the screen behind the window
    /// to "shine through" to varying degrees.
    /// This only works on systems with a compositing window manager.
    /// Setting opacity to a value other than 1.0 can impact render
    /// performance.
    #[dynamic(default = "default_one_point_oh")]
    pub window_background_opacity: f32,

    /// inactive_pane_hue, inactive_pane_saturation and
    /// inactive_pane_brightness allow for transforming the color
    /// of inactive panes.
    /// The pane colors are converted to HSV values and multiplied
    /// by these values before being converted back to RGB to
    /// use in the display.
    ///
    /// The default is 1.0 which leaves the values as-is.
    ///
    /// Modifying the hue changes the hue of the color by rotating
    /// it through the color wheel.  It is not as useful as the
    /// other components, but is available "for free" as part of
    /// the colorspace conversion.
    ///
    /// Modifying the saturation can add or reduce the amount of
    /// "colorfulness".  Making the value smaller can make it appear
    /// more washed out.
    ///
    /// Modifying the brightness can be used to dim or increase
    /// the perceived amount of light.
    ///
    /// The range of these values is 0.0 and up; they are used to
    /// multiply the existing values, so the default of 1.0
    /// preserves the existing component, whilst 0.5 will reduce
    /// it by half, and 2.0 will double the value.
    ///
    /// A subtle dimming effect can be achieved by setting:
    /// inactive_pane_saturation = 0.9
    /// inactive_pane_brightness = 0.8
    #[dynamic(default = "default_inactive_pane_hsb")]
    pub inactive_pane_hsb: HsbTransform,

    #[dynamic(default = "default_one_point_oh")]
    pub text_background_opacity: f32,

    /// Specifies how often a blinking cursor transitions between visible
    /// and invisible, expressed in milliseconds.
    /// Setting this to 0 disables blinking.
    /// Note that this value is approximate due to the way that the system
    /// event loop schedulers manage timers; non-zero values will be at
    /// least the interval specified with some degree of slop.
    #[dynamic(default = "default_cursor_blink_rate")]
    pub cursor_blink_rate: u64,
    #[dynamic(default = "linear_ease")]
    pub cursor_blink_ease_in: EasingFunction,
    #[dynamic(default = "linear_ease")]
    pub cursor_blink_ease_out: EasingFunction,

    #[dynamic(default = "default_anim_fps")]
    pub animation_fps: u8,

    #[dynamic(default)]
    pub text_min_contrast_ratio: Option<f32>,

    #[dynamic(default)]
    pub force_reverse_video_cursor: bool,
    #[dynamic(default = "default_reverse_video_cursor_min_contrast")]
    pub reverse_video_cursor_min_contrast: f32,

    /// Specifies the default cursor style.  various escape sequences
    /// can override the default style in different situations (eg:
    /// an editor can change it depending on the mode), but this value
    /// controls how the cursor appears when it is reset to default.
    /// The default is `SteadyBlock`.
    /// Acceptable values are `SteadyBlock`, `BlinkingBlock`,
    /// `SteadyUnderline`, `BlinkingUnderline`, `SteadyBar`,
    /// and `BlinkingBar`.
    #[dynamic(default)]
    pub default_cursor_style: DefaultCursorStyle,

    /// Specifies how often blinking text (normal speed) transitions
    /// between visible and invisible, expressed in milliseconds.
    /// Setting this to 0 disables slow text blinking.  Note that this
    /// value is approximate due to the way that the system event loop
    /// schedulers manage timers; non-zero values will be at least the
    /// interval specified with some degree of slop.
    #[dynamic(default = "default_text_blink_rate")]
    pub text_blink_rate: u64,
    #[dynamic(default = "linear_ease")]
    pub text_blink_ease_in: EasingFunction,
    #[dynamic(default = "linear_ease")]
    pub text_blink_ease_out: EasingFunction,

    /// Specifies how often blinking text (rapid speed) transitions
    /// between visible and invisible, expressed in milliseconds.
    /// Setting this to 0 disables rapid text blinking.  Note that this
    /// value is approximate due to the way that the system event loop
    /// schedulers manage timers; non-zero values will be at least the
    /// interval specified with some degree of slop.
    #[dynamic(default = "default_text_blink_rate_rapid")]
    pub text_blink_rate_rapid: u64,
    #[dynamic(default = "linear_ease")]
    pub text_blink_rapid_ease_in: EasingFunction,
    #[dynamic(default = "linear_ease")]
    pub text_blink_rapid_ease_out: EasingFunction,

    /// If true, the mouse cursor will be hidden while typing.
    /// This option is true by default.
    #[dynamic(default = "default_true")]
    pub hide_mouse_cursor_when_typing: bool,

    /// If non-zero, specifies the period (in seconds) at which various
    /// statistics are logged.  Note that there is a minimum period of
    /// 10 seconds.
    #[dynamic(default)]
    pub periodic_stat_logging: u64,

    /// If false, do not scroll to the bottom of the terminal when
    /// you send input to the terminal.
    /// The default is to scroll to the bottom when you send input
    /// to the terminal.
    #[dynamic(default = "default_true")]
    pub scroll_to_bottom_on_input: bool,

    #[dynamic(default = "default_true")]
    pub use_ime: bool,
    #[dynamic(default)]
    pub xim_im_name: Option<String>,
    #[dynamic(default)]
    pub ime_preedit_rendering: ImePreeditRendering,

    #[dynamic(default)]
    pub notification_handling: NotificationHandling,

    #[dynamic(default = "default_true")]
    pub use_dead_keys: bool,

    #[dynamic(default)]
    pub launch_menu: Vec<SpawnCommand>,

    #[dynamic(default)]
    pub use_box_model_render: bool,

    /// When true, watch the config file and reload it automatically
    /// when it is detected as changing.
    #[dynamic(default = "default_true")]
    pub automatically_reload_config: bool,

    #[dynamic(default = "default_check_for_updates")]
    pub check_for_updates: bool,
    #[dynamic(
        default,
        deprecated = "this option no longer does anything and will be removed in a future release"
    )]
    pub show_update_window: bool,

    #[dynamic(default = "default_update_interval")]
    pub check_for_updates_interval_seconds: u64,

    /// When set to true, use the CSI-U encoding scheme as described
    /// in http://www.leonerd.org.uk/hacks/fixterms/
    /// This is off by default because @wez and @jsgf find the shift-space
    /// mapping annoying in vim :-p
    #[dynamic(default)]
    pub enable_csi_u_key_encoding: bool,

    #[dynamic(default)]
    pub window_close_confirmation: WindowCloseConfirmation,

    #[dynamic(default)]
    pub native_macos_fullscreen_mode: bool,

    #[dynamic(default)]
    pub macos_fullscreen_extend_behind_notch: bool,

    #[dynamic(default = "default_word_boundary")]
    pub selection_word_boundary: String,

    #[dynamic(default = "default_enq_answerback")]
    pub enq_answerback: String,

    #[dynamic(default)]
    pub adjust_window_size_when_changing_font_size: Option<bool>,

    #[dynamic(default = "default_tiling_desktop_environments")]
    pub tiling_desktop_environments: Vec<String>,

    #[dynamic(default)]
    pub use_resize_increments: bool,

    #[dynamic(default = "default_alternate_buffer_wheel_scroll_speed")]
    pub alternate_buffer_wheel_scroll_speed: u8,

    #[dynamic(default = "default_status_update_interval")]
    pub status_update_interval: u64,

    #[dynamic(default)]
    pub experimental_pixel_positioning: bool,

    #[dynamic(default)]
    pub ignore_svg_fonts: bool,

    /// OnlyTerm bundles Hebrew fonts/niqqud support out of the box, so
    /// bidi (Unicode Bidirectional Algorithm, UAX #9) is on by default
    /// rather than requiring users to opt in.
    #[dynamic(default = "default_true")]
    pub bidi_enabled: bool,

    #[dynamic(default = "default_bidi_direction")]
    pub bidi_direction: ParagraphDirectionHint,

    #[dynamic(default = "default_stateless_process_list")]
    pub skip_close_confirmation_for_processes_named: Vec<String>,

    #[dynamic(default = "default_true")]
    pub quit_when_all_windows_are_closed: bool,

    #[dynamic(default = "default_true")]
    pub warn_about_missing_glyphs: bool,

    #[dynamic(default)]
    pub sort_fallback_fonts_by_coverage: bool,

    #[dynamic(default)]
    pub search_font_dirs_for_fallback: bool,

    #[dynamic(default)]
    pub use_cap_height_to_scale_fallback_fonts: bool,

    #[dynamic(default)]
    pub swallow_mouse_click_on_pane_focus: bool,

    #[dynamic(default = "default_swallow_mouse_click_on_window_focus")]
    pub swallow_mouse_click_on_window_focus: bool,

    #[dynamic(default)]
    pub pane_focus_follows_mouse: bool,

    #[dynamic(default = "default_true")]
    pub unzoom_on_switch_pane: bool,

    #[dynamic(default = "default_max_fps")]
    pub max_fps: u64,

    /// When true (the default), a background watchdog thread monitors the
    /// GUI thread's message loop and logs+counts when it appears to be
    /// stuck (see `gui_watchdog_threshold_ms`).
    #[dynamic(default = "default_true")]
    pub gui_watchdog_enabled: bool,

    /// How long the GUI thread's message loop heartbeat may go without
    /// advancing before the watchdog considers it hung, in milliseconds.
    #[dynamic(default = "default_gui_watchdog_threshold_ms")]
    pub gui_watchdog_threshold_ms: u64,

    /// When true (the default), WebGPU frame submission (present/swapchain)
    /// runs on a dedicated per-window thread instead of the shared GUI
    /// message loop, so a stuck GPU driver call can't freeze every window in
    /// the process. Currently only implemented on Windows; ignored
    /// elsewhere.
    #[dynamic(default = "default_true")]
    pub webgpu_render_thread: bool,

    /// Debug-only: sleep this many milliseconds inside the render thread
    /// right before submit_frame, to simulate a stuck GPU driver call for
    /// testing hang-isolation behavior. 0 (default) disables the sleep.
    #[dynamic(default = "default_debug_render_thread_stall_ms")]
    pub debug_render_thread_stall_ms: u64,

    /// How long a per-window render thread's currently in-flight
    /// submit/reconfigure GPU call may run before
    /// `RenderThreadHandle::render_thread_is_hung` considers that window's
    /// render thread stuck, in milliseconds. Only meaningful when
    /// `webgpu_render_thread` is enabled.
    #[dynamic(default = "default_render_thread_hang_threshold_ms")]
    pub render_thread_hang_threshold_ms: u64,

    /// How long, in milliseconds, building the active tab's pane content
    /// for a single frame (shaping, rasterization, quad building -- see
    /// `paint_tab_content`) may run before the remainder of that frame's
    /// content is skipped in favor of reusing whatever was already drawn.
    /// This is unrelated to `render_thread_hang_threshold_ms`/
    /// `gui_watchdog_threshold_ms`, which guard the GPU submit call and the
    /// message loop respectively: this one guards the CPU-side work of
    /// building a frame's content, which today always runs synchronously
    /// on the GUI thread for the active tab and has no time limit of its
    /// own otherwise. 40ms was chosen as a middle ground between a 60Hz
    /// frame budget (~16.6ms, unrealistically tight for a hard cutoff given
    /// normal frames already legitimately exceed it under heavier-but-not-
    /// pathological content) and "still feels responsive" (perceptible
    /// input lag typically starts somewhere around 100ms) -- similar in
    /// spirit to how `render_thread_hang_threshold_ms`'s 4000ms default
    /// is picked well above any expected normal duration for the thing it
    /// guards, just at a much smaller scale here because this budget is
    /// meant to trip on every slow frame (not just a true hang) and
    /// degrade gracefully rather than declare an error. Set to 0 to
    /// disable and always build the full frame regardless of how long it
    /// takes.
    #[dynamic(default = "default_tab_frame_build_budget_ms")]
    pub tab_frame_build_budget_ms: u64,

    #[dynamic(default = "default_shape_cache_size")]
    pub shape_cache_size: usize,
    #[dynamic(default = "default_line_state_cache_size")]
    pub line_state_cache_size: usize,
    #[dynamic(default = "default_line_quad_cache_size")]
    pub line_quad_cache_size: usize,
    #[dynamic(default = "default_line_to_ele_shape_cache_size")]
    pub line_to_ele_shape_cache_size: usize,
    #[dynamic(default = "default_glyph_cache_image_cache_size")]
    pub glyph_cache_image_cache_size: usize,

    #[dynamic(default)]
    pub visual_bell: VisualBell,

    #[dynamic(default)]
    pub audible_bell: AudibleBell,

    #[dynamic(default)]
    pub canonicalize_pasted_newlines: Option<NewlineCanon>,

    #[dynamic(default = "default_unicode_version")]
    pub unicode_version: u8,

    #[dynamic(default)]
    pub treat_east_asian_ambiguous_width_as_wide: bool,

    #[dynamic(default)]
    pub cell_widths: Option<Vec<CellWidth>>,

    #[dynamic(default = "default_true")]
    pub allow_download_protocols: bool,

    #[dynamic(default = "default_true")]
    pub allow_win32_input_mode: bool,

    #[dynamic(default)]
    pub default_domain: Option<String>,

    #[dynamic(default)]
    pub default_mux_server_domain: Option<String>,

    #[dynamic(default)]
    pub default_workspace: Option<String>,

    #[dynamic(default)]
    pub xcursor_theme: Option<String>,

    #[dynamic(default)]
    pub xcursor_size: Option<u32>,

    #[dynamic(default)]
    pub key_map_preference: KeyMapPreference,

    #[dynamic(default)]
    pub quote_dropped_files: DroppedFileQuoting,

    #[dynamic(default)]
    pub ui_key_cap_rendering: UIKeyCapRendering,

    #[dynamic(default = "default_one")]
    pub palette_max_key_assigments_for_action: usize,

    #[dynamic(default = "default_ulimit_nofile")]
    pub ulimit_nofile: u64,

    #[dynamic(default = "default_ulimit_nproc")]
    pub ulimit_nproc: u64,
}

fn default_one() -> usize {
    1
}

fn default_ulimit_nofile() -> u64 {
    2048
}

fn default_ulimit_nproc() -> u64 {
    2048
}

impl Default for Config {
    fn default() -> Self {
        // Ask FromDynamic to provide the defaults based on the attributes
        // specified in the struct so that we don't have to repeat
        // the same thing in a different form down here
        Config::from_dynamic(
            &wezterm_dynamic::Value::Object(Default::default()),
            Default::default(),
        )
        .unwrap()
    }
}

impl Config {
    pub fn load() -> LoadedConfig {
        Self::load_with_overrides(&wezterm_dynamic::Value::default())
    }

    pub fn wsl_domains(&self) -> Vec<WslDomain> {
        if let Some(domains) = &self.wsl_domains {
            domains.clone()
        } else {
            WslDomain::default_domains()
        }
    }

    pub fn update_ulimit(&self) -> anyhow::Result<()> {
        #[cfg(unix)]
        {
            use nix::sys::resource::{getrlimit, rlim_t, setrlimit, Resource};
            use std::convert::TryInto;

            let (no_file_soft, no_file_hard) = getrlimit(Resource::RLIMIT_NOFILE)?;

            let ulimit_nofile: rlim_t = self.ulimit_nofile.try_into().with_context(|| {
                format!(
                    "ulimit_nofile value {} is out of range for this system",
                    self.ulimit_nofile
                )
            })?;

            if no_file_soft < ulimit_nofile {
                setrlimit(
                    Resource::RLIMIT_NOFILE,
                    ulimit_nofile.min(no_file_hard),
                    no_file_hard,
                )
                .with_context(|| {
                    format!(
                        "raise RLIMIT_NOFILE from {no_file_soft} to ulimit_nofile {}",
                        ulimit_nofile
                    )
                })?;
            }
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use nix::sys::resource::{getrlimit, rlim_t, setrlimit, Resource};
            use std::convert::TryInto;

            let (nproc_soft, nproc_hard) = getrlimit(Resource::RLIMIT_NPROC)?;

            let ulimit_nproc: rlim_t = self.ulimit_nproc.try_into().with_context(|| {
                format!(
                    "ulimit_nproc value {} is out of range for this system",
                    self.ulimit_nproc
                )
            })?;

            if nproc_soft < ulimit_nproc {
                setrlimit(
                    Resource::RLIMIT_NPROC,
                    ulimit_nproc.min(nproc_hard),
                    nproc_hard,
                )
                .with_context(|| {
                    format!(
                        "raise RLIMIT_NPROC from {nproc_soft} to ulimit_nproc {}",
                        ulimit_nproc
                    )
                })?;
            }
        }

        Ok(())
    }

    pub fn load_with_overrides(overrides: &wezterm_dynamic::Value) -> LoadedConfig {
        // Note that the directories crate has methods for locating project
        // specific config directories, but only returns one of them, not
        // multiple.  In addition, it spawns a lot of subprocesses,
        // so we do this bit "by-hand"

        let mut paths = vec![PathPossibility::optional(HOME_DIR.join(".onlyterm.rhai"))];
        for dir in CONFIG_DIRS.iter() {
            paths.push(PathPossibility::optional(dir.join("onlyterm.rhai")))
        }

        if cfg!(windows) {
            // On Windows, a common use case is to maintain a thumb drive
            // with a set of portable tools that don't need to be installed
            // to run on a target system.  In that scenario, the user would
            // like to run with the config from their thumbdrive because
            // either the target system won't have any config, or will have
            // the config of another user.
            // So we prioritize that here: if there is a config in the same
            // dir as the executable that will take precedence.
            if let Ok(exe_name) = std::env::current_exe() {
                if let Some(exe_dir) = exe_name.parent() {
                    paths.insert(0, PathPossibility::optional(exe_dir.join("onlyterm.rhai")));
                }
            }
        }
        if let Some(path) = std::env::var_os("ONLYTERM_CONFIG_FILE") {
            log::trace!("Note: ONLYTERM_CONFIG_FILE is set in the environment");
            paths.insert(0, PathPossibility::required(path.into()));
        }

        if let Some(path) = CONFIG_FILE_OVERRIDE.lock().unwrap().as_ref() {
            log::trace!("Note: config file override is set");
            paths.insert(0, PathPossibility::required(path.clone()));
        }

        for path_item in &paths {
            if CONFIG_SKIP.load(Ordering::Relaxed) {
                break;
            }

            match Self::try_load(path_item, overrides) {
                Err(err) => {
                    return LoadedConfig {
                        config: Err(err),
                        file_name: Some(path_item.path.clone()),
                        event_script: None,
                        warnings: vec![],
                        rhai_watch_paths: vec![],
                    }
                }
                Ok(None) => continue,
                Ok(Some(loaded)) => return loaded,
            }
        }

        // We didn't find (or were asked to skip) a onlyterm.rhai file, so
        // update the environment to make it simpler to understand this
        // state.
        std::env::remove_var("ONLYTERM_CONFIG_FILE");
        std::env::remove_var("ONLYTERM_CONFIG_DIR");

        match Self::try_default() {
            Err(err) => LoadedConfig {
                config: Err(err),
                file_name: None,
                event_script: None,
                warnings: vec![],
                rhai_watch_paths: vec![],
            },
            Ok(cfg) => cfg,
        }
    }

    pub fn try_default() -> anyhow::Result<LoadedConfig> {
        let (config, warnings) =
            wezterm_dynamic::Error::capture_warnings(|| -> anyhow::Result<Config> {
                Ok(default_config_with_overrides_applied()?.compute_extra_defaults(None))
            });

        Ok(LoadedConfig {
            config: Ok(config?),
            file_name: None,
            // No config file: there is no script source to give the
            // event-callback bridge an `on(...)` handler to run against, but we
            // still hand back a (handler-less) `RhaiEventScript` descriptor so
            // that `config::with_rhai_config_on_main_thread`/
            // `run_immediate_with_rhai_config` always have *something* to build
            // a `RhaiConfigState` from (mirroring the pre-L4.6 companion mlua
            // context, which likewise always existed even with no config file).
            event_script: Some(rhai_engine::RhaiEventScript::for_default()),
            warnings,
            rhai_watch_paths: vec![],
        })
    }

    /// If `p` (a `.rhai` candidate path) doesn't exist, but a legacy
    /// `.lua`-suffixed sibling does, produce a clear, actionable error
    /// explaining that Lua configs are no longer supported at runtime:
    /// mlua has been retired from the live config-loading path and users
    /// must migrate their config to rhai syntax and rename the file.
    ///
    /// This is purely a diagnostic: we never parse the `.lua` file with
    /// mlua here, we only check for its existence on disk so that the
    /// error message can point the user at the specific file that needs
    /// migrating instead of a generic "file not found".
    fn legacy_lua_sibling(p: &Path) -> Option<PathBuf> {
        let file_name = p.file_name()?.to_str()?;
        let lua_name = if file_name == "onlyterm.rhai" {
            "onlyterm.lua".to_string()
        } else if file_name == ".onlyterm.rhai" {
            ".onlyterm.lua".to_string()
        } else if let Some(stripped) = file_name.strip_suffix(".rhai") {
            format!("{stripped}.lua")
        } else {
            return None;
        };

        let lua_path = p.with_file_name(lua_name);
        if lua_path.is_file() {
            Some(lua_path)
        } else {
            None
        }
    }

    fn try_load(
        path_item: &PathPossibility,
        overrides: &wezterm_dynamic::Value,
    ) -> anyhow::Result<Option<LoadedConfig>> {
        let p = path_item.path.as_path();
        log::trace!("consider config: {}", p.display());
        let mut file = match std::fs::File::open(p) {
            Ok(file) => file,
            Err(err) => match err.kind() {
                std::io::ErrorKind::NotFound if !path_item.is_required => {
                    if let Some(lua_path) = Self::legacy_lua_sibling(p) {
                        anyhow::bail!(
                            "Found a legacy Lua configuration file at {} but Lua \
                             configs are no longer supported: mlua has been removed \
                             from wezterm's live config-loading path. Please rename \
                             {} to {} and adapt its syntax to rhai. See the migration \
                             guide for details on translating a onlyterm.lua config to \
                             onlyterm.rhai.",
                            lua_path.display(),
                            lua_path.display(),
                            p.display()
                        );
                    }
                    return Ok(None);
                }
                _ => anyhow::bail!("Error opening {}: {}", p.display(), err),
            },
        };

        let mut s = String::new();
        file.read_to_string(&mut s)?;
        let rhai_engine = make_rhai_engine(p)?;

        // Skip a potential BOM that Windows software may have placed in the
        // file. Stripped once up front (rather than inside the closure below)
        // so that `script` is also available afterwards to build the
        // `RhaiEventScript` event-callback descriptor.
        let script = s.trim_start_matches('\u{FEFF}');

        let (config, warnings) =
            wezterm_dynamic::Error::capture_warnings(|| -> anyhow::Result<Config> {
                let cfg: Config;

                let (_ast, config_value) = rhai_engine.compile_and_eval(script).map_err(|e| {
                    anyhow::anyhow!("Error evaluating {}: {}", p.display(), e)
                })?;

                let config_value =
                    Config::apply_overrides_to_rhai(&rhai_engine, config_value)?;
                let config_value =
                    Config::apply_overrides_obj_to_rhai(config_value, overrides)?;

                cfg = Config::from_rhai_dynamic(config_value).with_context(|| {
                    format!(
                        "Error converting rhai value returned by script {} to Config struct",
                        p.display()
                    )
                })?;
                cfg.check_consistency()?;

                // Compute but discard the key bindings here so that we raise any
                // problems earlier than we use them.
                let _ = cfg.key_bindings();

                std::env::set_var("ONLYTERM_CONFIG_FILE", p);
                if let Some(dir) = p.parent() {
                    std::env::set_var("ONLYTERM_CONFIG_DIR", dir);
                }
                Ok(cfg)
            });
        let cfg = config?;

        // Grab any paths that the .rhai script added to its reload watch
        // list (via `add_to_config_reload_watch_list`) before the engine
        // that owns them goes out of scope; see `LoadedConfig::rhai_watch_paths`.
        let rhai_watch_paths = rhai_engine.watch_list.paths();

        // Event-callback bridge descriptor (see `RhaiEventScript`'s doc comment
        // in `config/src/rhai_engine.rs`): carries the script's own source text
        // (the same `script` string just parsed above, BOM already stripped) so
        // that whichever thread ends up building the live `RhaiConfigState` (the
        // main thread, via `RhaiPipe`/`with_rhai_config_on_main_thread` in
        // `config/src/lib.rs`) re-evaluates the *actual* config script rather
        // than an empty stand-in. Any top-level `on(...)` calls the user's
        // script makes are therefore registered for real, unlike the pre-L4.6
        // companion mlua context (which was built from `p` but never executed
        // the script's text at all).
        let event_script = rhai_engine::RhaiEventScript::for_script(script.to_string(), p.to_path_buf());

        Ok(Some(LoadedConfig {
            config: Ok(cfg.compute_extra_defaults(Some(p))),
            file_name: Some(p.to_path_buf()),
            event_script: Some(event_script),
            warnings,
            rhai_watch_paths,
        }))
    }

    /// Convert a `rhai::Dynamic` (the result of evaluating a `.rhai` config
    /// script) into a `Config`, in the same "strict: deny unknown fields"
    /// mode that the mlua config-builder path enforced via
    /// `config_builder_new_index` (see `config/src/lua.rs`).
    fn from_rhai_dynamic(value: rhai::Dynamic) -> anyhow::Result<Config> {
        let dyn_value = crate::rhai_value::rhai_dynamic_to_dynamic(&value)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Config::from_dynamic(
            &dyn_value,
            wezterm_dynamic::FromDynamicOptions {
                unknown_fields: wezterm_dynamic::UnknownFieldAction::Deny,
                deprecated_fields: wezterm_dynamic::UnknownFieldAction::Warn,
            },
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// rhai analogue of `apply_overrides_obj_to`: apply an overrides object
    /// (as used by `overridden_config`/palette previews) directly onto the
    /// value returned by the config script, without needing a running
    /// engine (the values are plain data, no callbacks are involved).
    pub(crate) fn apply_overrides_obj_to_rhai(
        mut config: rhai::Dynamic,
        overrides: &wezterm_dynamic::Value,
    ) -> anyhow::Result<rhai::Dynamic> {
        match overrides {
            wezterm_dynamic::Value::Object(obj) => {
                if obj.is_empty() {
                    return Ok(config);
                }
                let mut map = config.try_cast::<rhai::Map>().ok_or_else(|| {
                    anyhow::anyhow!(
                        "expected the config script to evaluate to an object/map, \
                         so that overrides could be applied"
                    )
                })?;
                for (key, value) in obj {
                    let key = match key {
                        wezterm_dynamic::Value::String(s) => s.clone(),
                        other => format!("{other:?}"),
                    };
                    let value = crate::rhai_value::dynamic_to_rhai_dynamic(value);
                    map.insert(key.into(), value);
                }
                config = rhai::Dynamic::from_map(map);
                Ok(config)
            }
            _ => Ok(config),
        }
    }

    /// rhai analogue of `apply_overrides_to`: apply the `--config key=value`
    /// command line overrides by evaluating each `value` as a rhai
    /// expression against `rhai_engine`, then setting `config[key] = value`.
    pub(crate) fn apply_overrides_to_rhai(
        rhai_engine: &RhaiConfigEngine,
        mut config: rhai::Dynamic,
    ) -> anyhow::Result<rhai::Dynamic> {
        let overrides = CONFIG_OVERRIDES.lock().unwrap();
        for (key, value) in &*overrides {
            if value == "nil" || value == "()" {
                // Literal nil/unit as the value is the same as not specifying
                // the value.  We special case this here as we want to
                // explicitly check for the value evaluating as unit, as can
                // happen in the case where the user specifies something like:
                // `--config term=xterm`.
                // The RHS references a global that doesn't exist and
                // evaluates as unit. We want to raise this as an error.
                continue;
            }

            let evaluated = rhai_engine.eval(value).map_err(|e| {
                anyhow::anyhow!("--config {}={}: error evaluating value: {}", key, value, e)
            })?;
            if evaluated.is_unit() {
                anyhow::bail!(
                    "--config {}={}: value evaluated as (). Check for missing \
                     quotes or other syntax issues",
                    key,
                    value
                );
            }

            let mut map = config.try_cast::<rhai::Map>().ok_or_else(|| {
                anyhow::anyhow!(
                    "expected the config script to evaluate to an object/map, \
                     so that --config {}={} could be applied",
                    key,
                    value
                )
            })?;
            log::debug!("Apply {}={} to config", key, value);
            map.insert(key.as_str().into(), evaluated);
            config = rhai::Dynamic::from_map(map);
        }
        Ok(config)
    }

    /// Check for logical conflicts in the config
    pub fn check_consistency(&self) -> anyhow::Result<()> {
        self.check_domain_consistency()?;
        Ok(())
    }

    fn check_domain_consistency(&self) -> anyhow::Result<()> {
        let mut domains = HashMap::new();

        let mut check_domain = |name: &str, kind: &str| {
            if let Some(exists) = domains.get(name) {
                anyhow::bail!(
                    "{kind} with name \"{name}\" conflicts with \
                     another existing {exists} with the same name"
                );
            }
            domains.insert(name.to_string(), kind.to_string());
            Ok(())
        };

        for d in &self.unix_domains {
            check_domain(&d.name, "unix domain")?;
        }
        for d in &self.exec_domains {
            check_domain(&d.name, "exec domain")?;
        }
        if let Some(domains) = &self.wsl_domains {
            for d in domains {
                check_domain(&d.name, "wsl domain")?;
            }
        }
        Ok(())
    }

    pub fn default_config() -> Self {
        Self::default().compute_extra_defaults(None)
    }

    pub fn key_bindings(&self) -> KeyTables {
        let mut tables = KeyTables::default();

        for k in &self.keys {
            let (key, mods) = k
                .key
                .key
                .resolve(self.key_map_preference)
                .normalize_shift(k.key.mods);
            tables.default.insert(
                (key, mods),
                KeyTableEntry {
                    action: k.action.clone(),
                },
            );
        }

        for (name, keys) in &self.key_tables {
            let mut table = KeyTable::default();
            for k in keys {
                let (key, mods) = k
                    .key
                    .key
                    .resolve(self.key_map_preference)
                    .normalize_shift(k.key.mods);
                table.insert(
                    (key, mods),
                    KeyTableEntry {
                        action: k.action.clone(),
                    },
                );
            }
            tables.by_name.insert(name.to_string(), table);
        }

        tables
    }

    pub fn mouse_bindings(
        &self,
    ) -> HashMap<(MouseEventTrigger, MouseEventTriggerMods), KeyAssignment> {
        let mut map = HashMap::new();

        for m in &self.mouse_bindings {
            map.insert((m.event.clone(), m.mods), m.action.clone());
        }

        map
    }

    /// In some cases we need to compute expanded values based
    /// on those provided by the user.  This is where we do that.
    pub fn compute_extra_defaults(&self, config_path: Option<&Path>) -> Self {
        let mut cfg = self.clone();

        // Convert any relative font dirs to their config file relative locations
        if let Some(config_dir) = config_path.as_ref().and_then(|p| p.parent()) {
            for font_dir in &mut cfg.font_dirs {
                if !font_dir.is_absolute() {
                    let dir = config_dir.join(&font_dir);
                    *font_dir = dir;
                }
            }

            if let Some(path) = &self.window_background_image {
                if !path.is_absolute() {
                    cfg.window_background_image.replace(config_dir.join(path));
                }
            }
        }

        // Add some reasonable default font rules
        let reduced = self.font.reduce_first_font_to_family();

        let italic = reduced.make_italic();

        let bold = reduced.make_bold();
        let bold_italic = bold.make_italic();

        let half_bright = reduced.make_half_bright();
        let half_bright_italic = half_bright.make_italic();

        cfg.font_rules.push(StyleRule {
            italic: Some(true),
            intensity: Some(wezterm_term::Intensity::Half),
            font: half_bright_italic,
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(false),
            intensity: Some(wezterm_term::Intensity::Half),
            font: half_bright,
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(false),
            intensity: Some(wezterm_term::Intensity::Bold),
            font: bold,
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(true),
            intensity: Some(wezterm_term::Intensity::Bold),
            font: bold_italic,
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(true),
            intensity: Some(wezterm_term::Intensity::Normal),
            font: italic,
            ..Default::default()
        });

        // Load any additional color schemes into the color_schemes map
        cfg.load_color_schemes(&cfg.compute_color_scheme_dirs())
            .ok();

        if let Some(scheme) = cfg.color_scheme.as_ref() {
            match cfg.resolve_color_scheme() {
                None => {
                    log::error!(
                        "Your configuration specifies color_scheme=\"{}\" \
                        but that scheme was not found",
                        scheme
                    );
                }
                Some(p) => {
                    cfg.resolved_palette = p.clone();
                }
            }
        }

        if let Some(colors) = &cfg.colors {
            cfg.resolved_palette = cfg.resolved_palette.overlay_with(colors);
        }

        if let Some(bg) = BackgroundLayer::with_legacy(self) {
            cfg.background.insert(0, bg);
        }

        cfg
    }

    fn compute_color_scheme_dirs(&self) -> Vec<PathBuf> {
        let mut paths = self.color_scheme_dirs.clone();
        for dir in CONFIG_DIRS.iter() {
            paths.push(dir.join("colors"));
        }
        if cfg!(windows) {
            // See commentary re: portable tools above!
            if let Ok(exe_name) = std::env::current_exe() {
                if let Some(exe_dir) = exe_name.parent() {
                    paths.insert(0, exe_dir.join("colors"));
                }
            }
        }
        paths
    }

    fn load_color_schemes(&mut self, paths: &[PathBuf]) -> anyhow::Result<()> {
        fn extract_scheme_name(name: &str) -> Option<&str> {
            if name.ends_with(".toml") {
                let len = name.len();
                Some(&name[..len - 5])
            } else {
                None
            }
        }

        fn load_scheme(path: &Path) -> anyhow::Result<ColorSchemeFile> {
            let s = std::fs::read_to_string(path)?;
            ColorSchemeFile::from_toml_str(&s).context("parsing TOML")
        }

        for colors_dir in paths {
            if let Ok(dir) = std::fs::read_dir(colors_dir) {
                for entry in dir {
                    if let Ok(entry) = entry {
                        if let Some(name) = entry.file_name().to_str() {
                            if let Some(scheme_name) = extract_scheme_name(name) {
                                if self.color_schemes.contains_key(scheme_name) {
                                    // This scheme has already been defined
                                    continue;
                                }

                                let path = entry.path();
                                match load_scheme(&path) {
                                    Ok(scheme) => {
                                        let name = scheme
                                            .metadata
                                            .name
                                            .unwrap_or_else(|| scheme_name.to_string());
                                        log::trace!(
                                            "Loaded color scheme `{}` from {}",
                                            name,
                                            path.display()
                                        );
                                        self.color_schemes.insert(name, scheme.colors);
                                    }
                                    Err(err) => {
                                        log::error!(
                                            "Color scheme in `{}` failed to load: {:#}",
                                            path.display(),
                                            err
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn resolve_color_scheme(&self) -> Option<&Palette> {
        let scheme_name = self.color_scheme.as_ref()?;

        if let Some(palette) = self.color_schemes.get(scheme_name) {
            Some(palette)
        } else {
            crate::COLOR_SCHEMES.get(scheme_name)
        }
    }

    pub fn initial_size(&self, dpi: u32, cell_pixel_dims: Option<(usize, usize)>) -> TerminalSize {
        // If we aren't passed the actual values, guess at a plausible
        // default set of pixel dimensions.
        // This is based on "typical" 10 point font at "normal"
        // pixel density.
        // This will get filled in by the gui layer, but there is
        // an edge case where we emit an iTerm image escape in
        // the software update banner through the mux layer before
        // the GUI has had a chance to update the pixel dimensions
        // when running under X11.
        // This is a bit gross.
        let (cell_pixel_width, cell_pixel_height) = cell_pixel_dims.unwrap_or((8, 16));

        TerminalSize {
            rows: self.initial_rows as usize,
            cols: self.initial_cols as usize,
            pixel_width: cell_pixel_width * self.initial_cols as usize,
            pixel_height: cell_pixel_height * self.initial_rows as usize,
            dpi,
        }
    }

    pub fn build_prog(
        &self,
        prog: Option<Vec<&OsStr>>,
        default_prog: Option<&Vec<String>>,
        default_cwd: Option<&PathBuf>,
    ) -> anyhow::Result<CommandBuilder> {
        let mut cmd = match prog {
            Some(args) => {
                let mut args = args.iter();
                let mut cmd = CommandBuilder::new(args.next().expect("executable name"));
                cmd.args(args);
                cmd
            }
            None => {
                if let Some(prog) = default_prog {
                    let mut args = prog.iter();
                    let mut cmd = CommandBuilder::new(args.next().expect("executable name"));
                    cmd.args(args);
                    cmd
                } else {
                    CommandBuilder::new_default_prog()
                }
            }
        };

        self.apply_cmd_defaults(&mut cmd, None, default_cwd);

        Ok(cmd)
    }

    pub fn apply_cmd_defaults(
        &self,
        cmd: &mut CommandBuilder,
        default_prog: Option<&Vec<String>>,
        default_cwd: Option<&PathBuf>,
    ) {
        // Apply `default_cwd` only if `cwd` is not already set, allows `--cwd`
        // option to take precedence
        if let (None, Some(cwd)) = (cmd.get_cwd(), default_cwd) {
            cmd.cwd(cwd);
        }

        if let Some(default_prog) = default_prog {
            if cmd.is_default_prog() {
                cmd.replace_default_prog(default_prog);
            }
        }

        // Augment WSLENV so that TERM related environment propagates
        // across the win32/wsl boundary
        let mut wsl_env = std::env::var("WSLENV").ok();

        // If we are running as an appimage, we will have "$APPIMAGE"
        // and "$APPDIR" set in the wezterm process. These will be
        // propagated to the child processes. Since some apps (including
        // wezterm) use these variables to detect if they are running in
        // an appimage, those child processes will be misconfigured.
        // Ensure that they are unset.
        // https://docs.appimage.org/packaging-guide/environment-variables.html#id2
        cmd.env_remove("APPIMAGE");
        cmd.env_remove("APPDIR");
        cmd.env_remove("OWD");

        for (k, v) in &self.set_environment_variables {
            if k == "WSLENV" {
                wsl_env.replace(v.clone());
            } else {
                cmd.env(k, v);
            }
        }

        if wsl_env.is_some() || cfg!(windows) || crate::version::running_under_wsl() {
            let mut wsl_env = wsl_env.unwrap_or_default();
            if !wsl_env.is_empty() {
                wsl_env.push(':');
            }
            wsl_env.push_str("TERM:COLORTERM:TERM_PROGRAM:TERM_PROGRAM_VERSION");
            cmd.env("WSLENV", wsl_env);
        }

        #[cfg(unix)]
        cmd.umask(umask::UmaskSaver::saved_umask());
        cmd.env("TERM", &self.term);
        cmd.env("COLORTERM", "truecolor");
        // TERM_PROGRAM and TERM_PROGRAM_VERSION are an emerging
        // de-facto standard for identifying the terminal.
        cmd.env("TERM_PROGRAM", "WezTerm");
        cmd.env("TERM_PROGRAM_VERSION", crate::wezterm_version());
    }
}

fn default_check_for_updates() -> bool {
    cfg!(not(feature = "distro-defaults"))
}

fn default_pane_select_fg_color() -> RgbaColor {
    SrgbaTuple(0.75, 0.75, 0.75, 1.0).into()
}

fn default_pane_select_bg_color() -> RgbaColor {
    SrgbaTuple(0., 0., 0., 0.5).into()
}

fn default_pane_select_font_size() -> f64 {
    36.0
}

fn default_integrated_title_buttons() -> Vec<IntegratedTitleButton> {
    use IntegratedTitleButton::*;
    vec![Hide, Maximize, Close]
}

fn default_char_select_font_size() -> f64 {
    18.0
}

fn default_char_select_fg_color() -> RgbaColor {
    SrgbaTuple(0.75, 0.75, 0.75, 1.0).into()
}

fn default_char_select_bg_color() -> RgbaColor {
    (0x33, 0x33, 0x33).into()
}

fn default_command_palette_font_size() -> f64 {
    14.0
}

fn default_command_palette_fg_color() -> RgbaColor {
    SrgbaTuple(0.75, 0.75, 0.75, 1.0).into()
}

fn default_command_palette_bg_color() -> RgbaColor {
    (0x33, 0x33, 0x33).into()
}

fn default_swallow_mouse_click_on_window_focus() -> bool {
    cfg!(target_os = "macos")
}

fn default_mux_output_parser_coalesce_delay_ms() -> u64 {
    3
}

fn default_mux_synchronized_output_timeout_ms() -> u64 {
    1000
}

fn default_mux_output_parser_buffer_size() -> usize {
    128 * 1024
}

fn default_mux_output_parser_chunk_size() -> usize {
    // Measured on a representative mixed print/CSI/control workload
    // (crates/term's perf_probe tests, task #147): CSI dispatch averages
    // roughly 12us/action in release builds, so 256 actions bounds a
    // single chunk's lock hold time to a few milliseconds even for a
    // CSI-heavy stream, while still being large enough that per-chunk
    // fixed costs (locking, Vec<Action> chunk allocation) stay
    // negligible relative to the work done per chunk.
    256
}

fn default_ratelimit_line_prefetches_per_second() -> u32 {
    50
}

fn default_cursor_blink_rate() -> u64 {
    800
}

fn default_text_blink_rate() -> u64 {
    500
}

fn default_text_blink_rate_rapid() -> u64 {
    250
}

fn default_swap_backspace_and_delete() -> bool {
    // cfg!(target_os = "macos")
    // See: https://github.com/wezterm/wezterm/issues/88
    false
}

fn default_scrollback_lines() -> usize {
    3500
}

const MAX_SCROLLBACK_LINES: usize = 999_999_999;
fn validate_scrollback_lines(value: &usize) -> Result<(), String> {
    if *value > MAX_SCROLLBACK_LINES {
        return Err(format!(
            "Illegal value {value} for scrollback_lines; it must be <= {MAX_SCROLLBACK_LINES}!"
        ));
    }
    Ok(())
}

fn default_initial_rows() -> u16 {
    24
}

fn default_initial_cols() -> u16 {
    80
}

pub fn default_hyperlink_rules() -> Vec<hyperlink::Rule> {
    vec![
        // First handle URLs wrapped with punctuation (i.e. brackets)
        // e.g. [http://foo] (http://foo) <http://foo>
        hyperlink::Rule::with_highlight(r"\((\w+://\S+)\)", "$1", 1).unwrap(),
        hyperlink::Rule::with_highlight(r"\[(\w+://\S+)\]", "$1", 1).unwrap(),
        hyperlink::Rule::with_highlight(r"<(\w+://\S+)>", "$1", 1).unwrap(),
        // Then handle URLs not wrapped in brackets that
        // 1) have a balanced ending parenthesis or
        hyperlink::Rule::new(hyperlink::CLOSING_PARENTHESIS_HYPERLINK_PATTERN, "$0").unwrap(),
        // 2) include terminating _, / or - characters, if any
        hyperlink::Rule::new(hyperlink::GENERIC_HYPERLINK_PATTERN, "$0").unwrap(),
        // implicit mailto link
        hyperlink::Rule::new(r"\b\w+@[\w-]+(\.[\w-]+)+\b", "mailto:$0").unwrap(),
    ]
}

fn default_harfbuzz_features() -> Vec<String> {
    ["kern", "liga", "clig"]
        .iter()
        .map(|&s| s.to_string())
        .collect()
}

fn default_term() -> String {
    "xterm-256color".into()
}

fn default_font_size() -> f64 {
    12.0
}

pub(crate) fn compute_cache_dir() -> anyhow::Result<PathBuf> {
    if let Some(runtime) = dirs_next::cache_dir() {
        return Ok(runtime.join("onlyterm"));
    }

    Ok(crate::HOME_DIR.join(".local/share/onlyterm"))
}

pub(crate) fn compute_data_dir() -> anyhow::Result<PathBuf> {
    if let Some(runtime) = dirs_next::data_dir() {
        return Ok(runtime.join("onlyterm"));
    }

    Ok(crate::HOME_DIR.join(".local/share/onlyterm"))
}

pub(crate) fn compute_runtime_dir() -> anyhow::Result<PathBuf> {
    if let Some(runtime) = dirs_next::runtime_dir() {
        return Ok(runtime.join("onlyterm"));
    }

    Ok(crate::HOME_DIR.join(".local/share/onlyterm"))
}

pub fn username_from_env() -> anyhow::Result<String> {
    #[cfg(unix)]
    const USER: &str = "USER";
    #[cfg(windows)]
    const USER: &str = "USERNAME";

    std::env::var(USER).with_context(|| format!("while resolving {} env var", USER))
}

pub fn default_read_timeout() -> Duration {
    Duration::from_secs(60)
}

pub fn default_write_timeout() -> Duration {
    Duration::from_secs(60)
}

pub fn default_local_echo_threshold_ms() -> Option<u64> {
    Some(100)
}

fn rgba(hex: &str) -> RgbaColor {
    <RgbaColor as std::convert::TryFrom<String>>::try_from(hex.to_string())
        .expect("built-in default color literal must be valid")
}

/// OnlyTerm defaults to a light, GitHub-style palette rather than
/// upstream wezterm's unset (effectively dark) palette.
fn default_colors() -> Option<Palette> {
    let tab_bar_color = |bg: &str, fg: &str| TabBarColor {
        bg_color: rgba(bg),
        fg_color: rgba(fg),
        ..Default::default()
    };

    Some(Palette {
        foreground: Some(rgba("#1f2328")),
        background: Some(rgba("#ffffff")),
        cursor_fg: Some(rgba("#ffffff")),
        cursor_bg: Some(rgba("#1f2328")),
        cursor_border: Some(rgba("#1f2328")),
        selection_fg: Some(rgba("#1f2328")),
        selection_bg: Some(rgba("#d0d7de")),
        ansi: Some([
            rgba("#f6f8fa"),
            rgba("#cf222e"),
            rgba("#116329"),
            rgba("#4d2d00"),
            rgba("#0969da"),
            rgba("#8250df"),
            rgba("#1b7c83"),
            rgba("#f6f8fa"),
        ]),
        brights: Some([
            rgba("#24292f"),
            rgba("#a40e26"),
            rgba("#1a7f37"),
            rgba("#633c01"),
            rgba("#0550ae"),
            rgba("#6f42c1"),
            rgba("#3192aa"),
            rgba("#ffffff"),
        ]),
        scrollbar_thumb: Some(rgba("#b6b6b6")),
        tab_bar: Some(TabBarColors {
            background: Some(rgba("#e8edf2")),
            active_tab: Some(tab_bar_color("#ffffff", "#1f2328")),
            inactive_tab: Some(tab_bar_color("#d0d7de", "#57606a")),
            inactive_tab_hover: Some(tab_bar_color("#c8d1da", "#24292f")),
            new_tab: Some(tab_bar_color("#e8edf2", "#57606a")),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// OnlyTerm is Windows-focused: default new panes/tabs to `cmd.exe` rather
/// than whatever the ambient `ComSpec` environment variable happens to hold
/// (which is sometimes PowerShell, depending on how the shell was launched).
/// Explicitly configuring `default_prog` makes this deterministic instead of
/// relying on `CommandBuilder::new_default_prog()`'s `ComSpec` fallback.
#[cfg(windows)]
fn windows_default_prog() -> Option<Vec<String>> {
    Some(vec!["cmd.exe".to_string()])
}

#[cfg(not(windows))]
fn windows_default_prog() -> Option<Vec<String>> {
    None
}

fn default_bidi_direction() -> ParagraphDirectionHint {
    ParagraphDirectionHint::AutoLeftToRight
}

fn default_bypass_mouse_reporting_modifiers() -> Modifiers {
    Modifiers::SHIFT
}

fn default_gui_startup_args() -> Vec<String> {
    vec!["start".to_string()]
}

// Coupled with term/src/config.rs:TerminalConfiguration::unicode_version
fn default_unicode_version() -> u8 {
    9
}

fn default_mux_env_remove() -> Vec<String> {
    vec![
        "SSH_AUTH_SOCK".to_string(),
        "SSH_CLIENT".to_string(),
        "SSH_CONNECTION".to_string(),
    ]
}

fn default_anim_fps() -> u8 {
    10
}

fn default_max_fps() -> u64 {
    60
}

fn default_gui_watchdog_threshold_ms() -> u64 {
    4_000
}

#[cfg(windows)]
fn default_front_end() -> FrontEndSelection {
    FrontEndSelection::WebGpu
}

#[cfg(not(windows))]
fn default_front_end() -> FrontEndSelection {
    FrontEndSelection::default()
}

#[cfg(test)]
mod default_front_end_test {
    use super::*;

    /// WebGpu (with its OpenGL fallback and dedicated render thread) is only
    /// the default on Windows; other platforms keep their historical
    /// `FrontEndSelection::default()` (`OpenGL`), per the
    /// execution-decoupling plan (task 221.8).
    #[test]
    fn matches_platform_expectation() {
        #[cfg(windows)]
        assert_eq!(default_front_end(), FrontEndSelection::WebGpu);

        #[cfg(not(windows))]
        assert_eq!(default_front_end(), FrontEndSelection::default());
    }
}

fn default_debug_render_thread_stall_ms() -> u64 {
    0
}

fn default_render_thread_hang_threshold_ms() -> u64 {
    4_000
}

fn default_tab_frame_build_budget_ms() -> u64 {
    40
}

fn default_tiling_desktop_environments() -> Vec<String> {
    [
        "X11 LG3D",
        "X11 Qtile",
        "X11 awesome",
        "X11 bspwm",
        "X11 dwm",
        "X11 i3",
        "X11 xmonad",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_stateless_process_list() -> Vec<String> {
    [
        "bash",
        "sh",
        "zsh",
        "fish",
        "tmux",
        "nu",
        "nu.exe",
        "cmd.exe",
        "pwsh.exe",
        "powershell.exe",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_status_update_interval() -> u64 {
    1_000
}

fn default_alternate_buffer_wheel_scroll_speed() -> u8 {
    3
}

fn default_num_alphabet() -> String {
    // Note: vi motion keys are intentionally excluded from this alphabet
    "1234567890abcdefghilmnopqrstuvwxyz".to_string()
}

fn default_alphabet() -> String {
    "asdfqwerzxcvjklmiuopghtybn".to_string()
}

fn default_word_boundary() -> String {
    " \t\n{[}]()\"'`".to_string()
}

fn default_enq_answerback() -> String {
    "".to_string()
}

fn default_tab_max_width() -> usize {
    16
}

fn default_update_interval() -> u64 {
    86400
}

fn default_prefer_egl() -> bool {
    !cfg!(windows)
}

fn default_clean_exits() -> Vec<u32> {
    vec![]
}

fn default_inactive_pane_hsb() -> HsbTransform {
    HsbTransform {
        brightness: 0.8,
        saturation: 0.9,
        hue: 1.0,
    }
}

#[derive(FromDynamic, ToDynamic, Clone, Copy, Debug, Default)]
pub enum DefaultCursorStyle {
    BlinkingBlock,
    #[default]
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

impl DefaultCursorStyle {
    pub fn effective_shape(self, shape: CursorShape) -> CursorShape {
        match shape {
            CursorShape::Default => match self {
                Self::BlinkingBlock => CursorShape::BlinkingBlock,
                Self::SteadyBlock => CursorShape::SteadyBlock,
                Self::BlinkingUnderline => CursorShape::BlinkingUnderline,
                Self::SteadyUnderline => CursorShape::SteadyUnderline,
                Self::BlinkingBar => CursorShape::BlinkingBar,
                Self::SteadyBar => CursorShape::SteadyBar,
            },
            _ => shape,
        }
    }
}

const fn linear_ease() -> EasingFunction {
    EasingFunction::Linear
}

const fn default_one_cell() -> Dimension {
    Dimension::Cells(1.)
}

const fn default_half_cell() -> Dimension {
    Dimension::Cells(0.5)
}

/// The scrollbar thumb should never shrink to the point of being hard to
/// grab or see, even on very long scrollback buffers.
const fn default_min_scroll_bar_height() -> Dimension {
    Dimension::Cells(2.0)
}

const fn default_reverse_video_cursor_min_contrast() -> f32 {
    2.5
}

#[derive(FromDynamic, ToDynamic, Clone, Copy, Debug)]
pub struct WindowPadding {
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_one_cell")]
    pub left: Dimension,
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_half_cell")]
    pub top: Dimension,
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_one_cell")]
    pub right: Dimension,
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_half_cell")]
    pub bottom: Dimension,
}

impl Default for WindowPadding {
    fn default() -> Self {
        Self {
            left: default_one_cell(),
            right: default_one_cell(),
            top: default_half_cell(),
            bottom: default_half_cell(),
        }
    }
}

#[derive(FromDynamic, ToDynamic, Clone, Copy, Debug, Default)]
pub struct WindowContentAlignment {
    pub horizontal: HorizontalWindowContentAlignment,
    pub vertical: VerticalWindowContentAlignment,
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum HorizontalWindowContentAlignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerticalWindowContentAlignment {
    #[default]
    Top,
    Center,
    Bottom,
}

#[derive(FromDynamic, ToDynamic, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewlineCanon {
    // FIXME: also allow deserialziing from bool
    None,
    LineFeed,
    CarriageReturn,
    CarriageReturnAndLineFeed,
}

#[derive(FromDynamic, ToDynamic, Clone, Copy, Debug, Default)]
pub enum WindowCloseConfirmation {
    AlwaysPrompt,
    // OnlyTerm defaults to never prompting on close (upstream wezterm
    // defaults to AlwaysPrompt).
    #[default]
    NeverPrompt,
    // TODO: something smart where we see whether the
    // running programs are stateful
}

struct PathPossibility {
    path: PathBuf,
    is_required: bool,
}
impl PathPossibility {
    pub fn required(path: PathBuf) -> PathPossibility {
        PathPossibility {
            path,
            is_required: true,
        }
    }
    pub fn optional(path: PathBuf) -> PathPossibility {
        PathPossibility {
            path,
            is_required: false,
        }
    }
}

/// Behavior when the program spawned by wezterm terminates
#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExitBehavior {
    /// Close the associated pane
    #[default]
    Close,
    /// Close the associated pane if the process was successful
    CloseOnCleanExit,
    /// Hold the pane until it is explicitly closed
    Hold,
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExitBehaviorMessaging {
    #[default]
    Verbose,
    Brief,
    Terse,
    None,
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq)]
pub enum DroppedFileQuoting {
    /// No quoting is performed, the file name is passed through as-is
    None,
    /// Backslash escape only spaces, leaving all other characters as-is
    SpacesOnly,
    /// Use POSIX style shell word escaping
    Posix,
    /// Use Windows style shell word escaping
    Windows,
    /// Always double quote the file name
    WindowsAlwaysQuoted,
}

impl Default for DroppedFileQuoting {
    fn default() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::SpacesOnly
        }
    }
}

impl DroppedFileQuoting {
    pub fn escape(self, s: &str) -> String {
        match self {
            Self::None => s.to_string(),
            Self::SpacesOnly => s.replace(" ", "\\ "),
            // https://docs.rs/shlex/latest/shlex/fn.quote.html
            Self::Posix => shlex::try_quote(s)
                .unwrap_or_else(|_| "".into())
                .into_owned(),
            Self::Windows => {
                let chars_need_quoting = [' ', '\t', '\n', '\x0b', '\"'];
                if s.chars().any(|c| chars_need_quoting.contains(&c)) {
                    format!("\"{}\"", s)
                } else {
                    s.to_string()
                }
            }
            Self::WindowsAlwaysQuoted => format!("\"{}\"", s),
        }
    }
}

fn default_glyph_cache_image_cache_size() -> usize {
    256
}

fn default_shape_cache_size() -> usize {
    1024
}

fn default_line_state_cache_size() -> usize {
    1024
}

fn default_line_quad_cache_size() -> usize {
    1024
}

fn default_line_to_ele_shape_cache_size() -> usize {
    1024
}

#[derive(Debug, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoldBrightening {
    /// Bold doesn't influence palette selection
    No,
    /// Bold Shifts palette from 0-7 to 8-15 and preserves bold font
    #[default]
    BrightAndBold,
    /// Bold Shifts palette from 0-7 to 8-15 and removes bold intensity
    BrightOnly,
}

impl FromDynamic for BoldBrightening {
    fn from_dynamic(
        value: &wezterm_dynamic::Value,
        options: wezterm_dynamic::FromDynamicOptions,
    ) -> Result<Self, wezterm_dynamic::Error> {
        match String::from_dynamic(value, options) {
            Ok(s) => match s.as_str() {
                "No" => Ok(Self::No),
                "BrightAndBold" => Ok(Self::BrightAndBold),
                "BrightOnly" => Ok(Self::BrightOnly),
                s => Err(wezterm_dynamic::Error::Message(format!(
                    "`{s}` is not valid, use one of `No`, `BrightAndBold` or `BrightOnly`"
                ))),
            },
            Err(err) => match bool::from_dynamic(value, options) {
                Ok(true) => Ok(Self::BrightAndBold),
                Ok(false) => Ok(Self::No),
                Err(_) => Err(err),
            },
        }
    }
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImePreeditRendering {
    /// IME preedit is rendered by WezTerm itself
    #[default]
    Builtin,
    /// IME preedit is rendered by system
    System,
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationHandling {
    #[default]
    AlwaysShow,
    NeverShow,
    SuppressFromFocusedPane,
    SuppressFromFocusedTab,
    SuppressFromFocusedWindow,
}

fn validate_row_or_col(value: &u16) -> Result<(), String> {
    if *value < 1 {
        Err("initial_cols and initial_rows must be non-zero".to_string())
    } else {
        Ok(())
    }
}

fn validate_line_height(value: &f64) -> Result<(), String> {
    if *value <= 0.0 {
        Err(format!(
            "Illegal value {value} for line_height; it must be positive and greater than zero!"
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_domain_name(name: &str) -> Result<(), String> {
    if name == "local" {
        Err(format!(
            "\"{name}\" is a built-in domain and cannot be redefined"
        ))
    } else if name == "" {
        Err("the empty string is an invalid domain name".to_string())
    } else {
        Ok(())
    }
}

/// <https://github.com/wezterm/wezterm/pull/2435>
/// <https://github.com/wezterm/wezterm/issues/2771>
/// <https://github.com/wezterm/wezterm/issues/2630>
fn default_macos_forward_mods() -> Modifiers {
    Modifiers::SHIFT
}

fn default_colr_rasterizer() -> FontRasterizerSelection {
    FontRasterizerSelection::Harfbuzz
}

#[cfg(test)]
mod rhai_config_load_test {
    use super::*;
    use std::io::Write;

    /// `CONFIG_OVERRIDES` is a process-wide `lazy_static` `Mutex`, and Rust test
    /// binaries run tests concurrently on multiple threads by default. Any test
    /// that reads or writes `CONFIG_OVERRIDES` must serialize against every other
    /// such test via this mutex, or a test elsewhere in this module can observe
    /// another test's overrides mid-flight (which is exactly what happened before
    /// this guard existed: `loads_a_rhai_config_file`, which never touches
    /// `CONFIG_OVERRIDES` itself, intermittently failed with `font_size=22.5`
    /// leaking in from `applies_config_overrides_as_rhai_expressions` running
    /// concurrently on another thread).
    static CONFIG_OVERRIDES_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// End to end proof that the real, public config-loading path (`Config::load`,
    /// via `Config::try_load`) parses a `.rhai` config file: previously this
    /// exercised `onlyterm.lua`/`.onlyterm.lua` text through `make_lua_context`; this
    /// is the rhai-side equivalent, confirming the switch described in
    /// `docs/plans/2026-07-23-lua-rhai-migration.md`'s L4.5 phase actually takes
    /// effect for the production loader, not just the standalone `rhai_engine`
    /// unit tests.
    #[test]
    fn loads_a_rhai_config_file() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`: this test doesn't set any
        // overrides itself, but must still serialize against
        // `applies_config_overrides_as_rhai_expressions` (which does), since
        // both call through `Config::try_load` -> `apply_overrides_to_rhai`,
        // which reads the same global `CONFIG_OVERRIDES`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.rhai");
        std::fs::write(
            &config_path,
            r#"
                #{
                    font_size: 14.0,
                    term: "screen-256color",
                }
            "#,
        )
        .unwrap();

        let path_item = PathPossibility::required(config_path.clone());
        let loaded = Config::try_load(&path_item, &wezterm_dynamic::Value::default())
            .expect("try_load should succeed")
            .expect("a config was found at the required path");

        let cfg = loaded.config.expect("config should parse");
        assert_eq!(cfg.font_size, 14.0);
        assert_eq!(cfg.term, "screen-256color");
        assert_eq!(loaded.file_name.as_deref(), Some(config_path.as_path()));
        // The event-callback bridge descriptor (see `RhaiEventScript`, L4.6)
        // should be present and carry the script's own source, since
        // mux/wezterm-gui's runtime event bridge (`wezterm.on`/`emit`) is
        // rebuilt from it on the main thread.
        let event_script = loaded.event_script.expect("event_script should be present");
        assert!(event_script.source.is_some());
    }

    /// `--config key=value`-style overrides (see `CONFIG_OVERRIDES`) are
    /// evaluated as rhai expressions and applied on top of the parsed config,
    /// exactly like the old `apply_overrides_to` did for Lua expressions.
    #[test]
    fn applies_config_overrides_as_rhai_expressions() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.rhai");
        std::fs::write(&config_path, "#{ font_size: 10.0 }").unwrap();

        *CONFIG_OVERRIDES.lock().unwrap() = vec![("font_size".to_string(), "22.5".to_string())];

        let path_item = PathPossibility::required(config_path);
        let loaded = Config::try_load(&path_item, &wezterm_dynamic::Value::default())
            .expect("try_load should succeed")
            .expect("a config was found");
        let cfg = loaded.config.expect("config should parse");
        assert_eq!(cfg.font_size, 22.5);

        // Clean up global state so other tests in this process aren't affected.
        CONFIG_OVERRIDES.lock().unwrap().clear();
    }

    /// If a legacy `onlyterm.lua`/`.onlyterm.lua` file exists but there is no
    /// `.rhai` sibling, `try_load` must not silently ignore it (which would
    /// look to the user like "wezterm forgot my config"); it must fail with
    /// an actionable message telling them Lua configs are no longer
    /// supported and pointing at the specific file to rename/migrate.
    #[test]
    fn legacy_lua_only_config_produces_actionable_error() {
        let dir = tempfile::tempdir().unwrap();
        let lua_path = dir.path().join("onlyterm.lua");
        let mut f = std::fs::File::create(&lua_path).unwrap();
        writeln!(f, "return {{}}").unwrap();
        drop(f);

        let rhai_path = dir.path().join("onlyterm.rhai");
        let path_item = PathPossibility::optional(rhai_path);
        let err = match Config::try_load(&path_item, &wezterm_dynamic::Value::default()) {
            Err(err) => err,
            Ok(_) => panic!("a legacy .lua-only directory must error, not silently skip"),
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("no longer supported"),
            "unexpected error message: {}",
            message
        );
        assert!(
            message.contains("onlyterm.rhai"),
            "error should mention the expected new filename: {}",
            message
        );
    }

    /// Sanity check for the pure path-diagnostics helper itself: only exact
    /// `<stem>.rhai` -> `<stem>.lua` siblings are detected, and only when the
    /// `.lua` file actually exists on disk (we must never claim a legacy file
    /// exists when it doesn't, else every fresh/no-config user would see the
    /// migration error instead of falling through to defaults).
    #[test]
    fn legacy_lua_sibling_detection() {
        let dir = tempfile::tempdir().unwrap();
        let rhai_path = dir.path().join("onlyterm.rhai");
        assert_eq!(Config::legacy_lua_sibling(&rhai_path), None);

        std::fs::write(dir.path().join("onlyterm.lua"), "return {}").unwrap();
        assert_eq!(
            Config::legacy_lua_sibling(&rhai_path),
            Some(dir.path().join("onlyterm.lua"))
        );

        let dot_rhai_path = dir.path().join(".onlyterm.rhai");
        assert_eq!(Config::legacy_lua_sibling(&dot_rhai_path), None);
        std::fs::write(dir.path().join(".onlyterm.lua"), "return {}").unwrap();
        assert_eq!(
            Config::legacy_lua_sibling(&dot_rhai_path),
            Some(dir.path().join(".onlyterm.lua"))
        );
    }
}
