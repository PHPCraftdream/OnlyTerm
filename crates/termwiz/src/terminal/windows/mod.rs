mod console;

use console::{dimensions_from_buffer_info, EventHandle, InputHandle, OutputHandle};
pub use console::{ConsoleInputHandle, ConsoleOutputHandle};

use crate::caps::Capabilities;
use crate::escape::csi::{DecPrivateMode, DecPrivateModeCode, Mode, CSI};
use crate::input::{InputEvent, InputParser};
use crate::istty::IsTty;
use crate::render::terminfo::TerminfoRenderer;
use crate::render::windows::WindowsConsoleRenderer;
use crate::surface::Change;
use crate::terminal::{cast, ProbeCapabilities, ScreenSize, Terminal};
use crate::{bail, format_err, Result};
use filedescriptor::FileDescriptor;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{stdin, stdout, Error as IoError, Read, Result as IoResult, Write};
use std::os::windows::io::AsRawHandle;
use std::sync::Arc;
use std::time::Duration;
use winapi::um::synchapi::WaitForMultipleObjects;
use winapi::um::winbase::{INFINITE, WAIT_FAILED, WAIT_OBJECT_0};
use winapi::um::wincon::{
    SetConsoleScreenBufferSize, COORD, DISABLE_NEWLINE_AUTO_RETURN, ENABLE_ECHO_INPUT,
    ENABLE_LINE_INPUT, ENABLE_MOUSE_INPUT, ENABLE_PROCESSED_INPUT, ENABLE_VIRTUAL_TERMINAL_INPUT,
    ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_WINDOW_INPUT,
};
use winapi::um::winnls::CP_UTF8;

enum Renderer {
    Terminfo(TerminfoRenderer),
    Windows(WindowsConsoleRenderer),
}

pub struct WindowsTerminal {
    input_handle: InputHandle,
    output_handle: OutputHandle,
    waker_handle: Arc<EventHandle>,
    saved_input_mode: u32,
    saved_output_mode: u32,
    renderer: Renderer,
    input_parser: InputParser,
    input_queue: VecDeque<InputEvent>,
    saved_input_cp: u32,
    saved_output_cp: u32,
    in_alternate_screen: bool,
    caps: Capabilities,
}

impl Drop for WindowsTerminal {
    fn drop(&mut self) {
        if matches!(&self.renderer, Renderer::Terminfo(_)) {
            macro_rules! decreset {
                ($variant:ident) => {
                    write!(
                        self.output_handle,
                        "{}",
                        CSI::Mode(Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                            DecPrivateModeCode::$variant
                        )))
                    )
                    .unwrap();
                };
            }
            self.render(&[Change::CursorVisibility(
                crate::surface::CursorVisibility::Visible,
            )])
            .ok();
            decreset!(BracketedPaste);
            decreset!(SGRMouse);
            decreset!(AnyEventMouse);
        }

        self.exit_alternate_screen().unwrap();
        self.output_handle.flush().unwrap();
        self.input_handle
            .set_input_mode(self.saved_input_mode)
            .expect("failed to restore console input mode");
        self.input_handle
            .set_input_cp(self.saved_input_cp)
            .expect("failed to restore console input codepage");
        self.output_handle
            .set_output_mode(self.saved_output_mode)
            .expect("failed to restore console output mode");
        self.output_handle
            .set_output_cp(self.saved_output_cp)
            .expect("failed to restore console output codepage");
    }
}

impl WindowsTerminal {
    /// Attempt to create an instance from the stdin and stdout of the
    /// process.  This will fail unless both are associated with a tty.
    /// Note that this will duplicate the underlying file descriptors
    /// and will no longer participate in the stdin/stdout locking
    /// provided by the rust standard library.
    pub fn new_from_stdio(caps: Capabilities) -> Result<Self> {
        Self::new_with(caps, stdin(), stdout())
    }

