use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Window {
    DeIconify,
    Iconify,
    MoveWindow {
        x: i64,
        y: i64,
    },
    ResizeWindowPixels {
        width: Option<i64>,
        height: Option<i64>,
    },
    RaiseWindow,
    LowerWindow,
    RefreshWindow,
    ResizeWindowCells {
        width: Option<i64>,
        height: Option<i64>,
    },
    RestoreMaximizedWindow,
    MaximizeWindow,
    MaximizeWindowVertically,
    MaximizeWindowHorizontally,
    UndoFullScreenMode,
    ChangeToFullScreenMode,
    ToggleFullScreen,
    ReportWindowState,
    ReportWindowPosition,
    ReportTextAreaPosition,
    ReportTextAreaSizePixels,
    ReportWindowSizePixels,
    ReportScreenSizePixels,
    ReportCellSizePixels,
    ReportCellSizePixelsResponse {
        width: Option<i64>,
        height: Option<i64>,
    },
    ReportTextAreaSizeCells,
    ReportScreenSizeCells,
    ReportIconLabel,
    ReportWindowTitle,
    PushIconAndWindowTitle,
    PushIconTitle,
    PushWindowTitle,
    PopIconAndWindowTitle,
    PopIconTitle,
    PopWindowTitle,
    /// DECRQCRA; used by esctest
    ChecksumRectangularArea {
        request_id: i64,
        page_number: i64,
        top: OneBased,
        left: OneBased,
        bottom: OneBased,
        right: OneBased,
    },
}

fn numstr_or_empty(x: &Option<i64>) -> String {
    match x {
        Some(x) => format!("{}", x),
        None => "".to_owned(),
    }
}

impl Display for Window {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        match self {
            Window::DeIconify => write!(f, "1t"),
            Window::Iconify => write!(f, "2t"),
            Window::MoveWindow { x, y } => write!(f, "3;{};{}t", x, y),
            Window::ResizeWindowPixels { width, height } => write!(
                f,
                "4;{};{}t",
                numstr_or_empty(height),
                numstr_or_empty(width),
            ),
            Window::RaiseWindow => write!(f, "5t"),
            Window::LowerWindow => write!(f, "6t"),
            Window::RefreshWindow => write!(f, "7t"),
            Window::ResizeWindowCells { width, height } => write!(
                f,
                "8;{};{}t",
                numstr_or_empty(height),
                numstr_or_empty(width),
            ),
            Window::RestoreMaximizedWindow => write!(f, "9;0t"),
            Window::MaximizeWindow => write!(f, "9;1t"),
            Window::MaximizeWindowVertically => write!(f, "9;2t"),
            Window::MaximizeWindowHorizontally => write!(f, "9;3t"),
            Window::UndoFullScreenMode => write!(f, "10;0t"),
            Window::ChangeToFullScreenMode => write!(f, "10;1t"),
            Window::ToggleFullScreen => write!(f, "10;2t"),
            Window::ReportWindowState => write!(f, "11t"),
            Window::ReportWindowPosition => write!(f, "13t"),
            Window::ReportTextAreaPosition => write!(f, "13;2t"),
            Window::ReportTextAreaSizePixels => write!(f, "14t"),
            Window::ReportWindowSizePixels => write!(f, "14;2t"),
            Window::ReportScreenSizePixels => write!(f, "15t"),
            Window::ReportCellSizePixels => write!(f, "16t"),
            Window::ReportCellSizePixelsResponse { width, height } => write!(
                f,
                "6;{};{}t",
                numstr_or_empty(height),
                numstr_or_empty(width),
            ),
            Window::ReportTextAreaSizeCells => write!(f, "18t"),
            Window::ReportScreenSizeCells => write!(f, "19t"),
            Window::ReportIconLabel => write!(f, "20t"),
            Window::ReportWindowTitle => write!(f, "21t"),
            Window::PushIconAndWindowTitle => write!(f, "22;0t"),
            Window::PushIconTitle => write!(f, "22;1t"),
            Window::PushWindowTitle => write!(f, "22;2t"),
            Window::PopIconAndWindowTitle => write!(f, "23;0t"),
            Window::PopIconTitle => write!(f, "23;1t"),
            Window::PopWindowTitle => write!(f, "23;2t"),
            Window::ChecksumRectangularArea {
                request_id,
                page_number,
                top,
                left,
                bottom,
                right,
            } => write!(
                f,
                "{};{};{};{};{};{}*y",
                request_id, page_number, top, left, bottom, right,
            ),
        }
    }
}
