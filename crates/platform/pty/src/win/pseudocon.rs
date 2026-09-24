use super::{job_object_required, WinChild};
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
use winapi::um::jobapi2::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject};
use winapi::um::processthreadsapi::*;
// `CREATE_NEW_PROCESS_GROUP` is deliberately not imported: see the note at
// the `dwCreationFlags` assignment below, and `super::mod`'s commentary on
// why the CTRL_BREAK_EVENT step was dropped.
use winapi::um::winbase::{
    CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};
use winapi::um::wincon::COORD;
use winapi::um::winnt::{
    JobObjectExtendedLimitInformation, HANDLE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

// `HPCON` is the literal type name Microsoft uses for the pseudoconsole handle
// in the Win32 API (see `wincontypes.h` / `CreatePseudoConsole`). Renaming it
// to `Hpcon` would break the direct correspondence with the documented Win32
// API and with the FFI signatures below that mirror `ConPTY.h`.
#[allow(clippy::upper_case_acronyms)]
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
    //
    // Falling back to `kernel` here does NOT mean a fully working in-box
    // ConPTY: on at least Windows 10 22H2 (build 19045) the in-box
    // `CreatePseudoConsole` succeeds, but the pseudo-console it creates
    // cannot actually host a child process -- `CreateProcessW` against it
    // fails with `ERROR_INVALID_HANDLE` (see task #326). The sideload is
    // there precisely to paper over that gap, so losing it (missing
    // `conpty.dll`/`OpenConsole.exe` next to the executable) is a real
    // degradation, not just a cosmetic difference. We still return the
    // kernel-provided functions here (rather than failing outright) so
    // that newer Windows versions where the in-box implementation *does*
    // work end up in a good state; `LocalDomain::spawn_pane` is
    // responsible for surfacing a loud, user-visible error if spawning
    // through whichever ConPTY we ended up with doesn't actually work.
    if let Ok(sideloaded) = ConPtyFuncs::open(Path::new("conpty.dll")) {
        sideloaded
    } else {
        log::warn!(
            "sideloaded conpty.dll not found next to the executable; \
             falling back to the system-provided ConPTY in kernel32.dll. \
             On some Windows versions this fallback cannot actually host a \
             shell (see task #326); if pane spawn fails, that's why."
        );
        kernel
    }
}

lazy_static! {
    static ref CONPTY: ConPtyFuncs = load_conpty();
}

fn spawn_with_prepared_job<J, R, C, S>(
    require_job: bool,
    create_job: C,
    spawn: S,
) -> anyhow::Result<(R, Option<J>)>
where
    C: FnOnce() -> anyhow::Result<J>,
    S: FnOnce(Option<&J>) -> anyhow::Result<R>,
{
    let job = if require_job {
        Some(create_job()?)
    } else {
        None
    };
    let result = spawn(job.as_ref())?;
    Ok((result, job))
}

