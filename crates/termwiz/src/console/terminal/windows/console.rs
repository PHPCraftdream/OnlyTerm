use crate::render::RenderTty;
use crate::{bail, ensure, Result};
use filedescriptor::{FileDescriptor, OwnedHandle};
use std::cmp::{max, min};
use std::io::{Error as IoError, Read, Result as IoResult, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::{mem, ptr};
use winapi::um::consoleapi;
use winapi::um::synchapi::{CreateEventW, SetEvent};
use winapi::um::wincon::{
    FillConsoleOutputAttribute, FillConsoleOutputCharacterW, GetConsoleScreenBufferInfo,
    ReadConsoleOutputW, ScrollConsoleScreenBufferW, SetConsoleCP, SetConsoleCursorPosition,
    SetConsoleOutputCP, SetConsoleTextAttribute, SetConsoleWindowInfo, WriteConsoleOutputW,
    CHAR_INFO, CONSOLE_SCREEN_BUFFER_INFO, COORD, INPUT_RECORD, SMALL_RECT,
};

const BUF_SIZE: usize = 128;

pub trait ConsoleInputHandle {
    fn set_input_mode(&mut self, mode: u32) -> Result<()>;
    fn get_input_mode(&mut self) -> Result<u32>;
    fn set_input_cp(&mut self, cp: u32) -> Result<()>;
    fn get_input_cp(&mut self) -> u32;
    fn get_number_of_input_events(&mut self) -> Result<usize>;
    fn read_console_input(&mut self, num_events: usize) -> Result<Vec<INPUT_RECORD>>;
}

pub trait ConsoleOutputHandle {
    fn set_output_mode(&mut self, mode: u32) -> Result<()>;
    fn get_output_mode(&mut self) -> Result<u32>;
    fn set_output_cp(&mut self, cp: u32) -> Result<()>;
    fn get_output_cp(&mut self) -> u32;
    fn fill_char(&mut self, text: char, x: i16, y: i16, len: u32) -> Result<u32>;
    fn fill_attr(&mut self, attr: u16, x: i16, y: i16, len: u32) -> Result<u32>;
    fn set_attr(&mut self, attr: u16) -> Result<()>;
    fn set_cursor_position(&mut self, x: i16, y: i16) -> Result<()>;
    fn get_buffer_info(&mut self) -> Result<CONSOLE_SCREEN_BUFFER_INFO>;
    fn get_buffer_contents(&mut self) -> Result<Vec<CHAR_INFO>>;
    fn set_buffer_contents(&mut self, buffer: &[CHAR_INFO]) -> Result<()>;
    fn set_viewport(&mut self, left: i16, top: i16, right: i16, bottom: i16) -> Result<()>;
    // Mirrors the Windows Console API ScrollConsoleScreenBufferW, which requires
    // all of these parameters; there is no natural grouping that reduces the count.
    #[allow(clippy::too_many_arguments)]
    fn scroll_region(
        &mut self,
        left: i16,
        top: i16,
        right: i16,
        bottom: i16,
        dx: i16,
        dy: i16,
        attr: u16,
    ) -> Result<()>;
}

pub(super) struct InputHandle {
    pub(super) handle: FileDescriptor,
}

impl Read for InputHandle {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        self.handle.read(buf)
    }
}

impl ConsoleInputHandle for InputHandle {
    fn set_input_mode(&mut self, mode: u32) -> Result<()> {
        // SAFETY: `self.handle` is a valid owned console input handle (kept
        // alive by the FileDescriptor for the lifetime of self); `mode` is a
        // plain u32. SetConsoleMode has no other preconditions.
        if unsafe { consoleapi::SetConsoleMode(self.handle.as_raw_handle() as *mut _, mode) } == 0 {
            bail!("SetConsoleMode failed: {}", IoError::last_os_error());
        }
        Ok(())
    }

