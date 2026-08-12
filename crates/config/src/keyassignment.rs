use crate::default_true;
use crate::keys::KeyNoAction;
use crate::window::WindowLevel;
use ordered_float::NotNan;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wezterm_dynamic::{FromDynamic, FromDynamicOptions, ToDynamic, Value};
use wezterm_input_types::{KeyCode, Modifiers};
use wezterm_term::input::MouseButton;
use wezterm_term::SemanticType;

mod launcher;
mod spawn;
pub use launcher::*;
pub use spawn::*;

#[derive(Debug, Copy, Clone, Eq, PartialEq, FromDynamic, ToDynamic)]
pub enum SelectionMode {
    Cell,
    Word,
    Line,
    SemanticZone,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic, Default)]
pub enum Pattern {
    CaseSensitiveString(String),
    CaseInSensitiveString(String),
    Regex(String),
    #[default]
    CurrentSelectionOrEmptyString,
}

impl Pattern {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::CaseSensitiveString(s) | Self::CaseInSensitiveString(s) | Self::Regex(s) => {
                s.is_empty()
            }
            Self::CurrentSelectionOrEmptyString => true,
        }
    }
}

/// A mouse event that can trigger an action
#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Hash, FromDynamic, ToDynamic)]
pub enum MouseEventTrigger {
    /// Mouse button is pressed. streak is how many times in a row
    /// it was pressed.
    Down { streak: usize, button: MouseButton },
    /// Mouse button is held down while the cursor is moving. streak is how many times in a row
    /// it was pressed, with the last of those being held to form the drag.
    Drag { streak: usize, button: MouseButton },
    /// Mouse button is being released. streak is how many times
    /// in a row it was pressed and released.
    Up { streak: usize, button: MouseButton },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, FromDynamic, ToDynamic)]
pub enum PaneDirection {
    Up,
    Down,
    Left,
    Right,
    Next,
    Prev,
}

impl PaneDirection {
    pub fn direction_from_str(arg: &str) -> Result<PaneDirection, String> {
        for candidate in PaneDirection::variants() {
            if candidate.eq_ignore_ascii_case(arg) {
                if let Ok(direction) = PaneDirection::from_dynamic(
                    &Value::String(candidate.to_string()),
                    FromDynamicOptions::default(),
                ) {
                    return Ok(direction);
                }
            }
        }
        Err(format!(
            "invalid direction {arg}, possible values are {:?}",
            PaneDirection::variants()
        ))
    }
}

#[derive(
    Debug, Copy, Clone, PartialEq, Eq, FromDynamic, ToDynamic, Serialize, Deserialize, Default,
)]
pub enum ScrollbackEraseMode {
    #[default]
    ScrollbackOnly,
    ScrollbackAndViewport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic, Default)]
pub enum ClipboardCopyDestination {
    Clipboard,
    PrimarySelection,
    #[default]
    ClipboardAndPrimarySelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic, Default)]
pub enum ClipboardPasteSource {
    #[default]
    Clipboard,
    PrimarySelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic, Default)]
pub enum PaneSelectMode {
    #[default]
    Activate,
    SwapWithActive,
    SwapWithActiveKeepFocus,
    MoveToNewTab,
    MoveToNewWindow,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
pub struct PaneSelectArguments {
    /// Overrides the main quick_select_alphabet config
    #[dynamic(default)]
    pub alphabet: String,

    #[dynamic(default)]
    pub mode: PaneSelectMode,

    #[dynamic(default)]
    pub show_pane_ids: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic, Default)]
pub enum CharSelectGroup {
    RecentlyUsed,
    #[default]
    SmileysAndEmotion,
    PeopleAndBody,
    AnimalsAndNature,
    FoodAndDrink,
    TravelAndPlaces,
    Activities,
    Objects,
    Symbols,
    Flags,
    NerdFonts,
    UnicodeNames,
    ShortCodes,
}

