use super::WinChild;
use crate::cmdbuilder::CommandBuilder;
use crate::win::procthreadattr::ProcThreadAttributeList;
use anyhow::{bail, ensure, Error};
use filedescriptor::{FileDescriptor, OwnedHandle};
use lazy_static::lazy_static;
use shared_library::shared_library;
use std::ffi::OsString;
use std::io::{Error as IoError, Read};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::sync::Mutex;
use std::{mem, ptr};
use winapi::shared::minwindef::DWORD;
use winapi::shared::winerror::{HRESULT, S_OK};
use winapi::um::handleapi::*;
use winapi::um::jobapi2::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
};
use winapi::um::processthreadsapi::*;
use winapi::um::winbase::{
    CREATE_NEW_PROCESS_GROUP, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};
use winapi::um::wincon::COORD;
use winapi::um::winnt::{
    JobObjectExtendedLimitInformation, HANDLE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

pub type HPCON = HANDLE;

pub const PSEUDOCONSOLE_INHERIT_CURSOR: DWORD = 0x1;
pub const PSEUDOCONSOLE_RESIZE_QUIRK: DWORD = 0x2;
pub const PSEUDOCONSOLE_WIN32_INPUT_MODE: DWORD = 0x4;
#[allow(dead_code)]
pub const PSEUDOCONSOLE_PASSTHROUGH_MODE: DWORD = 0x8;

shared_library!(ConPtyFuncs,
    pub fn CreatePseudoConsole(
        size: COORD,
        hInput: HANDLE,
        hOutput: HANDLE,
        flags: DWORD,
        hpc: *mut HPCON
    ) -> HRESULT,
    pub fn ResizePseudoConsole(hpc: HPCON, size: COORD) -> HRESULT,
    pub fn ClosePseudoConsole(hpc: HPCON),
);

fn load_conpty() -> ConPtyFuncs {
    // If the kernel doesn't export these functions then their system is
    // too old and we cannot run.
    let kernel = ConPtyFuncs::open(Path::new("kernel32.dll")).expect(
        "this system does not support conpty.  Windows 10 October 2018 or newer is required",
    );

    // We prefer to use a sideloaded conpty.dll and openconsole.exe host deployed
    // alongside the application.  We check for this after checking for kernel
    // support so that we don't try to proceed and do something crazy.
    if let Ok(sideloaded) = ConPtyFuncs::open(Path::new("conpty.dll")) {
        sideloaded
    } else {
        kernel
    }
}

lazy_static! {
    static ref CONPTY: ConPtyFuncs = load_conpty();
}

pub struct PseudoCon {
    con: HPCON,
    output: FileDescriptor,
}

// SAFETY: An `HPCON` is a process-global Windows kernel handle with no
// thread affinity. All mutation of a `PseudoCon` happens behind the
// `Mutex<Inner>` in `ConPtyMasterPty`, so sharing the value across threads
// and sending it to other threads is sound.
unsafe impl Send for PseudoCon {}
// SAFETY: same rationale as the `Send` impl above - access is always
// serialized through the `Mutex<Inner>` in `ConPtyMasterPty`.
unsafe impl Sync for PseudoCon {}

impl Drop for PseudoCon {
    fn drop(&mut self) {
        // HACK: manually closing handles to avoid `ClosePseudoConsole` call from blocking,
        //       in future versions of conpty.dll `ClosePseudoConsole` will no longer block
        //       and this unsafe block will not be needed.
        //
        // NOTE:
        // A `HPCON` is a struct consisting of 3 HANDLEs:
        // A pipe for communicating with the PTY, a handle to keep it alive
        // until ClosePseudoConsole is called, and a process handle to the
        // underlying conhost process.
        // SAFETY: As documented above, an `HPCON` is internally a struct of
        // three `HANDLE`s. We cast the opaque pointer to `*mut [HANDLE; 3]`
        // and close each handle individually to work around conpty blocking.
        // `self.con` is still valid because we have not yet called
        // ClosePseudoConsole (that happens after this drain loop).
        unsafe {
            let handles = self.con as *mut [HANDLE; 3];
            for i in 0..3 {
                CloseHandle((*handles)[i]);
                (*handles)[i] = std::ptr::null_mut();
            }
        }

        let mut buffer = [0; 1000];
        // read up to 1000 bytes
        while let Ok(num_byes_read) = self.output.read(&mut buffer) {
            if num_byes_read == 0 {
                break;
            }
        }

        // This won't do anything but deallocate the handle.
        // SAFETY: FFI call into the loaded conpty.dll. `self.con` was obtained
        // from CreatePseudoConsole and is still a valid (now manually-closed)
        // pseudo-console handle.
        unsafe { (CONPTY.ClosePseudoConsole)(self.con) };
    }
}

impl PseudoCon {
    pub fn new(size: COORD, input: FileDescriptor, output: FileDescriptor) -> Result<Self, Error> {
        let mut con: HPCON = INVALID_HANDLE_VALUE;
        // SAFETY: FFI call into conpty.dll. `input` and `output` are valid
        // FileDescriptor handles and `&mut con` is a valid out-pointer.
        let result = unsafe {
            (CONPTY.CreatePseudoConsole)(
                size,
                input.as_raw_handle() as _,
                output.as_raw_handle() as _,
                PSEUDOCONSOLE_INHERIT_CURSOR
                    | PSEUDOCONSOLE_RESIZE_QUIRK
                    | PSEUDOCONSOLE_WIN32_INPUT_MODE,
                &mut con,
            )
        };
        ensure!(
            result == S_OK,
            "failed to create pseudo console: HRESULT {}",
            result
        );
        Ok(Self { con, output })
    }

    pub fn resize(&self, size: COORD) -> Result<(), Error> {
        // SAFETY: FFI call into conpty.dll. `self.con` is a valid HPCON
        // obtained from CreatePseudoConsole.
        let result = unsafe { (CONPTY.ResizePseudoConsole)(self.con, size) };
        ensure!(
            result == S_OK,
            "failed to resize console to {}x{}: HRESULT: {}",
            size.X,
            size.Y,
            result
        );
        Ok(())
    }

    pub fn spawn_command(&self, cmd: CommandBuilder) -> anyhow::Result<WinChild> {
        // SAFETY: STARTUPINFOEXW is a `repr(C)` struct of primitive types;
        // zero-initialization is valid. `cb` is set immediately after.
        let mut si: STARTUPINFOEXW = unsafe { mem::zeroed() };
        si.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
        // Explicitly set the stdio handles as invalid handles otherwise
        // we can end up with a weird state where the spawned process can
        // inherit the explicitly redirected output handles from its parent.
        // For example, when daemonizing wezterm-mux-server, the stdio handles
        // are redirected to a log file and the spawned process would end up
        // writing its output there instead of to the pty we just created.
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        si.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        si.StartupInfo.hStdError = INVALID_HANDLE_VALUE;

        let mut attrs = ProcThreadAttributeList::with_capacity(1)?;
        attrs.set_pty(self.con)?;
        si.lpAttributeList = attrs.as_mut_ptr();

        // SAFETY: PROCESS_INFORMATION is a `repr(C)` struct of primitive
        // types; zero-initialization is valid. It will be filled by
        // CreateProcessW below.
        let mut pi: PROCESS_INFORMATION = unsafe { mem::zeroed() };

        let (mut exe, mut cmdline) = cmd.cmdline()?;
        let cmd_os = OsString::from_wide(&cmdline);

        let cwd = cmd.current_directory();

        // SAFETY: `exe` and `cmdline` are NUL-terminated wide strings with
        // valid mutable pointers. `si` is a valid STARTUPINFOEXW with the
        // attribute list set. `pi` is a valid out-pointer.
        let res = unsafe {
            CreateProcessW(
                exe.as_mut_slice().as_mut_ptr(),
                cmdline.as_mut_slice().as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                // NOTE: deliberately NOT setting CREATE_NEW_PROCESS_GROUP here.
                // Windows explicitly stops delivering CTRL_C_EVENT (physical
                // Ctrl+C) to processes created with that flag - only
                // CTRL_BREAK_EVENT reaches them. That's fine for a process we
                // want to signal selectively via GenerateConsoleCtrlEvent, but
                // it silently breaks the far more common case of the user
                // pressing Ctrl+C to interrupt/exit whatever is running in the
                // pane, which must always keep working.
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                cmd.environment_block().as_mut_slice().as_mut_ptr() as *mut _,
                cwd.as_ref()
                    .map(|c| c.as_slice().as_ptr())
                    .unwrap_or(ptr::null()),
                &mut si.StartupInfo,
                &mut pi,
            )
        };
        if res == 0 {
            let err = IoError::last_os_error();
            let msg = format!(
                "CreateProcessW `{:?}` in cwd `{:?}` failed: {}",
                cmd_os,
                cwd.as_ref().map(|c| OsString::from_wide(c)),
                err
            );
            log::error!("{}", msg);
            bail!("{}", msg);
        }

        // Make sure we close out the thread handle so we don't leak it;
        // we do this simply by making it owned
        // SAFETY: `pi.hThread` was populated by a successful CreateProcessW
        // with a valid thread handle. We take exclusive ownership so it is
        // closed on drop.
        let _main_thread = unsafe { OwnedHandle::from_raw_handle(pi.hThread as _) };
        // SAFETY: `pi.hProcess` was populated by a successful CreateProcessW
        // with a valid process handle. We take exclusive ownership.
        let proc = unsafe { OwnedHandle::from_raw_handle(pi.hProcess as _) };

        // Create a Job Object and assign the freshly spawned process to it,
        // configured so that closing the job's handle force-kills every
        // process still assigned to it (JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE).
        // This ensures that when we kill the direct child on pane-close, any
        // grandchild processes it spawned (eg. a shell running a CLI tool
        // that itself spawned other processes) are also cleaned up, rather
        // than being orphaned. The job handle is stored on `WinChild` and
        // closed as part of the kill sequence.
        //
        // Failure to set up the job object is non-fatal: we still have a
        // usable direct-child kill path, so we simply log a warning and
        // proceed without job-based cleanup in that (unexpected) case.
        // SAFETY: FFI call with NULL security attributes and NULL name;
        // returns either a valid job handle or NULL.
        let job = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
        let job = if job.is_null() {
            log::warn!(
                "CreateJobObjectW failed: {}; descendant processes of `{:?}` \
                 will not be automatically cleaned up on kill",
                IoError::last_os_error(),
                cmd_os,
            );
            None
        } else {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
                // SAFETY: `repr(C)` POD struct of primitive types; valid
                // zero-initialized.
                unsafe { mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            // SAFETY: `job` is a valid handle from CreateJobObjectW.
            // `info` is a properly initialized JOBOBJECT_EXTENDED_LIMIT_INFORMATION.
            let set_res = unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &mut info as *mut _ as *mut _,
                    mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if set_res == 0 {
                log::warn!(
                    "SetInformationJobObject failed: {}; descendant processes of \
                     `{:?}` will not be automatically cleaned up on kill",
                    IoError::last_os_error(),
                    cmd_os,
                );
                // SAFETY: `job` is a valid handle; we clean it up because
                // SetInformationJobObject failed.
                unsafe {
                    CloseHandle(job);
                }
                None
            } else {
                // SAFETY: Both `job` (from CreateJobObjectW) and `proc`
                // (from CreateProcessW) are valid handles.
                let assign_res = unsafe { AssignProcessToJobObject(job, proc.as_raw_handle() as _) };
                if assign_res == 0 {
                    log::warn!(
                        "AssignProcessToJobObject failed: {}; descendant processes of \
                         `{:?}` will not be automatically cleaned up on kill",
                        IoError::last_os_error(),
                        cmd_os,
                    );
                    // SAFETY: `job` is a valid handle; we clean it up
                    // because AssignProcessToJobObject failed.
                    unsafe {
                        CloseHandle(job);
                    }
                    None
                } else {
                    // SAFETY: `job` is a valid, fully-configured job object
                    // handle. We take exclusive ownership so it is closed (and
                    // thus triggers KILL_ON_JOB_CLOSE) on drop.
                    Some(unsafe { OwnedHandle::from_raw_handle(job as _) })
                }
            }
        };

        Ok(WinChild {
            proc: Mutex::new(proc),
            job: std::sync::Arc::new(Mutex::new(job)),
        })
    }
}
