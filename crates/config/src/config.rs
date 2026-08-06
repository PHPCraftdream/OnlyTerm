use crate::background::{BackgroundLayer, Gradient};
use crate::bell::{AudibleBell, EasingFunction, VisualBell};
use crate::color::{HsbTransform, Palette, SrgbaTuple, TabBarStyle, WindowFrameConfig};
use crate::config_types::*;
#[path = "config_impl.rs"]
mod config_impl;
use crate::daemon::DaemonOptions;
use crate::exec_domain::ExecDomain;
use crate::font::{
    AllowSquareGlyphOverflow, DisplayPixelGeometry, FontLocatorSelection, FontRasterizerSelection,
    FontShaperSelection, FreeTypeLoadFlags, FreeTypeLoadTarget, StyleRule, TextStyle,
};
use crate::frontend::FrontEndSelection;
#[cfg(test)]
use crate::keyassignment::KeyAssignment;
use crate::keyassignment::SpawnCommand;
use crate::keys::{Key, LeaderKey, Mouse};
use crate::units::Dimension;
use crate::unix::UnixDomain;
#[cfg(test)]
use crate::CONFIG_OVERRIDES;
use crate::{
    default_one_point_oh, default_one_point_oh_f64, default_true,
    default_win32_acrylic_accent_color, CellWidth, GpuInfo, IntegratedTitleButtonColor,
    KeyMapPreference, RgbaColor, SerialDomain, SystemBackdrop, WebGpuPowerPreference,
};
use anyhow::Context;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use termwiz::hyperlink;
use wezterm_bidi::ParagraphDirectionHint;
use wezterm_config_derive::ConfigMeta;
use wezterm_dynamic::{FromDynamic, ToDynamic};
use wezterm_input_types::{
    IntegratedTitleButton, IntegratedTitleButtonAlignment, IntegratedTitleButtonStyle, Modifiers,
    UIKeyCapRendering, WindowDecorations,
};

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

    /// The color palette.
    ///
    /// Deliberately has no default: this field means "what the user
    /// explicitly asked for", and `compute_extra_defaults` overlays it on
    /// top of the resolved color scheme. Giving it a default overlaid that
    /// default onto every scheme as well, which is how `color_scheme` came
    /// to have no effect whatsoever. The fork's light default palette now
    /// applies only when neither `colors` nor `color_scheme` is set -- see
    /// `compute_extra_defaults`.
    #[dynamic(default)]
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
    /// Was meant to let the COLR (color glyph) rasterizer be selected
    /// independently of `font_rasterizer`, but no code path ever read this
    /// field: `wezterm_font::rasterizer::new_rasterizer` (the sole call
    /// site, driven only by `font_rasterizer`) always renders COLR/COLRv1
    /// color glyphs by delegating internally to
    /// `rasterizer::colr_paint::ColrRasterizer` from within
    /// `SwashRasterizer`, with no way to pick a different backend for them.
    #[dynamic(
        default = "default_colr_rasterizer",
        deprecated = "this option currently has no effect: COLR/COLRv1 color glyph rendering is \
                      always handled internally by the Swash rasterizer and does not consult \
                      this setting"
    )]
    pub font_colr_rasterizer: FontRasterizerSelection,
    #[dynamic(default)]
    pub font_shaper: FontShaperSelection,

    /// NOTE: this option currently has no effect. It told the FreeType
    /// rasterizer (`wezterm-font/src/rasterizer/freetype.rs`, added in
    /// `8582165ff`) which subpixel channel order to assume when unpacking
    /// `FT_PIXEL_MODE_LCD`/`FT_PIXEL_MODE_LCD_V` bitmaps. That rasterizer
    /// was removed in the freetype+harfbuzz -> rustybuzz+swash migration
    /// (phase H4); the Swash-based rasterizer that replaced it always
    /// renders grayscale alpha masks and has no LCD/subpixel mode to steer,
    /// so `new_rasterizer` (`wezterm-font/src/rasterizer/mod.rs`) receives
    /// this value and explicitly discards it.
    #[dynamic(
        default,
        deprecated = "this option no longer does anything: it selected the LCD subpixel channel \
                      order for the FreeType rasterizer's bitmap output, and that rasterizer \
                      backend was removed in the rustybuzz/swash migration -- the Swash \
                      rasterizer that replaced it only ever produces grayscale alpha masks"
    )]
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
    ///
    /// NOTE: this option currently has no effect. It configured the
    /// FreeType `truetype`/`interpreter-version` property in
    /// `wezterm-font/src/ftwrap.rs::Library::new`, but that file (and the
    /// vendored FreeType backend it wrapped) was removed in the
    /// freetype+harfbuzz -> rustybuzz+swash migration (phase H4); the
    /// Swash-based rasterizer that replaced it has no equivalent knob.
    #[dynamic(
        default,
        deprecated = "this option no longer does anything: it configured the FreeType TrueType \
                      interpreter version, and the FreeType rasterizer backend it applied to was \
                      removed in the rustybuzz/swash migration"
    )]
    pub freetype_interpreter_version: Option<u32>,

    /// NOTE: this option currently has no effect. It configured the
    /// FreeType `pcf`/`no-long-family-names` property in
    /// `wezterm-font/src/ftwrap.rs::Library::new`, but that file (and the
    /// vendored FreeType backend it wrapped) was removed in the
    /// freetype+harfbuzz -> rustybuzz+swash migration (phase H4); the
    /// Swash-based rasterizer that replaced it has no equivalent knob.
    #[dynamic(
        default,
        deprecated = "this option no longer does anything: it configured a FreeType PCF font \
                      driver property, and the FreeType rasterizer backend it applied to was \
                      removed in the rustybuzz/swash migration"
    )]
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
        deprecated = "this option no longer does anything: the update-notification window it \
                      controlled was removed upstream in 39d2b6ca8, and it will be removed in a \
                      future release"
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

    /// How long, in milliseconds, a helper child process spawned to produce
    /// a status-bar/tab-title fragment (e.g. shelling out to `git`,
    /// `kubectl`, etc.) may block before giving up. 3000ms was chosen as
    /// comfortably above any legitimate status-bar refresh command's normal
    /// running time (these are meant to be quick, sub-second checks) while
    /// still being far short of "the user notices the window is frozen".
    /// Set to 0 to disable the timeout and wait indefinitely (the
    /// historical behavior).
    ///
    /// NOTE: as of the rhai/lua config-scripting removal (the event
    /// callbacks this timeout used to guard -- `format-tab-title`,
    /// `format-window-title`, `update-status` -- no longer exist), nothing
    /// in this codebase reads this field. It is kept only because the
    /// `.ktav` format still declares/defaults it; see task #315 for the
    /// investigation that turned it up as dead. Marked deprecated (task
    /// #320) so anyone who set it explicitly gets a warning instead of
    /// silently believing it still guards something.
    #[dynamic(
        default = "default_child_process_timeout_ms",
        deprecated = "this option no longer does anything: the event callbacks it used to guard (format-tab-title, format-window-title, update-status) were removed along with the Lua/rhai scripting layer, and will be removed in a future release"
    )]
    pub child_process_timeout_ms: u64,

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