    fn get_input_mode(&mut self) -> Result<u32> {
        let mut mode = 0;
        // SAFETY: `self.handle` is a valid owned console input handle; `mode`
        // is a valid u32 out-pointer live for the duration of the call.
        if unsafe { consoleapi::GetConsoleMode(self.handle.as_raw_handle() as *mut _, &mut mode) }
            == 0
        {
            bail!("GetConsoleMode failed: {}", IoError::last_os_error());
        }
        Ok(mode)
    }

    fn set_input_cp(&mut self, cp: u32) -> Result<()> {
        // SAFETY: SetConsoleCP takes a plain u32 code page id; no pointers.
        if unsafe { SetConsoleCP(cp) } == 0 {
            bail!("SetConsoleCP failed: {}", IoError::last_os_error());
        }
        Ok(())
    }

    fn get_input_cp(&mut self) -> u32 {
        // SAFETY: GetConsoleCP takes no arguments and has no preconditions.
        unsafe { consoleapi::GetConsoleCP() }
    }

    fn get_number_of_input_events(&mut self) -> Result<usize> {
        let mut num = 0;
        // SAFETY: `self.handle` is a valid owned console input handle; `num`
        // is a valid u32 out-pointer live for the duration of the call.
        if unsafe {
            consoleapi::GetNumberOfConsoleInputEvents(
                self.handle.as_raw_handle() as *mut _,
                &mut num,
            )
        } == 0
        {
            bail!(
                "GetNumberOfConsoleInputEvents failed: {}",
                IoError::last_os_error()
            );
        }
        Ok(num as usize)
    }

    fn read_console_input(&mut self, num_events: usize) -> Result<Vec<INPUT_RECORD>> {
        let mut res = Vec::with_capacity(num_events);
        // SAFETY: INPUT_RECORD is a plain win32 POD struct; an all-zero bit
        // pattern is a valid value (zeroed EventType + zeroed event union).
        let empty_record: INPUT_RECORD = unsafe { mem::zeroed() };
        res.resize(num_events, empty_record);

        let mut num = 0;

        // SAFETY: `self.handle` is a valid owned console input handle. `res`
        // owns `num_events` initialized INPUT_RECORD slots and `as_mut_ptr`/
        // `num_events` describe exactly that buffer; `num` is a valid
        // out-pointer. The call fills at most `num_events` records.
        if unsafe {
            consoleapi::ReadConsoleInputW(
                self.handle.as_raw_handle() as *mut _,
                res.as_mut_ptr(),
                num_events as u32,
                &mut num,
            )
        } == 0
        {
            bail!("ReadConsoleInput failed: {}", IoError::last_os_error());
        }

        // SAFETY: ReadConsoleInputW reported `num` records written (and num
        // <= num_events), into `res` whose capacity is num_events, so `num` is
        // a valid in-bounds length and every slot is initialized.
        unsafe { res.set_len(num as usize) };
        Ok(res)
    }
}

pub(super) struct OutputHandle {
    pub(super) handle: FileDescriptor,
    write_buffer: Vec<u8>,
}

impl OutputHandle {
    pub(super) fn new(handle: FileDescriptor) -> Self {
        Self {
            handle,
            write_buffer: Vec::with_capacity(BUF_SIZE),
        }
    }
}

pub(super) fn dimensions_from_buffer_info(info: CONSOLE_SCREEN_BUFFER_INFO) -> (usize, usize) {
    let cols = 1 + (info.srWindow.Right - info.srWindow.Left);
    let rows = 1 + (info.srWindow.Bottom - info.srWindow.Top);
    (cols as usize, rows as usize)
}

impl RenderTty for OutputHandle {
    fn get_size_in_cells(&mut self) -> Result<(usize, usize)> {
        let info = self.get_buffer_info()?;
        let (cols, rows) = dimensions_from_buffer_info(info);

        Ok((cols, rows))
    }
}

pub(super) struct EventHandle {
    pub(super) handle: OwnedHandle,
}

