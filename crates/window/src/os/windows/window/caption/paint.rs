use super::*;
use winapi::um::wingdi::*;

// winuser.h: DI_IMAGE | DI_MASK (omitted from winapi 0.3.9).
const DI_NORMAL: UINT = 0x0003;

pub(super) struct CaptionPaint {
    pub title_wide: Vec<u16>,
    pub status_wide: Vec<Vec<u16>>,
    pub font: Option<CaptionFont>,
    pub appearance: Appearance,
}

impl Default for CaptionPaint {
    fn default() -> Self {
        Self {
            title_wide: Vec::new(),
            status_wide: Vec::new(),
            font: None,
            appearance: Appearance::Light,
        }
    }
}

pub(super) struct CaptionFont {
    handle: HFONT,
    dpi: u32,
}

impl Drop for CaptionFont {
    fn drop(&mut self) {
        // SAFETY: exclusively owned CreateFontIndirectW result, restored out of every DC.
        unsafe {
            DeleteObject(self.handle as _);
        }
    }
}

pub(super) fn caption_text(text: &str) -> Vec<u16> {
    let mut chars = text.chars();
    let mut result = Vec::new();
    let mut units = [0; 2];
    for ch in chars.by_ref().take(1024) {
        result.extend_from_slice(ch.encode_utf16(&mut units));
    }
    if chars.next().is_some() {
        result.push(0x2026);
    }
    result
}

struct PaintDc {
    hwnd: HWND,
    dc: HDC,
    paint: PAINTSTRUCT,
}

impl Drop for PaintDc {
    fn drop(&mut self) {
        // SAFETY: pairs the single BeginPaint for this live WM_PAINT invocation.
        unsafe {
            EndPaint(self.hwnd, &self.paint);
        }
    }
}

struct SavedDc(HDC, i32);

impl Drop for SavedDc {
    fn drop(&mut self) {
        // SAFETY: restore this DC's saved state before the font can be replaced/dropped.
        unsafe {
            RestoreDC(self.0, self.1);
        }
    }
}

/// # Safety
/// hwnd must be the live child receiving WM_PAINT, on its owning GUI thread.
pub(super) unsafe fn paint_child(hwnd: HWND) {
    log::trace!("caption paint hwnd={:?}", hwnd);
    // SAFETY: PAINTSTRUCT has only integer/handle fields and permits zero initialization.
    let mut paint: PAINTSTRUCT = unsafe { std::mem::zeroed() };
    // SAFETY: live WM_PAINT window and initialized output buffer; guard pairs EndPaint.
    let dc = unsafe { BeginPaint(hwnd, &mut paint) };
    let paint_dc = PaintDc { hwnd, dc, paint };
    if paint_dc.dc.is_null() {
        return;
    }
    // SAFETY: this is the live DC obtained above and retained by paint_dc.
    unsafe {
        paint_in_dc(hwnd, dc);
    }
}

/// # Safety
/// hwnd and dc must be live on the owning thread, with child-client coordinates.
pub(super) unsafe fn paint_in_dc(hwnd: HWND, dc: HDC) {
    // SAFETY: child has a live parent during WM_PAINT; no ownership transfers.
    let parent = unsafe { GetParent(hwnd) };
    let Some(state) = PAINT_STATES.with(|states| states.borrow().get(&(hwnd as usize)).cloned())
    else {
        return;
    };
    let mut state = state.borrow_mut();
    let mut rect = RECT::default();
    // SAFETY: initialized output RECT for this live caption child.
    unsafe {
        GetClientRect(hwnd, &mut rect);
    }
    if rect.right <= 0 || rect.bottom <= 0 {
        return;
    }
    // SAFETY: private same-thread bitmap, sized to the child's client area.
    let Some(surface) = (unsafe { surface::Surface::new(dc, rect.right, rect.bottom) }) else {
        if let Some(inner) = rc_from_hwnd(parent) {
            promise::spawn::spawn(async move {
                inner.borrow_mut().caption_failed();
            })
            .detach();
        }
        return;
    };
    // SAFETY: DC and both windows stay live during this call on the same GUI thread.
    unsafe {
        state.paint(hwnd, parent, surface.dc);
        surface.present(dc);
    }
}