// next is default, previous is the reverse
macro_rules! char_select_group_impl_next_prev {
    ($($x:ident => $y:ident),+ $(,)?) => {
        impl CharSelectGroup {
            pub const fn next(self) -> Self {
                match self {
                    $(CharSelectGroup::$x => CharSelectGroup::$y),+
                }
            }

            pub const fn previous(self) -> Self {
                match self {
                    $(CharSelectGroup::$y => CharSelectGroup::$x),+
                }
            }
        }
    };
}

char_select_group_impl_next_prev! (
    RecentlyUsed => SmileysAndEmotion,
    SmileysAndEmotion => PeopleAndBody,
    PeopleAndBody => AnimalsAndNature,
    AnimalsAndNature => FoodAndDrink,
    FoodAndDrink => TravelAndPlaces,
    TravelAndPlaces => Activities,
    Activities => Objects,
    Objects => Symbols,
    Symbols => Flags,
    Flags => NerdFonts,
    NerdFonts => UnicodeNames,
    UnicodeNames => ShortCodes,
    ShortCodes => RecentlyUsed,
);

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
pub struct CharSelectArguments {
    #[dynamic(default)]
    pub group: Option<CharSelectGroup>,
    #[dynamic(default = "default_true")]
    pub copy_on_select: bool,
    #[dynamic(default)]
    pub copy_to: ClipboardCopyDestination,
}