impl EventHandle {
    pub(super) fn new() -> IoResult<Self> {
        // SAFETY: standard win32 call; both pointer args are explicitly NULL
        // (manual-reset event, unnamed object). Returns a valid HANDLE or NULL,
        // which is checked immediately below.
        let handle = unsafe { CreateEventW(ptr::null_mut(), 0, 0, ptr::null_mut()) };
        if handle.is_null() {
            Err(IoError::last_os_error())
        } else {
            Ok(Self {
                // SAFETY: `handle` is a valid, owned, non-null event HANDLE
                // freshly created by CreateEventW above and not aliased
                // anywhere else, so transferring sole ownership is sound.
                handle: unsafe { OwnedHandle::from_raw_handle(handle as *mut _) },
            })
        }
    }

    pub(super) fn set(&self) -> IoResult<()> {
        // SAFETY: `self.handle` is a valid owned event HANDLE. SetEvent has no
        // other preconditions and is documented to be thread-safe.
        let ok = unsafe { SetEvent(self.handle.as_raw_handle() as *mut _) };
        if ok == 0 {
            Err(IoError::last_os_error())
        } else {
            Ok(())
        }
    }
}

// SAFETY: EventHandle only wraps a CreateEventW HANDLE and exposes `set`
// (which calls the thread-safe SetEvent) plus as_raw_handle reads. The handle
// is immutable after construction, so sharing a &EventHandle across threads is
// sound. OwnedHandle is already Send, so EventHandle becomes Send + Sync.
unsafe impl Sync for EventHandle {}

impl Write for OutputHandle {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        if self.write_buffer.len() + buf.len() > self.write_buffer.capacity() {
            self.flush()?;
        }
        if buf.len() >= self.write_buffer.capacity() {
            self.handle.write(buf)
        } else {
            self.write_buffer.write(buf)
        }
    }

    fn flush(&mut self) -> IoResult<()> {
        if !self.write_buffer.is_empty() {
            self.handle.write_all(&self.write_buffer)?;
            self.write_buffer.clear();
        }
        Ok(())
    }
}

impl ConsoleOutputHandle for OutputHandle {
    fn set_output_mode(&mut self, mode: u32) -> Result<()> {
        // SAFETY: `self.handle` is a valid owned console output handle (kept
        // alive by the FileDescriptor for the lifetime of self); `mode` is a
        // plain u32. SetConsoleMode has no other preconditions.
        if unsafe { consoleapi::SetConsoleMode(self.handle.as_raw_handle() as *mut _, mode) } == 0 {
            bail!("SetConsoleMode failed: {}", IoError::last_os_error());
        }
        Ok(())
    }

    fn get_output_mode(&mut self) -> Result<u32> {
        let mut mode = 0;
        // SAFETY: `self.handle` is a valid owned console output handle; `mode`
        // is a valid u32 out-pointer live for the duration of the call.
        if unsafe { consoleapi::GetConsoleMode(self.handle.as_raw_handle() as *mut _, &mut mode) }
            == 0
        {
            bail!("GetConsoleMode failed: {}", IoError::last_os_error());
        }
        Ok(mode)
    }

    fn set_output_cp(&mut self, cp: u32) -> Result<()> {
        // SAFETY: SetConsoleOutputCP takes a plain u32 code page id; no pointers.
        if unsafe { SetConsoleOutputCP(cp) } == 0 {
            bail!("SetConsoleOutputCP failed: {}", IoError::last_os_error());
        }
        Ok(())
    }

    fn get_output_cp(&mut self) -> u32 {
        // SAFETY: GetConsoleOutputCP takes no arguments and has no preconditions.
        unsafe { consoleapi::GetConsoleOutputCP() }
    }

    fn fill_char(&mut self, text: char, x: i16, y: i16, len: u32) -> Result<u32> {
        let mut wrote = 0;
        // SAFETY: `self.handle` is a valid owned console output handle; `wrote`
        // is a valid u32 out-pointer. The remaining args are plain values/copies.
        if unsafe {
            FillConsoleOutputCharacterW(
                self.handle.as_raw_handle() as *mut _,
                text as u16,
                len,
                COORD { X: x, Y: y },
                &mut wrote,
            )
        } == 0
        {
            bail!(
                "FillConsoleOutputCharacterW failed: {}",
                IoError::last_os_error()
            );
        }
        Ok(wrote)
    }

