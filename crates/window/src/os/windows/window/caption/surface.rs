//! Opaque 32-bit backing surface for a caption over DWM's extended frame.
use super::*;
use std::convert::TryFrom;
use winapi::um::wingdi::*;

pub(super) struct Surface {
    pub dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    pixels: *mut u32,
    count: usize,
    width: i32,
    height: i32,
}

impl Surface {
    /// # Safety
    /// target must be null (screen-compatible) or a live same-thread DC.
    /// GDI owns the allocation until Drop.
    pub unsafe fn new(target: HDC, width: i32, height: i32) -> Option<Self> {
        let count = usize::try_from(width)
            .ok()?
            .checked_mul(usize::try_from(height).ok()?)?;
        let bytes = count.checked_mul(4)?;
        if bytes == 0 || bytes > 32 * 1024 * 1024 {
            return None;
        }
        // SAFETY: BITMAPINFO is a C struct of scalar fields; every header field is sized below.
        let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        // SAFETY: create a private DC and top-down 32-bit DIB; no GDI objects are shared.
        unsafe {
            let dc = CreateCompatibleDC(target);
            if dc.is_null() {
                return None;
            }
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() {
                DeleteDC(dc);
                return None;
            }
            let previous = SelectObject(dc, bitmap as _);
            if previous.is_null() || std::ptr::eq(previous, HGDI_ERROR) {
                DeleteObject(bitmap as _);
                DeleteDC(dc);
                return None;
            }
            // CreateDIBSection allocates width*height DWORD pixels (no padding at 32 bpp).
            std::ptr::write_bytes(bits.cast::<u8>(), 0, bytes);
            Some(Self {
                dc,
                bitmap,
                previous,
                pixels: bits.cast(),
                count,
                width,
                height,
            })
        }
    }