fn create_kill_on_close_job() -> anyhow::Result<OwnedHandle> {
    // SAFETY: Null attributes and name request a non-inheritable, unnamed job.
    let raw = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
    if raw.is_null() {
        let error = IoError::last_os_error();
        bail!("CreateJobObjectW failed: {}", error);
    }
    // SAFETY: CreateJobObjectW returned a valid owned HANDLE.
    let job = unsafe { OwnedHandle::from_raw_handle(raw as _) };
    // SAFETY: This WinAPI information struct is valid when zero-initialized.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `job` is live and `info` is a valid input of the declared size.
    let set_res = unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as _,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut _,
            mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if set_res == 0 {
        let error = IoError::last_os_error();
        bail!("SetInformationJobObject failed: {}", error);
    }
    Ok(job)
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
        let (mut exe, mut cmdline) = cmd.cmdline()?;
        let cmd_os = OsString::from_wide(&cmdline);
        let cwd = cmd.current_directory();
        let require_job = job_object_required();
        let (proc, required_job) =
            spawn_with_prepared_job(require_job, create_kill_on_close_job, |job| {
                // SAFETY: STARTUPINFOEXW is valid when zero-initialized.
                let mut si: STARTUPINFOEXW = unsafe { mem::zeroed() };
                si.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
                si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
                si.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
                si.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
                si.StartupInfo.hStdError = INVALID_HANDLE_VALUE;

                let mut attrs =
                    ProcThreadAttributeList::with_capacity(if job.is_some() { 2 } else { 1 })?;
                attrs.set_pty(self.con)?;
                if let Some(job) = job {
                    attrs.set_job(job.as_raw_handle() as _)?;
                }
                si.lpAttributeList = attrs.as_mut_ptr();

                // SAFETY: PROCESS_INFORMATION is valid when zero-initialized.
                let mut pi: PROCESS_INFORMATION = unsafe { mem::zeroed() };
                // SAFETY: All pointers refer to live command, environment, startup,
                // attribute, and output storage for the duration of this call.
                let res = unsafe {
                    CreateProcessW(
                        exe.as_mut_slice().as_mut_ptr(),
                        cmdline.as_mut_slice().as_mut_ptr(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        0,
                        // CREATE_NEW_PROCESS_GROUP would break physical Ctrl+C.
                        EXTENDED_STARTUPINFO_PRESENT
                            | CREATE_UNICODE_ENVIRONMENT
                            | cmd.priority_class(),
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

                // SAFETY: CreateProcessW initialized both owned handles.
                let _main_thread = unsafe { OwnedHandle::from_raw_handle(pi.hThread as _) };
                // SAFETY: CreateProcessW initialized the owned process handle.
                let proc = unsafe { OwnedHandle::from_raw_handle(pi.hProcess as _) };
                Ok(proc)
            })?;

        let job = if require_job {
            required_job
        } else {
            match create_kill_on_close_job() {
                Ok(job) => {
                    // SAFETY: Both job and process handles are live; neither is transferred.
                    let assigned = unsafe {
                        AssignProcessToJobObject(
                            job.as_raw_handle() as _,
                            proc.as_raw_handle() as _,
                        )
                    };
                    if assigned == 0 {
                        let error = IoError::last_os_error();
                        log::warn!(
                            "AssignProcessToJobObject failed: {}; descendants of `{:?}` \
                             may survive pane close",
                            error,
                            cmd_os
                        );
                        None
                    } else {
                        Some(job)
                    }
                }
                Err(error) => {
                    log::warn!(
                        "Job Object setup failed: {:#}; descendants of `{:?}` \
                         may survive pane close",
                        error,
                        cmd_os
                    );
                    None
                }
            }
        };

        Ok(WinChild {
            proc: Mutex::new(proc),
            job: std::sync::Arc::new(Mutex::new(job)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::{Arc, Mutex};

    #[test]
    fn required_job_is_prepared_before_spawn_and_failure_skips_spawn() {
        let state = Cell::new(0);
        let (result, job) = spawn_with_prepared_job(
            true,
            || {
                state.set(1);
                Ok(7u8)
            },
            |job| {
                assert_eq!(state.get(), 1);
                assert_eq!(job.copied(), Some(7));
                state.set(2);
                Ok(42u8)
            },
        )
        .unwrap();
        assert_eq!((result, job, state.get()), (42, Some(7), 2));

        let spawn_called = Cell::new(false);
        let result = spawn_with_prepared_job::<(), (), _, _>(
            true,
            || anyhow::bail!("job unavailable"),
            |_| {
                spawn_called.set(true);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!spawn_called.get());
    }

    #[test]
    fn ordinary_spawn_does_not_require_a_job_before_process_creation() {
        let (result, job) = spawn_with_prepared_job::<(), _, _, _>(
            false,
            || panic!("ordinary spawn must not prepare a job first"),
            |job| {
                assert!(job.is_none());
                Ok(42u8)
            },
        )
        .unwrap();
        assert_eq!(result, 42);
        assert!(job.is_none());
    }

    #[test]
    fn windows_accepts_a_single_job_list_attribute() {
        let job = create_kill_on_close_job().unwrap();
        let mut attrs = ProcThreadAttributeList::with_capacity(2).unwrap();
        assert!(attrs.set_job(std::ptr::null_mut()).is_err());
        attrs.set_job(job.as_raw_handle() as _).unwrap();
        assert!(attrs.set_job(job.as_raw_handle() as _).is_err());
    }

    #[test]
    fn killer_without_process_handle_still_closes_its_job() {
        let job = Arc::new(Mutex::new(Some(create_kill_on_close_job().unwrap())));
        let mut killer = crate::win::WinChildKiller {
            proc: None,
            job: Arc::clone(&job),
        };
        crate::ChildKiller::kill(&mut killer).unwrap();
        assert!(job.lock().unwrap().is_none());
    }
}