    fn fill_attr(&mut self, attr: u16, x: i16, y: i16, len: u32) -> Result<u32> {
        let mut wrote = 0;
        // SAFETY: `self.handle` is a valid owned console output handle; `wrote`
        // is a valid u32 out-pointer. The remaining args are plain values/copies.
        if unsafe {
            FillConsoleOutputAttribute(
                self.handle.as_raw_handle() as *mut _,
                attr,
                len,
                COORD { X: x, Y: y },
                &mut wrote,
            )
        } == 0
        {
            bail!(
                "FillConsoleOutputAttribute failed: {}",
                IoError::last_os_error()
            );
        }
        Ok(wrote)
    }

    fn set_attr(&mut self, attr: u16) -> Result<()> {
        // SAFETY: `self.handle` is a valid owned console output handle; `attr`
        // is a plain u16. SetConsoleTextAttribute has no other preconditions.
        if unsafe { SetConsoleTextAttribute(self.handle.as_raw_handle() as *mut _, attr) } == 0 {
            bail!(
                "SetConsoleTextAttribute failed: {}",
                IoError::last_os_error()
            );
        }
        Ok(())
    }

    fn set_cursor_position(&mut self, x: i16, y: i16) -> Result<()> {
        // SAFETY: `self.handle` is a valid owned console output handle; the
        // COORD is a plain copy. SetConsoleCursorPosition has no other
        // preconditions.
        if unsafe {
            SetConsoleCursorPosition(self.handle.as_raw_handle() as *mut _, COORD { X: x, Y: y })
        } == 0
        {
            bail!(
                "SetConsoleCursorPosition(x={}, y={}) failed: {}",
                x,
                y,
                IoError::last_os_error()
            );
        }
        Ok(())
    }

    fn get_buffer_contents(&mut self) -> Result<Vec<CHAR_INFO>> {
        let info = self.get_buffer_info()?;

        let cols = info.dwSize.X as usize;
        let rows = 1 + info.srWindow.Bottom as usize - info.srWindow.Top as usize;

        let mut res = vec![
            CHAR_INFO {
                Attributes: 0,
                // SAFETY: the Char union is a plain POD union; all-zero bits
                // are a valid value for the u16/AsciiChar variants it holds.
                Char: unsafe { mem::zeroed() }
            };
            cols * rows
        ];
        let mut read_region = SMALL_RECT {
            Left: 0,
            Right: info.dwSize.X - 1,
            Top: info.srWindow.Top,
            Bottom: info.srWindow.Bottom,
        };
        // SAFETY: `self.handle` is a valid owned console output handle; `res`
        // owns cols*rows CHAR_INFO slots and the buffer/size/coord describe it;
        // `read_region` is a valid out-pointer. The call only writes into `res`.
        unsafe {
            if ReadConsoleOutputW(
                self.handle.as_raw_handle() as *mut _,
                res.as_mut_ptr(),
                COORD {
                    X: cols as i16,
                    Y: rows as i16,
                },
                COORD { X: 0, Y: 0 },
                &mut read_region,
            ) == 0
            {
                bail!("ReadConsoleOutputW failed: {}", IoError::last_os_error());
            }
        }
        Ok(res)
    }