impl Default for CharSelectArguments {
    fn default() -> Self {
        Self {
            group: None,
            copy_on_select: true,
            copy_to: ClipboardCopyDestination::default(),
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct QuickSelectArguments {
    /// Overrides the main quick_select_alphabet config
    #[dynamic(default)]
    pub alphabet: String,
    /// Overrides the main quick_select_patterns config
    #[dynamic(default)]
    pub patterns: Vec<String>,
    #[dynamic(default)]
    pub action: Option<Box<KeyAssignment>>,
    /// Skip triggering `action` after paste is performed (capital selection)
    #[dynamic(default)]
    pub skip_action_on_paste: bool,
    /// Label to use in place of "copy" when `action` is set
    #[dynamic(default)]
    pub label: String,
    /// How many lines before and how many lines after the viewport to
    /// search to produce the quickselect results
    pub scope_lines: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct PromptInputLine {
    pub action: Box<KeyAssignment>,
    /// Optional label to pre-fill the input line with
    #[dynamic(default)]
    pub initial_value: Option<String>,
    /// Descriptive text to show ahead of prompt
    #[dynamic(default)]
    pub description: String,
    /// Text to show for prompt
    #[dynamic(default = "default_prompt")]
    pub prompt: String,
}

fn default_prompt() -> String {
    "> ".to_string()
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct InputSelectorEntry {
    pub label: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct InputSelector {
    pub action: Box<KeyAssignment>,
    #[dynamic(default)]
    pub title: String,

    pub choices: Vec<InputSelectorEntry>,

    #[dynamic(default)]
    pub fuzzy: bool,

    #[dynamic(default = "default_num_alphabet")]
    pub alphabet: String,

    #[dynamic(default = "default_description")]
    pub description: String,

    #[dynamic(default = "default_fuzzy_description")]
    pub fuzzy_description: String,
}

fn default_num_alphabet() -> String {
    "1234567890abcdefghilmnopqrstuvwxyz".to_string()
}

fn default_description() -> String {
    "Select an item and press Enter = accept,  Esc = cancel,  / = filter".to_string()
}

fn default_fuzzy_description() -> String {
    "Fuzzy matching: ".to_string()
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct Confirmation {
    pub action: Box<KeyAssignment>,
    #[dynamic(default)]
    pub cancel: Option<Box<KeyAssignment>>,
    /// Text to show for confirmation
    #[dynamic(default = "default_message")]
    pub message: String,
}

fn default_message() -> String {
    "🛑 Really continue?".to_string()
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub enum KeyAssignment {
    SpawnTab(SpawnTabDomain),
    SpawnWindow,
    ToggleFullScreen,
    ToggleAlwaysOnTop,
    ToggleAlwaysOnBottom,
    SetWindowLevel(WindowLevel),
    CopyTo(ClipboardCopyDestination),
    CopyTextTo {
        text: String,
        destination: ClipboardCopyDestination,
    },
    PasteFrom(ClipboardPasteSource),
    ActivateTabRelative(isize),
    ActivateTabRelativeNoWrap(isize),
    IncreaseFontSize,
    DecreaseFontSize,
    ResetFontSize,
    ResetFontAndWindowSize,
    ActivateTab(isize),
    ActivateLastTab,
    SendString(String),
    SendKey(KeyNoAction),
    Nop,
    DisableDefaultAssignment,
    Hide,
    Show,
    CloseCurrentTab {
        confirm: bool,
    },
    ReloadConfiguration,
    MoveTabRelative(isize),
    MoveTab(usize),
    /// Prompts for a new title for the active tab (pre-filled with its
    /// current title) and applies it once confirmed. Not itself a prompt
    /// definition -- see `TermWindow::rename_current_tab` and
    /// `show_line_prompt_overlay`'s handling of this variant, which drives
    /// a `PromptInputLine` overlay built with the tab's live current title
    /// (something a static config value can't express) and applies the
    /// entered text via `Tab::set_title` when the prompt completes.
    RenameCurrentTab,
    ScrollByPage(NotNan<f64>),
    ScrollByLine(isize),
    ScrollByCurrentEventWheelDelta,
    ScrollToPrompt(isize),
    ScrollToTop,
    ScrollToBottom,
    ShowTabNavigator,
    ShowDebugOverlay,
    /// Shows a centered overlay with the running OnlyTerm version and
    /// commit hash, and copies that same text to the clipboard. See
    /// `TermWindow::show_version_overlay` and `overlay::show_version_overlay`.
    ShowVersionOverlay,
    /// Open the `.ktav` config file in whatever the OS has associated with
    /// it, creating it from a commented starter template first if the user
    /// doesn't have one yet.
    OpenConfigFile,
    HideApplication,
    QuitApplication,
    SpawnCommandInNewTab(SpawnCommand),
    SpawnCommandInNewWindow(SpawnCommand),
    SplitHorizontal(SpawnCommand),
    SplitVertical(SpawnCommand),
    ShowLauncherArgs(LauncherActionArgs),
    ClearScrollback(ScrollbackEraseMode),
    Search(Pattern),
    ActivateCopyMode,

    SelectTextAtMouseCursor(SelectionMode),
    ExtendSelectionToMouseCursor(SelectionMode),
    OpenLinkAtMouseCursor,
    CopyLinkAtMouseCursor(ClipboardCopyDestination),
    ClearSelection,
    /// If the active pane has a selection, copies it to the clipboard and
    /// clears the selection (matching CopyTo(Clipboard) + ClearSelection).
    /// Otherwise, sends a literal Ctrl+C (0x03) to the pane, exactly as if
    /// Ctrl+C had no other binding at all. This is the default binding for
    /// Ctrl+C: it lets Ctrl+C serve double duty as "copy" when there's a
    /// selection to copy, without ever preventing a plain Ctrl+C from
    /// reaching the running program to interrupt it when there isn't.
    CopySelectionOrInterrupt,
    /// Sends Enter with the given modifier (eg. CTRL or SHIFT) through
    /// whatever keyboard protocol the pane's app has negotiated (kitty or
    /// win32-input-mode), so an app that asked for eg. Kitty's
    /// DISAMBIGUATE_ESCAPE_CODES gets the modified-Enter CSI-u sequence it
    /// expects instead of a plain carriage return. If the app hasn't
    /// negotiated a protocol that can represent a modified Enter, falls
    /// back to sending a literal line feed ('\n'), since that is the best
    /// a plain/legacy app can do with this chord. This is the default
    /// binding for CTRL+Enter and SHIFT+Enter.
    SendEnterOrNewline(Modifiers),
    CompleteSelection(ClipboardCopyDestination),
    CompleteSelectionOrOpenLinkAtMouseCursor(ClipboardCopyDestination),
    StartWindowDrag,

    AdjustPaneSize(PaneDirection, usize),
    ActivatePaneDirection(PaneDirection),
    ActivatePaneByIndex(usize),
    TogglePaneZoomState,
    SetPaneZoomState(bool),
    CloseCurrentPane {
        confirm: bool,
    },
    EmitEvent(String),
    QuickSelect,
    QuickSelectArgs(QuickSelectArguments),

    Multiple(Vec<KeyAssignment>),

    SwitchToWorkspace {
        name: Option<String>,
        spawn: Option<SpawnCommand>,
    },
    SwitchWorkspaceRelative(isize),

    ActivateKeyTable {
        name: String,
        #[dynamic(default)]
        timeout_milliseconds: Option<u64>,
        #[dynamic(default)]
        replace_current: bool,
        #[dynamic(default = "crate::default_true")]
        one_shot: bool,
        #[dynamic(default)]
        until_unknown: bool,
        #[dynamic(default)]
        prevent_fallback: bool,
    },
    PopKeyTable,
    ClearKeyTableStack,
    DetachDomain(SpawnTabDomain),
    AttachDomain(String),

    CopyMode(CopyModeAssignment),
    RotatePanes(RotationDirection),
    SplitPane(SplitPane),
    PaneSelect(PaneSelectArguments),
    CharSelect(CharSelectArguments),

    ResetTerminal,
    OpenUri(String),
    ActivateCommandPalette,
    ActivateWindow(usize),
    ActivateWindowRelative(isize),
    ActivateWindowRelativeNoWrap(isize),
    PromptInputLine(PromptInputLine),
    InputSelector(InputSelector),
    Confirmation(Confirmation),
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct SplitPane {
    pub direction: PaneDirection,
    #[dynamic(default)]
    pub size: SplitSize,
    #[dynamic(default)]
    pub command: SpawnCommand,
    #[dynamic(default)]
    pub top_level: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum SplitSize {
    Cells(usize),
    Percent(u8),
}

impl Default for SplitSize {
    fn default() -> Self {
        Self::Percent(50)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, FromDynamic, ToDynamic)]
pub enum RotationDirection {
    Clockwise,
    CounterClockwise,
}

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum CopyModeAssignment {
    MoveToViewportBottom,
    MoveToViewportTop,
    MoveToViewportMiddle,
    MoveToScrollbackTop,
    MoveToScrollbackBottom,
    SetSelectionMode(Option<SelectionMode>),
    ClearSelectionMode,
    MoveToStartOfLineContent,
    MoveToEndOfLineContent,
    MoveToStartOfLine,
    MoveToStartOfNextLine,
    MoveToSelectionOtherEnd,
    MoveToSelectionOtherEndHoriz,
    MoveBackwardWord,
    MoveForwardWord,
    MoveForwardWordEnd,
    MoveRight,
    MoveLeft,
    MoveUp,
    MoveDown,
    MoveByPage(NotNan<f64>),
    PageUp,
    PageDown,
    Close,
    PriorMatch,
    NextMatch,
    PriorMatchPage,
    NextMatchPage,
    CycleMatchType,
    ClearPattern,
    EditPattern,
    AcceptPattern,
    MoveBackwardSemanticZone,
    MoveForwardSemanticZone,
    MoveBackwardZoneOfType(SemanticType),
    MoveForwardZoneOfType(SemanticType),
    JumpForward { prev_char: bool },
    JumpBackward { prev_char: bool },
    JumpAgain,
    JumpReverse,
}

pub type KeyTable = HashMap<(KeyCode, Modifiers), KeyTableEntry>;

#[derive(Debug, Clone, Default)]
pub struct KeyTables {
    pub default: KeyTable,
    pub by_name: HashMap<String, KeyTable>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyTableEntry {
    pub action: KeyAssignment,
}