    /// # Safety
    /// target is live; only this GUI thread may draw into or access this private DIB.
    pub unsafe fn present(&self, target: HDC) {
        // SAFETY: flush this thread's GDI batch before accessing the DIB allocation.
        // CreateDIBSection documents this synchronization requirement.
        unsafe {
            GdiFlush();
            // GDI text leaves alpha unset. DWM requires opaque alpha for this solid band.
            // pixels points to count initialized, DWORD-aligned pixels in the owned DIB.
            for index in 0..self.count {
                *self.pixels.add(index) |= 0xff00_0000;
            }
            BitBlt(
                target,
                0,
                0,
                self.width,
                self.height,
                self.dc,
                0,
                0,
                SRCCOPY,
            );
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: deselect the exclusively owned bitmap before releasing it and its DC.
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap as _);
            DeleteDC(self.dc);
        }
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    #[test]
    fn child_paints_without_borrowing_parent_state_and_releases_registry_entry() {
        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                // SAFETY: the hidden parent and its child belong to this test thread.
                unsafe {
                    DestroyWindow(self.0);
                }
            }
        }
        // SAFETY: private hidden HWNDs and DIBs; no application window is accessed.
        unsafe {
            let class = wide_string("STATIC");
            let parent = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                400,
                200,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            );
            assert!(!parent.is_null());
            let window = TestWindow(parent);
            assert_eq!(
                GetWindowLongW(parent, GWL_STYLE) as u32 & WS_CLIPCHILDREN,
                0
            );
            assert!(super::super::set_child_clipping(parent, true));
            assert!(super::super::set_child_clipping(parent, true));
            assert_ne!(
                GetWindowLongW(parent, GWL_STYLE) as u32 & WS_CLIPCHILDREN,
                0
            );
            assert!(super::super::set_child_clipping(parent, false));
            assert_eq!(
                GetWindowLongW(parent, GWL_STYLE) as u32 & WS_CLIPCHILDREN,
                0
            );
            SetWindowLongW(
                parent,
                GWL_STYLE,
                GetWindowLongW(parent, GWL_STYLE) | WS_CLIPCHILDREN as i32,
            );
            assert!(super::super::set_child_clipping(parent, true));
            assert!(super::super::set_child_clipping(parent, false));
            assert_ne!(
                GetWindowLongW(parent, GWL_STYLE) as u32 & WS_CLIPCHILDREN,
                0
            );
            let state = Rc::new(RefCell::new(super::super::paint::CaptionPaint::default()));
            state.borrow_mut().title_wide = "TITLE".encode_utf16().collect();
            let child = super::super::create_child(parent, Rc::clone(&state)).unwrap();
            assert_ne!(
                SetWindowPos(
                    child,
                    null_mut(),
                    0,
                    0,
                    260,
                    31,
                    SWP_NOACTIVATE | SWP_NOZORDER
                ),
                0
            );
            assert!(rc_from_hwnd(parent).is_none());
            let target = Surface::new(null_mut(), 260, 31).unwrap();
            super::super::paint::paint_in_dc(child, target.dc);
            GdiFlush();
            let mut glyph_pixels = 0;
            for index in 0..target.count {
                let pixel = *target.pixels.add(index);
                assert_eq!(pixel >> 24, 255);
                if pixel & 0x00ff_ffff != 0x00ff_ffff {
                    glyph_pixels += 1;
                }
            }
            assert!(
                glyph_pixels > 0,
                "child must paint even without parent Rust state"
            );
            assert_eq!(Rc::strong_count(&state), 2);
            drop(window);
            assert_eq!(Rc::strong_count(&state), 1);
            PAINT_STATES.with(|states| assert!(!states.borrow().contains_key(&(child as usize))));
        }
    }

    #[test]
    fn native_buttons_get_transparent_background_without_erasing_content() {
        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                // SAFETY: this test owns the hidden window on the creating thread.
                unsafe {
                    DestroyWindow(self.0);
                }
            }
        }
        // SAFETY: private hidden HWND and DIB, all accessed and released on this thread.
        unsafe {
            let class = wide_string("STATIC");
            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                320,
                200,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            );
            assert!(!hwnd.is_null());
            let _window = TestWindow(hwnd);
            // Fixture: an active caption band, without needing DWM composition in CI.
            assert_ne!(
                SetPropA(hwnd, super::super::ENABLED.as_ptr().cast(), 1usize as _),
                0
            );
            assert_ne!(
                SetPropA(hwnd, super::super::HEIGHT.as_ptr().cast(), 31usize as _),
                0
            );
            let mut client = RECT::default();
            assert_ne!(GetClientRect(hwnd, &mut client), 0);
            assert!(client.bottom > 31);
            let surface = Surface::new(null_mut(), client.right, client.bottom).unwrap();
            SetDCBrushColor(surface.dc, RGB(255, 255, 255));
            assert_ne!(
                FillRect(surface.dc, &client, GetStockObject(DC_BRUSH as i32) as _),
                0
            );
            super::super::clear_frame_background(hwnd, surface.dc);
            GdiFlush();
            assert_eq!(
                *surface.pixels, 0,
                "frame must be transparent premultiplied black, not white"
            );
            let body = 31 * surface.width as usize;
            assert_eq!(
                *surface.pixels.add(body) & 0x00ff_ffff,
                0x00ff_ffff,
                "caption clearing must not touch terminal content"
            );
            assert_ne!(
                SetPropA(
                    hwnd,
                    super::super::LABEL_WIDTH.as_ptr().cast(),
                    100usize as _
                ),
                0
            );
            FillRect(surface.dc, &client, GetStockObject(DC_BRUSH as i32) as _);
            super::super::clear_frame_background(hwnd, surface.dc);
            GdiFlush();
            assert_eq!(
                *surface.pixels & 0x00ff_ffff,
                0x00ff_ffff,
                "parent must not erase the caption child's labels"
            );
            assert_eq!(
                *surface.pixels.add(100),
                0,
                "buttons still need transparent black"
            );

            // Hidden windows have no update region; show the fixture outside all monitors.
            assert_ne!(
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    GetSystemMetrics(SM_XVIRTUALSCREEN)
                        + GetSystemMetrics(SM_CXVIRTUALSCREEN)
                        + 100,
                    GetSystemMetrics(SM_YVIRTUALSCREEN),
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                ),
                0
            );
            ValidateRect(hwnd, null());
            super::super::invalidate_content(hwnd);
            let mut update = RECT::default();
            assert_ne!(GetUpdateRect(hwnd, &mut update, FALSE), 0);
            assert_eq!(
                update.top, 31,
                "terminal invalidation must exclude the caption"
            );
            assert_eq!(update.bottom, client.bottom);
        }
    }

    #[test]
    fn caption_glyphs_survive_the_opaque_dwm_transfer() {
        // SAFETY: both private DIBs are created, painted, inspected and dropped on this thread.
        unsafe {
            let target = Surface::new(null_mut(), 240, 24).expect("caption test DIB");
            let source = Surface::new(target.dc, 240, 24).expect("caption drawing DIB");
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 240,
                bottom: 24,
            };
            SetDCBrushColor(source.dc, RGB(255, 255, 255));
            assert_ne!(
                FillRect(source.dc, &rect, GetStockObject(DC_BRUSH as i32) as _),
                0
            );
            SetTextColor(source.dc, RGB(0, 0, 0));
            SetBkMode(source.dc, TRANSPARENT as i32);
            let text: Vec<_> = "TITLE [CPU 1%]".encode_utf16().collect();
            let mut prefix = SIZE { cx: 0, cy: 0 };
            assert_ne!(
                GetTextExtentPoint32W(
                    source.dc,
                    text.as_ptr(),
                    (text.len() - 1) as i32,
                    &mut prefix
                ),
                0
            );
            super::super::paint::draw(source.dc, &text, &mut rect, 0);
            source.present(target.dc);
            GdiFlush();
            let mut glyph_pixels = 0;
            let mut closing_bracket_pixels = 0;
            for index in 0..target.count {
                // Initialized 32-bit pixels in this DIB; all GDI writes have completed.
                let pixel = *target.pixels.add(index);
                assert_eq!(
                    pixel >> 24,
                    255,
                    "DWM must not discard glyph pixels as transparent"
                );
                if pixel & 0x00ff_ffff != 0x00ff_ffff {
                    glyph_pixels += 1;
                    if index % target.width as usize >= prefix.cx as usize {
                        closing_bracket_pixels += 1;
                    }
                }
            }
            assert!(
                glyph_pixels > 0,
                "successful DrawText must produce actual visible pixels"
            );
            assert!(
                closing_bracket_pixels > 0,
                "the closing bracket must survive the transfer too"
            );
        }
    }
}