    fn set_buffer_contents(&mut self, buffer: &[CHAR_INFO]) -> Result<()> {
        let info = self.get_buffer_info()?;

        let cols = info.dwSize.X as usize;
        let rows = 1 + info.srWindow.Bottom as usize - info.srWindow.Top as usize;
        ensure!(
            rows * cols == buffer.len(),
            "buffer size doesn't match screen size"
        );

        let mut write_region = SMALL_RECT {
            Left: 0,
            Right: info.dwSize.X - 1,
            Top: info.srWindow.Top,
            Bottom: info.srWindow.Bottom,
        };

        // SAFETY: `self.handle` is a valid owned console output handle;
        // `buffer` is a valid shared slice of rows*cols CHAR_INFO (checked
        // above) whose lifetime encloses the call; `write_region` is a valid
        // out-pointer.
        unsafe {
            if WriteConsoleOutputW(
                self.handle.as_raw_handle() as *mut _,
                buffer.as_ptr(),
                COORD {
                    X: cols as i16,
                    Y: rows as i16,
                },
                COORD { X: 0, Y: 0 },
                &mut write_region,
            ) == 0
            {
                bail!("WriteConsoleOutputW failed: {}", IoError::last_os_error());
            }
        }
        Ok(())
    }

    fn get_buffer_info(&mut self) -> Result<CONSOLE_SCREEN_BUFFER_INFO> {
        // SAFETY: CONSOLE_SCREEN_BUFFER_INFO is a plain POD struct; all-zero
        // bits are a valid value to pass to GetConsoleScreenBufferInfo.
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = unsafe { mem::zeroed() };
        // SAFETY: `self.handle` is a valid owned console output handle; `info`
        // is a valid out-pointer live for the duration of the call.
        let ok = unsafe {
            GetConsoleScreenBufferInfo(self.handle.as_raw_handle() as *mut _, &mut info as *mut _)
        };
        if ok == 0 {
            bail!(
                "GetConsoleScreenBufferInfo failed: {}",
                IoError::last_os_error()
            );
        }
        Ok(info)
    }

    fn set_viewport(&mut self, left: i16, top: i16, right: i16, bottom: i16) -> Result<()> {
        let rect = SMALL_RECT {
            Left: left,
            Top: top,
            Right: right,
            Bottom: bottom,
        };
        // SAFETY: `self.handle` is a valid owned console output handle; `&rect`
        // is a valid shared SMALL_RECT reference live for the call.
        if unsafe { SetConsoleWindowInfo(self.handle.as_raw_handle() as *mut _, 1, &rect) } == 0 {
            bail!("SetConsoleWindowInfo failed: {}", IoError::last_os_error());
        }
        Ok(())
    }

    fn scroll_region(
        &mut self,
        left: i16,
        top: i16,
        right: i16,
        bottom: i16,
        dx: i16,
        dy: i16,
        attr: u16,
    ) -> Result<()> {
        let scroll_rect = SMALL_RECT {
            Left: max(left, left - dx),
            Top: max(top, top - dy),
            Right: min(right, right - dx),
            Bottom: min(bottom, bottom - dy),
        };
        let clip_rect = SMALL_RECT {
            Left: left,
            Top: top,
            Right: right,
            Bottom: bottom,
        };
        // SAFETY: CHAR_INFO is POD and its Char union is valid zeroed; the
        // `fill.Char.UnicodeChar_mut()` pointer is a valid in-place pointer
        // into the local `fill`, so writing through it initializes the u16
        // variant before `fill` is read.
        let fill = unsafe {
            let mut fill = CHAR_INFO {
                Char: mem::zeroed(),
                Attributes: attr,
            };
            *fill.Char.UnicodeChar_mut() = ' ' as u16;
            fill
        };
        // SAFETY: `self.handle` is a valid owned console output handle;
        // `scroll_rect`, `clip_rect` and `fill` are valid shared references
        // for the call; the COORD is a plain copy.
        if unsafe {
            ScrollConsoleScreenBufferW(
                self.handle.as_raw_handle() as *mut _,
                &scroll_rect,
                &clip_rect,
                COORD {
                    X: max(left, left + dx),
                    Y: max(top, top + dy),
                },
                &fill,
            )
        } == 0
        {
            bail!(
                "ScrollConsoleScreenBufferW failed: {}",
                IoError::last_os_error()
            );
        }
        Ok(())
    }
}
