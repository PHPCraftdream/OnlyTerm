use super::*;
#[path = "config_impl.rs"]
mod config_impl;
#[path = "config_paths.rs"]
pub(super) mod config_paths;
#[path = "state/configuration.rs"]
pub(super) mod configuration;
#[path = "state/terminal.rs"]
pub(super) mod terminal;
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

    /// When true, launching a new onlyterm instance will prefer
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

    #[doc = "If no `prog` is specified on the command line, use this\ninstead of running the user's shell.\nFor example, to have `onlyterm` always run `top` by default,\nyou'd use this:\n\n```toml\ndefault_prog = [\"top\"]\n```\n\n`default_prog` is implemented as an array where the 0th element\nis the command to run and the rest of the elements are passed\nas the positional arguments to that command.\n"]
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
    /// After two short taps of either Ctrl key, the next key press is
    /// encoded and sent to the application in the active pane without
    /// being matched against any OnlyTerm key binding, so chords that
    /// OnlyTerm binds (eg. Ctrl+Shift+C) can still reach programs that
    /// need them. The mode disarms itself after that one physical press
    /// (its autorepeats included), on a repeated double-tap, or when
    /// the window loses focus. See
    /// docs/plans/2026-09-23-double-ctrl-pass-through.md.
    #[dynamic(default = "default_true")]
    pub pass_through_next_key_on_double_ctrl: bool,

    /// Whether the terminal should respond to requests to read the
    /// title string.
    /// Disabled by default for security concerns with shells that might
    /// otherwise attempt to execute the response.
    /// <https://marc.info/?l=bugtraq&m=104612710031920&w=2>
    #[dynamic(default)]
    pub enable_title_reporting: bool,

    /// Whether programs may change tab/window titles with OSC 0/1/2.
    /// Disabled by default to keep titles stable and avoid update storms.
    #[dynamic(default)]
    pub allow_process_title_updates: bool,

    /// Show this OnlyTerm process tree's CPU and memory usage as a suffix
    /// on the OS window title, refreshed every 5 seconds. Enabled by
    /// default.
    #[dynamic(default = "default_true")]
    pub show_process_tree_stats_in_title: bool,

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
    #[dynamic(default)]
    pub font_shaper: FontShaperSelection,

    #[dynamic(default)]
    pub freetype_load_target: FreeTypeLoadTarget,
    #[dynamic(default)]
    pub freetype_render_target: Option<FreeTypeLoadTarget>,
    #[dynamic(default)]
    pub freetype_load_flags: Option<FreeTypeLoadFlags>,

    #[doc = "Specify the features to enable when using harfbuzz for font shaping.\nThere is some light documentation here:\n<https://harfbuzz.github.io/shaping-opentype-features.html>\nbut it boils down to allowing opentype feature names to be specified\nusing syntax similar to the CSS font-feature-settings options:\n<https://developer.mozilla.org/en-US/docs/Web/CSS/font-feature-settings>.\nThe OpenType spec lists a number of features here:\n<https://docs.microsoft.com/en-us/typography/opentype/spec/featurelist>\n\nOptions of likely interest will be:\n\n* `calt` - <https://docs.microsoft.com/en-us/typography/opentype/spec/features_ae#tag-calt>\n* `clig` - <https://docs.microsoft.com/en-us/typography/opentype/spec/features_ae#tag-clig>\n\nIf you want to disable ligatures in most fonts, then you may want to\nuse a setting like this:\n\n```toml\nharfbuzz_features = [\"calt=0\", \"clig=0\", \"liga=0\"]\n```\n\nSome fonts make available extended options via stylistic sets.\nIf you use the [Fira Code font](https://github.com/tonsky/FiraCode),\nit lists available stylistic sets here:\n<https://github.com/tonsky/FiraCode/wiki/How-to-enable-stylistic-sets>\n\nand you can set them in onlyterm:\n\n```toml\n# Use this for a zero with a dot rather than a line through it\n# when using the Fira Code font\nharfbuzz_features = [\"zero\"]\n```\n"]
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

    /// When true, each new tab runs in its own single-pane hosting process
    /// via onlyterm-mux-server --single-pane. A crash in one tab's process
    /// does not affect other tabs or the window. Default: false, until
    /// Phase D (see docs/plans/2026-08-13-per-tab-process-elevated-admin-tab.md)
    /// closes the known gaps this Phase B rollout flag exists to cover --
    /// most importantly, the per-tab ClientDomain this spawns is never
    /// unregistered when the tab closes (a real, deliberately-deferred-to-
    /// Phase-D leak, not yet a problem for a short test session but not
    /// something to turn on by default for every user's every tab yet).
    /// The end state (per the plan's user-confirmed decision) is for this
    /// to always be on; this field is the rollout/rollback lever to get
    /// there safely, not the final resting state.
    #[dynamic(default)]
    pub per_tab_process_isolation: bool,

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
    /// hold back output before onlyterm stops waiting for the closing
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

    /// Fallback tab title (used when a tab hasn't been explicitly renamed
    /// via F2 / the tab-bar rename UI, and `SpawnCommand::title`
    /// wasn't set for that particular launch) applied to every newly
    /// spawned tab. If unset, the tab title tracks the cwd basename
    /// instead. See `SpawnCommand::title` for the per-launch override that
    /// takes priority over this.
    pub default_tab_title: Option<String>,

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

    /// When `text_min_contrast_ratio` is set, a cell whose foreground and
    /// background colors are exactly identical is by default boosted like
    /// any other too-low-contrast cell -- most real occurrences are an
    /// app's "dim" chrome color landing on the same value as its own
    /// background under the active color scheme, not a deliberate hide.
    /// Set this to `true` to restore the old behavior: identical
    /// foreground/background is left untouched (eg. for apps that
    /// deliberately hide text, like a password prompt).
    #[dynamic(default)]
    pub text_min_contrast_respects_hidden_text: bool,

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

    /// Panes whose foreground process is one of these names never have bidi
    /// reordering applied, even when `bidi_enabled` is true. Matched against
    /// the foreground process's executable basename (case-insensitively),
    /// re-checked live as the foreground process changes.
    ///
    /// This exists for TUI applications (like Claude Code) that lay out and
    /// position their own text -- they assume the terminal shows exactly the
    /// bytes they wrote, in the order they wrote them. Reordering their
    /// Hebrew output at the terminal layer conflicts with the app's own
    /// (bidi-unaware) cursor/layout bookkeeping and produces worse results
    /// than not reordering at all, even though the same reordering is
    /// correct for a plain shell.
    #[dynamic(default = "default_disable_bidi_for_processes_named")]
    pub disable_bidi_for_processes_named: Vec<String>,

    /// Executable basenames (matched case-insensitively against the pane's
    /// live foreground process) for which a Ctrl+<letter> chord is encoded
    /// with the plain letter in the win32-input-mode `UnicodeChar` field
    /// instead of the ASCII control code -- eg. `j` (0x6A) rather than 0x0A
    /// for Ctrl+J.
    ///
    /// The control code is what a genuine KEY_EVENT_RECORD carries and stays
    /// the default for everything else. This list exists for applications
    /// whose console reader ignores `UnicodeChar` when it is below 0x20 and
    /// instead re-derives the character from the virtual key via
    /// `ToUnicodeEx` under the *active* keyboard layout: under a Cyrillic
    /// layout that turns VK_J into 'о' and VK_C into 'с', so the chord is
    /// looked up as Ctrl+о / Ctrl+с and silently does nothing. A printable
    /// `UnicodeChar` is taken verbatim by those readers, which sidesteps the
    /// re-derivation entirely.
    ///
    /// Keep this list tight. The substitution is measurably WRONG for
    /// applications that read the byte stream, because conhost hands this
    /// field through verbatim: Claude Code, for instance, works correctly
    /// today and starts receiving a literal 'j' where it expects 0x0A.
    ///
    /// Measured against Codex CLI 0.147.0; see
    /// docs/codex-cyrillic-ctrl-chords.md for the full investigation.
    #[dynamic(default = "default_ctrl_letter_as_char_processes")]
    pub win32_input_ctrl_letter_as_char_processes: Vec<String>,

    /// Executable base names (matched case-insensitively anywhere in the
    /// pane's process tree) for which SHIFT+Enter is sent as ESC followed by
    /// CR (`\x1b\r`) instead of a faithful modified-Enter key record.
    ///
    /// A faithful SHIFT+Enter is the correct encoding and stays the default.
    /// It is useless to an application that reads the byte stream rather than
    /// console records, though: conhost renders the record down to a bare
    /// `\r`, which is indistinguishable from plain Enter, so the chord
    /// submits instead of inserting a newline. `ESC` `CR` is the convention
    /// such applications recognise for "newline, not submit".
    ///
    /// Do NOT widen this to applications that read console records --
    /// Codex CLI resolves SHIFT+Enter by virtual key and needs the faithful
    /// form; ESC CR would be two unrelated keypresses to it.
    #[dynamic(default = "default_shift_enter_esc_cr_processes")]
    pub shift_enter_esc_cr_processes: Vec<String>,

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

#[path = "state/defaults.rs"]
pub(super) mod defaults;
use defaults::*;

#[cfg(test)]
use crate::keyassignment::KeyAssignment;

#[cfg(test)]
#[path = "ktav_config_load_test.rs"]
mod ktav_config_load_test;
#[cfg(test)]
#[path = "stack_overflow_repro.rs"]
mod stack_overflow_repro;