impl CaptionPaint {
    /// # Safety
    /// Both windows and dc must be live on their owning thread, throughout the call.
    unsafe fn paint(&mut self, child: HWND, parent: HWND, dc: HDC) {
        let appearance = self.appearance;
        // SAFETY: scalar DPI query on the live caption parent.
        let dpi = unsafe { GetDpiForWindow(parent) }.max(96);
        if self.font.as_ref().map(|font| font.dpi) != Some(dpi) {
            // SAFETY: NONCLIENTMETRICSW is a C struct of scalar fields, sized before use.
            let mut metrics: NONCLIENTMETRICSW = unsafe { std::mem::zeroed() };
            metrics.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
            // SAFETY: SPI_GETNONCLIENTMETRICS writes this exact struct, in this window's DPI.
            if unsafe {
                SystemParametersInfoForDpi(
                    SPI_GETNONCLIENTMETRICS,
                    metrics.cbSize,
                    &mut metrics as *mut _ as _,
                    0,
                    dpi,
                )
            } != FALSE
            {
                // At least 12pt at the window's DPI, without shrinking an accessibility font.
                metrics.lfCaptionFont.lfHeight = -metrics
                    .lfCaptionFont
                    .lfHeight
                    .saturating_abs()
                    .max((dpi.saturating_mul(16) / 96) as i32);
                // SAFETY: CreateFontIndirectW copies this initialized LOGFONT; we own its result.
                let font = unsafe { CreateFontIndirectW(&metrics.lfCaptionFont) };
                if !font.is_null() {
                    self.font = Some(CaptionFont { handle: font, dpi });
                }
            }
        }
        // SAFETY: dc is a live paint DC. A successful SaveDC is paired by SavedDc.
        let saved = unsafe { SaveDC(dc) };
        if saved == 0 {
            return;
        }
        let _restore = SavedDc(dc, saved);
        // SAFETY: GDI objects are used exclusively on the GUI thread; stock brush is borrowed.
        unsafe {
            if let Some(font) = &self.font {
                SelectObject(dc, font.handle as _);
            }
            let mut rect = RECT::default();
            GetClientRect(child, &mut rect);
            let dark = matches!(appearance, Appearance::Dark | Appearance::DarkHighContrast);
            let high_contrast = matches!(
                appearance,
                Appearance::LightHighContrast | Appearance::DarkHighContrast
            );
            let background = if high_contrast {
                GetSysColor(COLOR_WINDOW)
            } else if dark {
                RGB(32, 32, 32)
            } else {
                RGB(255, 255, 255)
            };
            let foreground = if high_contrast {
                GetSysColor(COLOR_WINDOWTEXT)
            } else if GetForegroundWindow() == parent {
                if dark {
                    RGB(240, 240, 240)
                } else {
                    RGB(24, 24, 24)
                }
            } else if dark {
                RGB(160, 160, 160)
            } else {
                RGB(110, 110, 110)
            };
            SetDCBrushColor(dc, background);
            FillRect(dc, &rect, GetStockObject(DC_BRUSH as i32) as _);
            let (_, _, text_top) = layout::caption_insets(
                rect.bottom,
                GetSystemMetricsForDpi(SM_CYCAPTION, dpi),
                IsZoomed(parent) != FALSE,
            );
            rect.top = text_top;
            SetTextColor(dc, foreground);
            SetBkMode(dc, TRANSPARENT as i32);

            let margin = (4 * dpi / 96) as i32;
            let icon_size = GetSystemMetricsForDpi(SM_CXSMICON, dpi);
            let icon = GetClassLongPtrW(parent, GCLP_HICON) as HICON;
            if !icon.is_null() && rect.right >= icon_size + margin * 2 {
                DrawIconEx(
                    dc,
                    margin,
                    (rect.top + rect.bottom - icon_size) / 2,
                    icon,
                    icon_size,
                    icon_size,
                    0,
                    null_mut(),
                    DI_NORMAL,
                );
            }
            let mut outer = RECT::default();
            GetWindowRect(parent, &mut outer);
            let mut origin = POINT { x: 0, y: 0 };
            ClientToScreen(child, &mut origin);
            let widths: Vec<_> = self
                .status_wide
                .iter()
                .map(|text| measure(dc, text).unwrap_or(i32::MAX).saturating_add(2))
                .collect();
            let layout = layout::layout(
                outer.right - outer.left,
                origin.x - outer.left,
                icon_size + margin * 3,
                rect.right - margin,
                &widths,
                margin * 2,
            );
            let mut title_rect = RECT {
                left: layout.title.start,
                right: layout.title.end,
                ..rect
            };
            draw(dc, &self.title_wide, &mut title_rect, DT_END_ELLIPSIS);
            if let Some((index, range)) = layout.status {
                let mut status_rect = RECT {
                    left: range.start,
                    right: range.end,
                    ..rect
                };
                draw(dc, &self.status_wide[index], &mut status_rect, 0);
            }
        }
    }
}

/// # Safety
/// dc must be valid with the drawing font selected; text is borrowed only for this call.
unsafe fn measure(dc: HDC, text: &[u16]) -> Option<i32> {
    let count = text.len().try_into().ok()?;
    let mut size = SIZE { cx: 0, cy: 0 };
    // SAFETY: live DC, initialized UTF-16 slice and sized output structure.
    if unsafe { GetTextExtentPoint32W(dc, text.as_ptr(), count, &mut size) } == FALSE {
        None
    } else {
        Some(size.cx)
    }
}

/// # Safety
/// dc must be a live paint DC, rect initialized, and no DT_MODIFYSTRING flag is allowed.
pub(super) unsafe fn draw(dc: HDC, text: &[u16], rect: &mut RECT, flags: u32) {
    if text.is_empty() || rect.right <= rect.left {
        return;
    }
    let Ok(count) = text.len().try_into() else {
        return;
    };
    // SAFETY: DrawTextW does not mutate the buffer without DT_MODIFYSTRING; explicit UTF-16 count.
    unsafe {
        DrawTextW(
            dc,
            text.as_ptr(),
            count,
            rect,
            flags | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_unicode_title_is_bounded_without_splitting_surrogates() {
        let text = caption_text(&"😀".repeat(2000));
        assert_eq!(text.len(), 2049);
        let decoded = String::from_utf16(&text).unwrap();
        assert!(decoded.ends_with('…'));
        assert_eq!(decoded.chars().count(), 1025);
    }
}
