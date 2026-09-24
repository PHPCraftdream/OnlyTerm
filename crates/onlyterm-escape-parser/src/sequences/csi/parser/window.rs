use super::*;

impl<'a> CSIParser<'a> {
    pub(super) fn window(&mut self, params: &'a [CsiParam]) -> Result<Window, ()> {
        let params = Cracked::parse(params)?;

        let p = params.int(0)?;
        let arg1 = params.opt_int(1);
        let arg2 = params.opt_int(2);

        match p {
            1 => Ok(Window::DeIconify),
            2 => Ok(Window::Iconify),
            3 => Ok(Window::MoveWindow {
                x: arg1.unwrap_or(0),
                y: arg2.unwrap_or(0),
            }),
            4 => Ok(Window::ResizeWindowPixels {
                height: arg1,
                width: arg2,
            }),
            5 => Ok(Window::RaiseWindow),
            6 => match params.len() {
                1 => Ok(Window::LowerWindow),
                _ => Ok(Window::ReportCellSizePixelsResponse {
                    height: arg1,
                    width: arg2,
                }),
            },
            7 => Ok(Window::RefreshWindow),
            8 => Ok(Window::ResizeWindowCells {
                height: arg1,
                width: arg2,
            }),
            9 => match arg1 {
                Some(0) => Ok(Window::RestoreMaximizedWindow),
                Some(1) => Ok(Window::MaximizeWindow),
                Some(2) => Ok(Window::MaximizeWindowVertically),
                Some(3) => Ok(Window::MaximizeWindowHorizontally),
                _ => Err(()),
            },
            10 => match arg1 {
                Some(0) => Ok(Window::UndoFullScreenMode),
                Some(1) => Ok(Window::ChangeToFullScreenMode),
                Some(2) => Ok(Window::ToggleFullScreen),
                _ => Err(()),
            },
            11 => Ok(Window::ReportWindowState),
            13 => match arg1 {
                None => Ok(Window::ReportWindowPosition),
                Some(2) => Ok(Window::ReportTextAreaPosition),
                _ => Err(()),
            },
            14 => match arg1 {
                None => Ok(Window::ReportTextAreaSizePixels),
                Some(2) => Ok(Window::ReportWindowSizePixels),
                _ => Err(()),
            },
            15 => Ok(Window::ReportScreenSizePixels),
            16 => Ok(Window::ReportCellSizePixels),
            18 => Ok(Window::ReportTextAreaSizeCells),
            19 => Ok(Window::ReportScreenSizeCells),
            20 => Ok(Window::ReportIconLabel),
            21 => Ok(Window::ReportWindowTitle),
            22 => match arg1 {
                Some(0) => Ok(Window::PushIconAndWindowTitle),
                Some(1) => Ok(Window::PushIconTitle),
                Some(2) => Ok(Window::PushWindowTitle),
                _ => Err(()),
            },
            23 => match arg1 {
                Some(0) => Ok(Window::PopIconAndWindowTitle),
                Some(1) => Ok(Window::PopIconTitle),
                Some(2) => Ok(Window::PopWindowTitle),
                _ => Err(()),
            },
            _ => Err(()),
        }
    }
}