    /// Create an instance using the provided capabilities, read and write
    /// handles. The read and write handles must be tty handles of this
    /// will return an error.
    pub fn new_with<A: Read + IsTty + AsRawHandle, B: Write + IsTty + AsRawHandle>(
        caps: Capabilities,
        read: A,
        write: B,
    ) -> Result<Self> {
        if !read.is_tty() || !write.is_tty() {
            bail!("stdin and stdout must both be tty handles");
        }

        let mut input_handle = InputHandle {
            handle: FileDescriptor::dup(&read)?,
        };
        let mut output_handle = OutputHandle::new(FileDescriptor::dup(&write)?);
        let waker_handle = Arc::new(EventHandle::new()?);

        let saved_input_mode = input_handle.get_input_mode()?;
        let saved_output_mode = output_handle.get_output_mode()?;
        let saved_input_cp = input_handle.get_input_cp();
        let saved_output_cp = output_handle.get_output_cp();

        // Test whether we have a virtual terminal capable
        // console device by attempting to set the appropriate flags.
        let virtual_terminal_available = output_handle
            .set_output_mode(
                saved_output_mode
                    | ENABLE_VIRTUAL_TERMINAL_PROCESSING
                    | DISABLE_NEWLINE_AUTO_RETURN,
            )
            .is_ok();

        // Allow opting out of that processing
        fn bypass_virtual_terminal() -> bool {
            if let Ok(t) = std::env::var("TERMWIZ_BYPASS_VIRTUAL_TERMINAL") {
                t == "1"
            } else {
                false
            }
        }

        let renderer = if caps.terminfo_db().is_some() {
            Renderer::Terminfo(TerminfoRenderer::new(caps.clone()))
        } else if virtual_terminal_available && !bypass_virtual_terminal() {
            Renderer::Terminfo(TerminfoRenderer::new(caps.clone().apply_builtin_terminfo()))
        } else {
            Renderer::Windows(WindowsConsoleRenderer::new(caps.clone()))
        };
        let input_parser = InputParser::new();

        let mut terminal = Self {
            input_handle,
            output_handle,
            waker_handle,
            saved_input_mode,
            saved_output_mode,
            saved_input_cp,
            saved_output_cp,
            renderer,
            input_parser,
            input_queue: VecDeque::new(),
            in_alternate_screen: false,
            caps,
        };

        terminal.input_handle.set_input_cp(CP_UTF8)?;
        terminal.output_handle.set_output_cp(CP_UTF8)?;

        // We already enabled this for output, but let's also turn it
        // on for input here now.
        terminal.enable_virtual_terminal_processing_if_needed()?;

        Ok(terminal)
    }

    fn enable_virtual_terminal_processing_if_needed(&mut self) -> Result<()> {
        match &self.renderer {
            Renderer::Terminfo(_) => self.enable_virtual_terminal_processing(),
            Renderer::Windows(_) => Ok(()),
        }
    }

    /// Attempt to explicitly open handles to a console device (CONIN$,
    /// CONOUT$). This should yield the terminal already associated with
    /// the process, even if stdio streams have been redirected.
    pub fn new(caps: Capabilities) -> Result<Self> {
        let read = OpenOptions::new().read(true).write(true).open("CONIN$")?;
        let write = OpenOptions::new().read(true).write(true).open("CONOUT$")?;
        Self::new_with(caps, read, write)
    }