/// WebGpu is the only supported front end; the OpenGL renderer (and its
/// `Software`/Mesa variant) has been removed from OnlyTerm.
fn default_front_end() -> FrontEndSelection {
    FrontEndSelection::WebGpu
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

fn default_child_process_timeout_ms() -> u64 {
    3_000
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

const fn linear_ease() -> EasingFunction {
    EasingFunction::Linear
}

/// The scrollbar thumb should never shrink to the point of being hard to
/// grab or see, even on very long scrollback buffers.
const fn default_min_scroll_bar_height() -> Dimension {
    Dimension::Cells(2.0)
}

const fn default_reverse_video_cursor_min_contrast() -> f32 {
    2.5
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
    } else if name.is_empty() {
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
mod ktav_config_load_test {
    use super::*;
    use std::io::Write;

    /// `CONFIG_OVERRIDES` is a process-wide `lazy_static` `Mutex`, and Rust test
    /// binaries run tests concurrently on multiple threads by default. Any test
    /// that reads or writes `CONFIG_OVERRIDES` must serialize against every other
    /// such test via this mutex, or a test elsewhere in this module can observe
    /// another test's overrides mid-flight (which is exactly what happened before
    /// this guard existed: `loads_a_ktav_config_file`, which never touches
    /// `CONFIG_OVERRIDES` itself, intermittently failed with `font_size=22.5`
    /// leaking in from `applies_config_overrides_to_ktav_config` running
    /// concurrently on another thread).
    static CONFIG_OVERRIDES_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// End to end proof that the real, public config-loading path (`Config::load`,
    /// via `Config::try_load`) parses a `.ktav` config file: this is the
    /// keystone task (#275) of the rhai -> ktav config-format migration,
    /// replacing the previous rhai-script-evaluation loading path with a
    /// direct `ktav::parse` -> `wezterm_dynamic::Value` ->
    /// `Config::from_dynamic` pipeline (see `ktav_value::ktav_value_to_dynamic`
    /// and `Config::from_ktav_dynamic`). Exercises a font size override, a
    /// color scheme name, and a keybinding, so this proves the new load path
    /// actually populates real `Config` fields end-to-end, not just compiles.
    #[test]
    fn loads_a_ktav_config_file() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`: this test doesn't set any
        // overrides itself, but must still serialize against
        // `applies_config_overrides_to_ktav_config` (which does), since both
        // call through `Config::try_load` -> `apply_overrides_to_ktav`,
        // which reads the same global `CONFIG_OVERRIDES`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.ktav");
        std::fs::write(
            &config_path,
            "\
font_size: 14
term: screen-256color
color_scheme: Builtin Solarized Dark
keys: [
    {
        key: t
        mods: CTRL|SHIFT
        action: ToggleFullScreen
    }
]
",
        )
        .unwrap();

        let path_item = PathPossibility::required(config_path.clone());
        let loaded = Config::try_load(&path_item, &wezterm_dynamic::Value::default())
            .expect("try_load should succeed")
            .expect("a config was found at the required path");

        let cfg = loaded.config.expect("config should parse");
        assert_eq!(cfg.font_size, 14.0);
        assert_eq!(cfg.term, "screen-256color");
        assert_eq!(cfg.color_scheme.as_deref(), Some("Builtin Solarized Dark"));
        assert_eq!(cfg.keys.len(), 1);
        let key = &cfg.keys[0];
        assert_eq!(key.key.mods, Modifiers::CTRL | Modifiers::SHIFT);
        assert!(matches!(key.action, KeyAssignment::ToggleFullScreen));
        assert_eq!(loaded.file_name.as_deref(), Some(config_path.as_path()));
    }

    /// `--config key=value`-style overrides (see `CONFIG_OVERRIDES`) are
    /// parsed as standalone ktav value fragments and spliced on top of the
    /// parsed config, replacing the previous rhai-expression-evaluation
    /// behavior now that config values are plain, engine-free data.
    #[test]
    fn applies_config_overrides_to_ktav_config() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.ktav");
        std::fs::write(&config_path, "font_size: 10\n").unwrap();

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

    /// Task #413 (OpenGL removal, config layer): a real `.ktav` config file
    /// left over from before the OpenGL renderer was removed may still say
    /// `front_end: OpenGL` (the historical default) or `front_end: Software`
    /// (the Mesa/SWRAST mode). Neither backend exists anymore, but loading
    /// such a config must not fail -- that would turn a previously-working
    /// setup into a hard error on upgrade. `FrontEndSelection::from_dynamic`
    /// maps both legacy values onto `WebGpu` (see `frontend.rs`); this test
    /// exercises that mapping through the full, real
    /// `Config::try_load` -> `ktav::parse` -> `Config::from_dynamic` pipeline,
    /// not just the unit-level `FromDynamic` impl.
    #[test]
    fn legacy_front_end_values_migrate_to_web_gpu() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let load = |body: &str| {
            let dir = tempfile::tempdir().unwrap();
            let config_path = dir.path().join("onlyterm.ktav");
            std::fs::write(&config_path, body).unwrap();
            let path_item = PathPossibility::required(config_path);
            Config::try_load(&path_item, &wezterm_dynamic::Value::default())
                .expect("try_load should succeed")
                .expect("a config was found")
                .config
                .expect("config should parse despite the removed front_end value")
        };

        let cfg = load("front_end: OpenGL\n");
        assert_eq!(cfg.front_end, FrontEndSelection::WebGpu);

        let cfg = load("front_end: Software\n");
        assert_eq!(cfg.front_end, FrontEndSelection::WebGpu);

        // The still-supported value keeps working normally.
        let cfg = load("front_end: WebGpu\n");
        assert_eq!(cfg.front_end, FrontEndSelection::WebGpu);
    }

    /// Regression test for task #336: `color_scheme` used to have no effect
    /// whatsoever. `colors` carried a non-empty default (this fork's light
    /// `default_colors` palette), and `compute_extra_defaults` overlays
    /// `colors` on top of the resolved scheme -- so the default silently
    /// overwrote every scheme the user asked for. The three cases below
    /// pin the intended layering.
    #[test]
    fn color_scheme_is_not_clobbered_by_the_default_palette() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let load = |body: &str| {
            let dir = tempfile::tempdir().unwrap();
            let config_path = dir.path().join("onlyterm.ktav");
            std::fs::write(&config_path, body).unwrap();
            let path_item = PathPossibility::required(config_path);
            Config::try_load(&path_item, &wezterm_dynamic::Value::default())
                .expect("try_load should succeed")
                .expect("a config was found")
                .config
                .expect("config should parse")
        };
        let rgba = |hex: &str| {
            <crate::color::RgbaColor as std::convert::TryFrom<String>>::try_from(hex.to_string())
                .unwrap()
        };

        // A scheme alone must actually reach the palette. Batman's
        // background is #1b1d1e; before the fix this came back white.
        let cfg = load(
            "color_scheme: Batman
",
        );
        assert_eq!(
            cfg.resolved_palette.background,
            Some(rgba("#1b1d1e")),
            "color_scheme must decide the background when `colors` is unset"
        );

        // An explicit `colors` still wins over the scheme -- that is the
        // documented override direction.
        let cfg = load(
            "color_scheme: Batman
colors: { background: #123456 }
",
        );
        assert_eq!(
            cfg.resolved_palette.background,
            Some(rgba("#123456")),
            "explicit colors must override the scheme"
        );

        // With neither set, the fork's light default still applies.
        let cfg = load(
            "font_size: 12
",
        );
        assert_eq!(
            cfg.resolved_palette.background,
            Some(rgba("#ffffff")),
            "the light default palette must survive when nothing is configured"
        );
    }

    /// Regression test for task #294: ktav object keys are unconditionally
    /// strings (see `crate::ktav_value::ktav_value_to_dynamic`), so a
    /// numeric-looking `colors.indexed` key like `136` arrives as
    /// `Value::String("136")`, not `Value::I64(136)`. `Palette::indexed` is
    /// `HashMap<u8, RgbaColor>`, and before the `map_key_from_dynamic`
    /// fallback in `wezterm_dynamic::fromdynamic` (see
    /// `crates/wezterm-dynamic/src/fromdynamic.rs`), `u8::from_dynamic`
    /// rejected a `Value::String` key outright with an opaque
    /// `NoConversion { source_type: "String", dest_type: "u8" }`, so any user
    /// customizing `colors.indexed` -- documented as usable at
    /// `docs/config/appearance.md` -- got a config load failure. This proves
    /// the documented syntax loads end-to-end and lands at the expected
    /// palette index.
    #[test]
    fn loads_colors_indexed_with_string_keys() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.ktav");
        std::fs::write(
            &config_path,
            "\
colors: {
    indexed: { 136: #af8700 }
}
",
        )
        .unwrap();

        let path_item = PathPossibility::required(config_path);
        let loaded = Config::try_load(&path_item, &wezterm_dynamic::Value::default())
            .expect("try_load should succeed")
            .expect("a config was found");
        let cfg = loaded.config.expect("config should parse");

        let palette = cfg.colors.expect("colors table should be present");
        let expected = <crate::color::RgbaColor as std::convert::TryFrom<String>>::try_from(
            "#af8700".to_string(),
        )
        .unwrap();
        assert_eq!(
            palette.indexed.get(&136u8),
            Some(&expected),
            "colors.indexed.136 should have loaded from the string key \"136\""
        );
    }

    /// If a legacy `onlyterm.rhai`/`onlyterm.lua` file exists but there is no
    /// `.ktav` sibling, `try_load` must not silently ignore it (which would
    /// look to the user like "wezterm forgot my config"); it must fail with
    /// an actionable message telling them scripted configs are no longer
    /// supported and pointing at the specific file to rename/migrate.
    #[test]
    fn legacy_rhai_only_config_produces_actionable_error() {
        let dir = tempfile::tempdir().unwrap();
        let rhai_path = dir.path().join("onlyterm.rhai");
        let mut f = std::fs::File::create(&rhai_path).unwrap();
        writeln!(f, "#{{}}").unwrap();
        drop(f);

        let ktav_path = dir.path().join("onlyterm.ktav");
        let path_item = PathPossibility::optional(ktav_path);
        let err = match Config::try_load(&path_item, &wezterm_dynamic::Value::default()) {
            Err(err) => err,
            Ok(_) => panic!("a legacy .rhai-only directory must error, not silently skip"),
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("no longer supported"),
            "unexpected error message: {}",
            message
        );
        assert!(
            message.contains("onlyterm.ktav"),
            "error should mention the expected new filename: {}",
            message
        );
    }

    /// Regression test for task #298 / bug F9: a legacy `.rhai`/`.lua`
    /// sibling sitting next to an *earlier* candidate in the config search
    /// order must not prevent a *later* candidate's valid, already-migrated
    /// `.ktav` config from loading. Concretely: a user who migrated from
    /// `$HOME/.onlyterm.rhai` to the more advanced `<config-dir>/onlyterm.ktav`
    /// location, but left the old `.rhai` file sitting on disk, must still
    /// start successfully from the `.ktav` file found later in the search
    /// order -- not be blocked by a legacy-script error pointing at the old,
    /// irrelevant file. This exercises the actual multi-candidate search loop
    /// (`Config::search_paths_for_config`), not just a single `try_load`
    /// call, since the bug was specifically in how the search loop reacted
    /// to an error from an earlier candidate.
    #[test]
    fn legacy_sibling_at_earlier_candidate_does_not_block_later_valid_ktav() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        // First (higher-priority) candidate directory: has an old `.rhai`
        // file but no `.ktav` replacement -- simulates a stale legacy file
        // left behind after migrating elsewhere.
        let old_dir = tempfile::tempdir().unwrap();
        std::fs::write(old_dir.path().join(".onlyterm.rhai"), "#{}").unwrap();
        let old_ktav_path = old_dir.path().join(".onlyterm.ktav");

        // Second (lower-priority) candidate directory: has a genuine, valid,
        // already-migrated `.ktav` config.
        let new_dir = tempfile::tempdir().unwrap();
        let new_ktav_path = new_dir.path().join("onlyterm.ktav");
        std::fs::write(&new_ktav_path, "font_size: 18\nterm: screen\n").unwrap();

        let paths = vec![
            PathPossibility::optional(old_ktav_path),
            PathPossibility::optional(new_ktav_path.clone()),
        ];

        let loaded = Config::search_paths_for_config(&paths, &wezterm_dynamic::Value::default())
            .expect("the later candidate's valid .ktav config should be found");
        let cfg = loaded
            .config
            .expect("a valid .ktav config later in the search order must load successfully, not be blocked by an earlier legacy .rhai sibling");
        assert_eq!(cfg.font_size, 18.0);
        assert_eq!(cfg.term, "screen");
        assert_eq!(loaded.file_name.as_deref(), Some(new_ktav_path.as_path()));
    }

    /// Companion to the test above: confirms the existing legacy-detection
    /// behavior is unchanged for the simple case where NO candidate anywhere
    /// in the search order has a valid `.ktav` config -- only a legacy
    /// `.rhai`/`.lua` file. This must still produce the same clear,
    /// actionable "found a legacy scripted configuration file" error as
    /// before, not be silently swallowed now that the search loop defers
    /// legacy-sibling errors.
    #[test]
    fn legacy_sibling_with_no_ktav_anywhere_still_errors() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let old_dir = tempfile::tempdir().unwrap();
        std::fs::write(old_dir.path().join(".onlyterm.rhai"), "#{}").unwrap();
        let old_ktav_path = old_dir.path().join(".onlyterm.ktav");

        // A second candidate directory that has neither a `.ktav` file nor
        // any legacy sibling at all -- just a plain "nothing here".
        let empty_dir = tempfile::tempdir().unwrap();
        let empty_ktav_path = empty_dir.path().join("onlyterm.ktav");

        let paths = vec![
            PathPossibility::optional(old_ktav_path),
            PathPossibility::optional(empty_ktav_path),
        ];

        let loaded = Config::search_paths_for_config(&paths, &wezterm_dynamic::Value::default())
            .expect("a deferred legacy-sibling error must still be surfaced eventually");
        let err = match loaded.config {
            Err(err) => err,
            Ok(_) => panic!(
                "a legacy .rhai-only search order (no .ktav found anywhere) must still \
                 error, not silently fall through"
            ),
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("no longer supported"),
            "unexpected error message: {}",
            message
        );
        assert!(
            message.contains(".onlyterm.rhai") || message.contains("onlyterm.rhai"),
            "error should mention the specific legacy file to migrate: {}",
            message
        );
    }

    /// Regression test for task #301 (second-round review of task #298's
    /// refactor): when no candidate anywhere in the search order produces a
    /// real config and the deferred legacy-sibling error is finally
    /// surfaced, `LoadedConfig.file_name` must be `Some(expected_path)` --
    /// the not-yet-existing `.ktav` path the user is expected to migrate
    /// to -- not `None`. `ConfigInner::reload` (see
    /// `crates/config/src/lib.rs`) builds its filesystem-watch list solely
    /// from `file_name`; with `None` nothing gets watched in this error
    /// state, so migrating the legacy file while OnlyTerm is still running
    /// would never be picked up by the live-reload watcher until the user
    /// manually restarts. Before task #298's refactor this was
    /// `Some(path_item.path.clone())`; the refactor accidentally dropped it
    /// to `None` on the deferred-error path specifically.
    #[test]
    fn deferred_legacy_error_still_reports_expected_ktav_path_as_file_name() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let old_dir = tempfile::tempdir().unwrap();
        std::fs::write(old_dir.path().join(".onlyterm.rhai"), "#{}").unwrap();
        let old_ktav_path = old_dir.path().join(".onlyterm.ktav");

        // A second candidate directory that has neither a `.ktav` file nor
        // any legacy sibling at all -- just a plain "nothing here" -- so no
        // candidate in the whole search order produces a real config.
        let empty_dir = tempfile::tempdir().unwrap();
        let empty_ktav_path = empty_dir.path().join("onlyterm.ktav");

        let paths = vec![
            PathPossibility::optional(old_ktav_path.clone()),
            PathPossibility::optional(empty_ktav_path),
        ];

        let loaded = Config::search_paths_for_config(&paths, &wezterm_dynamic::Value::default())
            .expect("a deferred legacy-sibling error must still be surfaced eventually");
        assert!(
            loaded.config.is_err(),
            "no valid .ktav config exists anywhere, so this must still be an error"
        );
        assert_eq!(
            loaded.file_name.as_deref(),
            Some(old_ktav_path.as_path()),
            "file_name must be the expected (not-yet-existing) .ktav path, so that its \
             parent directory -- which does exist -- ends up on the config-reload \
             watch list; otherwise migrating the legacy file while OnlyTerm is still \
             running is never picked up by the live-reload watcher"
        );
        // The expected path's parent directory is `old_dir`, which does
        // exist on disk: this is exactly the directory that
        // `ConfigInner::reload` in `crates/config/src/lib.rs` would add to
        // its `notify` watch list from `file_name.parent()`, since
        // `file_name` is now `Some(old_ktav_path)` rather than `None`.
        assert!(
            loaded.file_name.as_ref().unwrap().parent() == Some(old_dir.path()),
            "the watched parent directory must be the real, existing directory the user \
             would migrate their config into"
        );
    }

    /// Regression test for task #295: `ExecDomain::fixup_command` used to
    /// name a rhai function that rewrote a spawned command (e.g. to route
    /// it into `docker exec`, `ssh`, etc. -- see
    /// `LocalDomain::fixup_command` in `crates/mux/src/domain.rs`). With
    /// the rhai/Lua scripting engines removed there is no callback
    /// mechanism left to run, and a mechanical removal of the rhai
    /// callsite left the rest of the spawn path proceeding with the
    /// command completely unwrapped -- silently spawning on the host
    /// instead of erroring. A non-empty `exec_domains` must now fail to
    /// load, loudly, rather than let that silent behavior change through.
    #[test]
    fn exec_domains_are_rejected_at_load_time() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.ktav");
        std::fs::write(
            &config_path,
            "\
exec_domains: [
    {
        name: mydomain
        fixup_command: wrap_in_docker
    }
]
",
        )
        .unwrap();

        let path_item = PathPossibility::required(config_path);
        let err = match Config::try_load(&path_item, &wezterm_dynamic::Value::default()) {
            Err(err) => err,
            Ok(_) => panic!(
                "a config with a non-empty exec_domains must fail to load, \
                 not silently spawn commands unwrapped"
            ),
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("rhai scripting engine")
                && message.contains("fixup_command")
                && message.contains("no longer usable"),
            "error should clearly explain that exec_domains can no longer \
             wrap commands now that the rhai scripting engine is gone: {}",
            message
        );
    }

    /// The common case -- no `exec_domains` at all -- must be completely
    /// unaffected by the task #295 check: no new error, no behavior change.
    #[test]
    fn config_with_no_exec_domains_is_unaffected() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.ktav");
        std::fs::write(&config_path, "font_size: 12\n").unwrap();

        let path_item = PathPossibility::required(config_path);
        let loaded = Config::try_load(&path_item, &wezterm_dynamic::Value::default())
            .expect("try_load should succeed")
            .expect("a config was found");
        let cfg = loaded
            .config
            .expect("config with no exec_domains should load fine");
        assert!(cfg.exec_domains.is_empty());
        assert_eq!(cfg.font_size, 12.0);
    }

    /// Regression test for task #296: a config file that parses
    /// successfully as *syntactically valid ktav* but not as a top-level
    /// `Object` -- e.g. because a `key: value` line is missing the space
    /// after the `:`, so ktav reads the whole line as one bare unquoted
    /// string rather than a `key`/`value` pair, and the whole document
    /// parses as a top-level `Array` of strings -- must fail with a clear
    /// error naming the actual parsed shape and hinting at the likely
    /// cause, not the old opaque `NoConversion Array`-style error from deep
    /// inside `wezterm_dynamic`'s `Config::from_dynamic` conversion.
    #[test]
    fn malformed_top_level_array_config_produces_actionable_error() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.ktav");
        // Missing space after the first `:` on each line: ktav parses each
        // line as one bare string rather than a `key: value` pair, so the
        // whole document parses as a top-level `Array`, not an `Object`.
        std::fs::write(&config_path, "font_size:14\nterm: screen\n").unwrap();

        let path_item = PathPossibility::required(config_path);
        let err = match Config::try_load(&path_item, &wezterm_dynamic::Value::default()) {
            Err(err) => err,
            Ok(_) => panic!(
                "a config document that parses as a top-level array, not an \
                 object, must fail to load, not be silently accepted"
            ),
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("top-level ktav object")
                && message.contains("an array")
                && message.contains("missing the space after the `:`"),
            "error should clearly explain the config parsed as a non-Object \
             shape and hint at the likely typo, not just show the old opaque \
             `NoConversion Array` error: {}",
            message
        );
        assert!(
            !message.contains("NoConversion"),
            "the new, clearer error should be raised before ever reaching \
             the opaque wezterm_dynamic NoConversion error: {}",
            message
        );
    }

    /// The common case -- a normal, correctly-formed ktav config file that
    /// parses as a top-level object -- must be completely unaffected by the
    /// task #296 shape check: no new error, no false-positive rejection.
    #[test]
    fn normal_config_is_unaffected_by_top_level_shape_check() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.ktav");
        std::fs::write(&config_path, "font_size: 14\nterm: screen\n").unwrap();

        let path_item = PathPossibility::required(config_path);
        let loaded = Config::try_load(&path_item, &wezterm_dynamic::Value::default())
            .expect("try_load should succeed")
            .expect("a config was found");
        let cfg = loaded
            .config
            .expect("a normal top-level-object ktav config should load fine");
        assert_eq!(cfg.font_size, 14.0);
        assert_eq!(cfg.term, "screen");
    }

    /// Sanity check for the pure path-diagnostics helper itself: only exact
    /// `<stem>.ktav` -> `<stem>.rhai`/`<stem>.lua` siblings are detected, and
    /// only when the legacy file actually exists on disk (we must never
    /// claim a legacy file exists when it doesn't, else every fresh/no-config
    /// user would see the migration error instead of falling through to
    /// defaults).
    #[test]
    fn legacy_script_sibling_detection() {
        let dir = tempfile::tempdir().unwrap();
        let ktav_path = dir.path().join("onlyterm.ktav");
        assert_eq!(Config::legacy_script_sibling(&ktav_path), None);

        std::fs::write(dir.path().join("onlyterm.rhai"), "#{}").unwrap();
        assert_eq!(
            Config::legacy_script_sibling(&ktav_path),
            Some(dir.path().join("onlyterm.rhai"))
        );

        let dot_ktav_path = dir.path().join(".onlyterm.ktav");
        assert_eq!(Config::legacy_script_sibling(&dot_ktav_path), None);
        std::fs::write(dir.path().join(".onlyterm.lua"), "return {}").unwrap();
        assert_eq!(
            Config::legacy_script_sibling(&dot_ktav_path),
            Some(dir.path().join(".onlyterm.lua"))
        );
    }

    /// Task #420 (OS-portable config paths). A relative `font_dirs` entry
    /// must resolve to a path under the directory that holds the config
    /// file itself, using whatever `PathBuf`/`Path::join` produces on the
    /// host OS -- there is no hardcoded separator or platform assumption in
    /// `compute_extra_defaults`. This is what makes a relative `font_dirs`
    /// entry (e.g. `font_dirs: [fonts]`) safe to sync between Windows and
    /// Linux/macOS: the join happens the same way regardless of platform,
    /// unlike a hardcoded absolute path such as `C:/Windows/Fonts`, which
    /// only ever makes sense on one OS (see `docs/config/reference/config/
    /// font_dirs.md`).
    #[test]
    fn relative_font_dirs_resolve_against_the_config_file_directory() {
        // See `CONFIG_OVERRIDES_TEST_LOCK`.
        let _guard = CONFIG_OVERRIDES_TEST_LOCK.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("onlyterm.ktav");
        std::fs::write(&config_path, "font_dirs: [fonts, ../shared-fonts]\n").unwrap();

        let path_item = PathPossibility::required(config_path);
        let loaded = Config::try_load(&path_item, &wezterm_dynamic::Value::default())
            .expect("try_load should succeed")
            .expect("a config was found");
        let cfg = loaded.config.expect("config should parse");

        assert_eq!(
            cfg.font_dirs,
            vec![dir.path().join("fonts"), dir.path().join("../shared-fonts"),],
            "relative font_dirs entries must resolve relative to the config \
             file's own directory on every OS, with no hardcoded platform path"
        );
        for font_dir in &cfg.font_dirs {
            assert!(
                font_dir.is_absolute(),
                "resolved font_dirs entries must be absolute: {:?}",
                font_dir
            );
        }
    }
}

#[cfg(test)]
mod stack_overflow_repro {
    //! Task #422: investigating a real `EXCEPTION_STACK_OVERFLOW` seen in
    //! production (two crash dumps, identical crash address, both at
    //! 5-7s of uptime) with the reported crash frame inside
    //! `FontDatabase::with_font_dirs` (`crates/wezterm-font/src/db.rs`)
    //! while scanning `C:/Windows/Fonts` led here instead of to font
    //! parsing: bisecting the crash (shrinking the font-file set under
    //! test, then isolating individual `ttf_parser`/`swash` accessor
    //! calls on the single file the bisection converged on,
    //! `EmojiOneColor-SVGinOT.ttf`) showed every font-parsing call
    //! succeeding fine even on a 64KiB thread stack -- ruling out
    //! COLR/COLRv1 paint-graph recursion, TTC collection handling, and
    //! cmap/coverage walking as the cause (see
    //! `crates/wezterm-font/src/db.rs`'s `stack_overflow_repro` module
    //! for the font-side half of this investigation).
    //!
    //! What *did* reproduce the overflow in isolation, with no font file
    //! involved at all, was the first call to `Config::default()` in the
    //! process. `Config` derives `FromDynamic`
    //! (`crates/wezterm-dynamic/derive/src/fromdynamic.rs`), and for a
    //! struct with 200+ fields that derive expands into one large
    //! `from_dynamic` function body containing one field-initializer
    //! expression per field (each doing a map lookup, an
    //! `Option`/`Result` match, and a fallback-to-default) -- all inlined
    //! into a single `Ok(Self { .. })` struct literal, not a bounded
    //! recursion. In an unoptimized/debug build this function's own
    //! stack frame measured as needing on the order of 256-512KiB to
    //! avoid overflowing (down to ~128-256KiB in an optimized release
    //! build); `Config`'s own resting size (`std::mem::size_of::<Config>()`,
    //! asserted below) is a few KiB, so this is about the *construction*
    //! being frame-heavy, not the final value being large.
    //!
    //! This is comfortably within the 1MiB stack Windows gives a
    //! process's main thread by default (confirmed nothing in this
    //! codebase runs `Config::default()`/`config::configuration()` on a
    //! smaller custom-stack-size thread), and re-running the exact
    //! crashing configuration directly against a freshly built GUI
    //! binary did not reproduce the crash within a comparable uptime
    //! window. So this size-of/stack-cost regression test exists as a
    //! canary rather than as a fix for a confirmed live bug: if `Config`
    //! keeps growing and this ever needs meaningfully more than 512KiB
    //! in a debug build, that is worth knowing before it gets any closer
    //! to the 1MiB default main-thread budget.
    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "stack_size probing is most meaningful on Windows' fixed 1MiB main-thread default"
    )]
    fn config_default_fits_comfortably_under_half_the_default_stack() {
        // Half of the 1MiB Windows gives a process's main thread by
        // default: if `Config::default()` needs anywhere near this much
        // margin, that is a sign the derive-generated `from_dynamic` is
        // creeping towards being a real risk on the actual default stack,
        // not just a probe artifact.
        const HALF_OF_DEFAULT_MAIN_THREAD_STACK: usize = 512 * 1024;

        // The struct's own resting size is unrelated to the derive-generated
        // constructor's stack-frame cost above -- assert it stays small so
        // the doc comment's "construction is frame-heavy, not the value
        // itself" claim keeps being true rather than silently going stale.
        assert!(
            std::mem::size_of::<crate::Config>() < 16 * 1024,
            "Config::size_of() grew unexpectedly large ({} bytes) -- re-check whether the \
             frame-heavy-construction-not-large-value assumption in this module's doc comment \
             still holds",
            std::mem::size_of::<crate::Config>()
        );

        let handle = std::thread::Builder::new()
            .name("config-default-stack-canary".into())
            .stack_size(HALF_OF_DEFAULT_MAIN_THREAD_STACK)
            .spawn(crate::Config::default)
            .expect("failed to spawn probe thread");

        // Note: on Windows, an actual stack overflow hits the guard page and
        // aborts the whole process (Rust cannot turn it into a catchable
        // panic), so if this probe thread genuinely overflows, the test
        // binary dies outright rather than this specific test failing with
        // the message below -- that message is only reachable if
        // `Config::default()` panics here for some *other* reason. The
        // process-level abort is still a visible, loud CI failure either
        // way, just not this friendlier one.
        handle.join().expect(
            "Config::default() panicked (not necessarily a stack overflow -- a real overflow \
             would abort the whole process instead of reaching this message) on a thread stack \
             half the size of Windows' default 1MiB main-thread stack -- FromDynamic's generated \
             from_dynamic for Config has grown large enough to be a real stack-overflow risk \
             (see task #422); consider whether new fields can avoid growing this further, or \
             whether the derive macro needs to split large structs' from_dynamic bodies into \
             smaller, separately-framed helper functions",
        );
    }
}