    pub fn enable_virtual_terminal_processing(&mut self) -> Result<()> {
        let mode = self.output_handle.get_output_mode()?;
        self.output_handle.set_output_mode(
            mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING | DISABLE_NEWLINE_AUTO_RETURN,
        )?;

        let mode = self.input_handle.get_input_mode()?;
        self.input_handle
            .set_input_mode(mode | ENABLE_VIRTUAL_TERMINAL_INPUT)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct WindowsTerminalWaker {
    handle: Arc<EventHandle>,
}

impl WindowsTerminalWaker {
    pub fn wake(&self) -> IoResult<()> {
        self.handle.set()?;
        Ok(())
    }
}

impl Terminal for WindowsTerminal {
    fn set_raw_mode(&mut self) -> Result<()> {
        let mode = self.output_handle.get_output_mode()?;
        self.output_handle
            .set_output_mode(mode | DISABLE_NEWLINE_AUTO_RETURN)
            .ok();

        let mode = self.input_handle.get_input_mode()?;

        self.input_handle.set_input_mode(
            (mode & !(ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT))
                | ENABLE_MOUSE_INPUT
                | ENABLE_WINDOW_INPUT,
        )?;

        if matches!(&self.renderer, Renderer::Terminfo(_)) {
            macro_rules! decset {
                ($variant:ident) => {
                    write!(
                        self.output_handle,
                        "{}",
                        CSI::Mode(Mode::SetDecPrivateMode(DecPrivateMode::Code(
                            DecPrivateModeCode::$variant
                        )))
                    )?;
                };
            }

            if self.caps.bracketed_paste() {
                decset!(BracketedPaste);
            }
            if self.caps.mouse_reporting() {
                decset!(AnyEventMouse);
                decset!(SGRMouse);
            }
            self.output_handle.flush()?;
        }

        Ok(())
    }

    fn set_cooked_mode(&mut self) -> Result<()> {
        let mode = self.output_handle.get_output_mode()?;
        self.output_handle
            .set_output_mode(mode & !DISABLE_NEWLINE_AUTO_RETURN)
            .ok();

        let mode = self.input_handle.get_input_mode()?;

        self.input_handle.set_input_mode(
            (mode & !(ENABLE_MOUSE_INPUT | ENABLE_WINDOW_INPUT))
                | ENABLE_ECHO_INPUT
                | ENABLE_LINE_INPUT
                | ENABLE_PROCESSED_INPUT,
        )
    }

    fn enter_alternate_screen(&mut self) -> Result<()> {
        if matches!(&self.renderer, Renderer::Terminfo(_)) {
            if !self.in_alternate_screen {
                write!(
                    self.output_handle,
                    "{}",
                    CSI::Mode(Mode::SetDecPrivateMode(DecPrivateMode::Code(
                        DecPrivateModeCode::ClearAndEnableAlternateScreen
                    )))
                )?;
                self.in_alternate_screen = true;
            }
        } else {
            // TODO: Implement using CreateConsoleScreenBuffer and
            // SetConsoleActiveScreenBuffer.
        }
        Ok(())
    }

    fn exit_alternate_screen(&mut self) -> Result<()> {
        // TODO: Implement using SetConsoleActiveScreenBuffer.
        if matches!(&self.renderer, Renderer::Terminfo(_)) {
            if self.in_alternate_screen {
                write!(
                    self.output_handle,
                    "{}",
                    CSI::Mode(Mode::ResetDecPrivateMode(DecPrivateMode::Code(
                        DecPrivateModeCode::ClearAndEnableAlternateScreen
                    )))
                )?;
                self.in_alternate_screen = false;
            }
        } else {
            // TODO: Implement using CreateConsoleScreenBuffer and
            // SetConsoleActiveScreenBuffer.
        }
        Ok(())
    }

    fn get_screen_size(&mut self) -> Result<ScreenSize> {
        let info = self.output_handle.get_buffer_info()?;
        let (cols, rows) = dimensions_from_buffer_info(info);

        Ok(ScreenSize {
            rows: cast(rows)?,
            cols: cast(cols)?,
            xpixel: 0,
            ypixel: 0,
        })
    }

    fn probe_capabilities(&mut self) -> Option<ProbeCapabilities<'_>> {
        Some(ProbeCapabilities::new(
            &mut self.input_handle,
            &mut self.output_handle,
        ))
    }

    fn set_screen_size(&mut self, size: ScreenSize) -> Result<()> {
        // FIXME: take into account the visible window size here;
        // this probably changes the size of everything including scrollback
        let size = COORD {
            X: cast(size.cols)?,
            Y: cast(size.rows)?,
        };
        let handle = self.output_handle.handle.as_raw_handle();
        // SAFETY: `handle` is a valid owned console output handle; `size` is a
        // plain COORD copy. SetConsoleScreenBufferSize has no other preconditions.
        if unsafe { SetConsoleScreenBufferSize(handle as *mut _, size) } != 1 {
            bail!(
                "failed to SetConsoleScreenBufferSize: {}",
                IoError::last_os_error()
            );
        }
        Ok(())
    }

    fn render(&mut self, changes: &[Change]) -> Result<()> {
        match &mut self.renderer {
            Renderer::Terminfo(r) => r.render_to(changes, &mut self.output_handle),
            Renderer::Windows(r) => r.render_to(changes, &mut self.output_handle),
        }
    }

    fn flush(&mut self) -> Result<()> {
        self.output_handle
            .flush()
            .map_err(|e| format_err!("flush failed: {}", e))
    }

    fn poll_input(&mut self, wait: Option<Duration>) -> Result<Option<InputEvent>> {
        loop {
            if let Some(event) = self.input_queue.pop_front() {
                return Ok(Some(event));
            }

            let mut pending = self.input_handle.get_number_of_input_events()?;

            if pending == 0 {
                let mut handles = [
                    self.input_handle.handle.as_raw_handle() as *mut _,
                    self.waker_handle.handle.as_raw_handle() as *mut _,
                ];
                let result = unsafe {
                    // SAFETY: `handles` holds two valid raw handles (owned
                    // console input + event) live for the call; count is 2 and
                    // the array outlives WaitForMultipleObjects.
                    WaitForMultipleObjects(
                        2,
                        handles.as_mut_ptr(),
                        0,
                        wait.map(|wait| wait.as_millis() as u32).unwrap_or(INFINITE),
                    )
                };
                if result == WAIT_OBJECT_0 {
                    pending = self.input_handle.get_number_of_input_events()?;
                } else if result == WAIT_OBJECT_0 + 1 {
                    return Ok(Some(InputEvent::Wake));
                } else if result == WAIT_FAILED {
                    bail!(
                        "failed to WaitForMultipleObjects: {}",
                        IoError::last_os_error()
                    );
                } else {
                    // WAIT_TIMEOUT and any other unexpected value: nothing to report
                    return Ok(None);
                }
            }

            let records = self.input_handle.read_console_input(pending)?;

            let input_queue = &mut self.input_queue;
            self.input_parser
                .decode_input_records(&records, &mut |evt| input_queue.push_back(evt));
        }
    }

    fn waker(&self) -> WindowsTerminalWaker {
        WindowsTerminalWaker {
            handle: self.waker_handle.clone(),
        }
    }
}
