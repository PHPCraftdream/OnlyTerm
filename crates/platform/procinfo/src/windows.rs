#![cfg(windows)]
use super::*;
use ntapi::ntpebteb::PEB;
use ntapi::ntpsapi::{
    NtQueryInformationProcess, ProcessBasicInformation, ProcessWow64Information,
    PROCESS_BASIC_INFORMATION,
};
use ntapi::ntrtl::RTL_USER_PROCESS_PARAMETERS;
use ntapi::ntwow64::RTL_USER_PROCESS_PARAMETERS32;
use std::ffi::OsString;
use std::io;
use std::mem::MaybeUninit;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winapi::shared::minwindef::{DWORD, FILETIME, LPVOID, MAX_PATH};
use winapi::shared::ntdef::{FALSE, NT_SUCCESS};
use winapi::shared::winerror::ERROR_NO_MORE_FILES;
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::memoryapi::ReadProcessMemory;
use winapi::um::processthreadsapi::{
    GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess,
};
use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use winapi::um::realtimeapiset::QueryProcessCycleTime;
use winapi::um::shellapi::CommandLineToArgvW;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::sysinfoapi::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use winapi::um::tlhelp32::*;
use winapi::um::winbase::{LocalFree, QueryFullProcessImageNameW};
use winapi::um::winnt::{
    HANDLE, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    SYNCHRONIZE,
};

/// Manages a Toolhelp32 snapshot handle
struct Snapshot(HANDLE);

impl Snapshot {
    pub fn new() -> io::Result<Self> {
        // SAFETY: TH32CS_SNAPPROCESS and 0 are valid arguments; the returned
        // handle is stored in `Self` and closed on drop.
        let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    pub fn iter(&self) -> ProcIter<'_> {
        ProcIter {
            snapshot: self,
            first: true,
            finished: false,
        }
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a valid snapshot handle obtained from
        // CreateToolhelp32Snapshot.
        unsafe { CloseHandle(self.0) };
    }
}

struct ProcIter<'a> {
    snapshot: &'a Snapshot,
    first: bool,
    finished: bool,
}

impl<'a> Iterator for ProcIter<'a> {
    type Item = io::Result<PROCESSENTRY32W>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        // SAFETY: PROCESSENTRY32W is a `repr(C)` struct of primitive types;
        // zero-initialization is valid. `dwSize` is set immediately after.
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as _;
        let res = if self.first {
            self.first = false;
            // SAFETY: `self.snapshot.0` is a valid snapshot handle;
            // `&mut entry` is a valid out-pointer with `dwSize` set.
            unsafe { Process32FirstW(self.snapshot.0, &mut entry) }
        } else {
            // SAFETY: same as above.
            unsafe { Process32NextW(self.snapshot.0, &mut entry) }
        };
        if res == 0 {
            // Capture the OS error before logging/allocation can overwrite
            // it. Only NO_MORE_FILES means a complete snapshot; a partial
            // list must not be accepted as evidence that Codex has exited.
            let err = io::Error::last_os_error();
            self.finished = true;
            if err.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                None
            } else {
                Some(Err(err))
            }
        } else {
            Some(Ok(entry))
        }
    }
}

fn wstr_to_path(slice: &[u16]) -> PathBuf {
    match slice.iter().position(|&c| c == 0) {
        Some(nul) => OsString::from_wide(&slice[..nul]),
        None => OsString::from_wide(slice),
    }
    .into()
}

fn wstr_to_string(slice: &[u16]) -> String {
    wstr_to_path(slice).to_string_lossy().into_owned()
}

struct ProcParams {
    argv: Vec<String>,
    cwd: PathBuf,
    console: HANDLE,
}

fn wchar_read_len(byte_size: usize) -> Option<usize> {
    let max_bytes = MAX_PATH * 4;
    (byte_size <= max_bytes && byte_size.is_multiple_of(2)).then_some(byte_size / 2)
}

fn finish_wchar_read(mut buf: Vec<u16>, bytes_read: usize) -> Vec<u16> {
    // The API normally reports an exact byte count, but clamp defensively and
    // discard a possible trailing half code unit from a short read.
    let bytes_read = bytes_read.min(buf.len().saturating_mul(2));
    buf.truncate(bytes_read / 2);

    if let Some(nul) = buf.iter().position(|&c| c == 0) {
        buf.truncate(nul + 1);
    } else {
        buf.push(0);
    }
    buf
}

/// A handle to an opened process
struct ProcHandle {
    pid: u32,
    proc: HANDLE,
}

impl ProcHandle {
    pub fn new(pid: u32) -> Option<Self> {
        if pid ==
            // SAFETY: GetCurrentProcessId has no preconditions and no UB.
            unsafe { GetCurrentProcessId() }
        {
            // Avoid the potential for deadlock if we're examining ourselves
            log::trace!("ProcHandle::new({}): skip because it is my own pid", pid);
            return None;
        }
        let options = PROCESS_QUERY_INFORMATION | PROCESS_VM_READ;
        log::trace!("ProcHandle::new({}): OpenProcess", pid);
        // SAFETY: `options` and `pid` are valid values; `FALSE` is a valid BOOL.
        // The returned handle (or NULL) is stored and closed on drop.
        let handle = unsafe { OpenProcess(options, FALSE as _, pid) };
        log::trace!("ProcHandle::new({}): OpenProcess -> {:?}", pid, handle);
        if handle.is_null() {
            return None;
        }
        Some(Self { pid, proc: handle })
    }

    /// Returns the executable image for the process
    pub fn executable(&self) -> Option<PathBuf> {
        let mut buf = [0u16; MAX_PATH + 1];
        let mut len = buf.len() as DWORD;
        // SAFETY: `self.proc` is a valid process handle; `buf` is a valid
        // writable buffer; `&mut len` is a valid out-pointer.
        let res = unsafe { QueryFullProcessImageNameW(self.proc, 0, buf.as_mut_ptr(), &mut len) };
        if res == 0 {
            None
        } else {
            Some(wstr_to_path(&buf))
        }
    }

    /// Wrapper around NtQueryInformationProcess that fetches `what` as `T`
    fn query_proc<T>(&self, what: u32) -> Option<T> {
        let mut data = MaybeUninit::<T>::uninit();
        // SAFETY: `self.proc` is a valid process handle; `data.as_mut_ptr()`
        // is a valid writable pointer with `size_of::<T>()` bytes.
        let res = unsafe {
            NtQueryInformationProcess(
                self.proc,
                what,
                data.as_mut_ptr() as _,
                std::mem::size_of::<T>() as _,
                std::ptr::null_mut(),
            )
        };
        if !NT_SUCCESS(res) {
            return None;
        }
        // SAFETY: NtQueryInformationProcess wrote exactly `size_of::<T>()`
        // bytes on success, so every byte of `data` is initialized. This is
        // only sound because every caller instantiates `T` with a plain
        // winapi POD struct (e.g. `LPVOID`, `PROCESS_BASIC_INFORMATION`) that
        // has no validity invariants beyond "any bit pattern is valid" - this
        // function does not itself constrain `T`, so a future caller must
        // preserve that property.
        let data = unsafe { data.assume_init() };
        Some(data)
    }

    /// Read a `T` from the target process at the specified address
    fn read_struct<T>(&self, addr: LPVOID) -> Option<T> {
        let mut data = MaybeUninit::<T>::uninit();
        // SAFETY: `self.proc` is a valid process handle with PROCESS_VM_READ
        // access; `addr` and `data.as_mut_ptr()` are valid pointers.
        let res = unsafe {
            ReadProcessMemory(
                self.proc,
                addr as _,
                data.as_mut_ptr() as _,
                std::mem::size_of::<T>() as _,
                std::ptr::null_mut(),
            )
        };
        if res == 0 {
            return None;
        }
        // SAFETY: ReadProcessMemory wrote exactly `size_of::<T>()` bytes from
        // the target process into `data` on success. As with `query_proc`
        // above, this is only sound because every caller instantiates `T`
        // with a plain POD struct read from the remote process's memory
        // layout (e.g. `PEB32`) with no validity invariants beyond
        // "any bit pattern is valid" - this function does not itself
        // constrain `T`.
        let data = unsafe { data.assume_init() };
        Some(data)
    }

    /// If the process is a 32-bit process running on Win64, return the address
    /// of its process parameters.
    /// Otherwise, return None to indicate a native win64 process.
    fn get_peb32_addr(&self) -> Option<LPVOID> {
        let peb32_addr: LPVOID = self.query_proc(ProcessWow64Information)?;
        if peb32_addr.is_null() {
            None
        } else {
            Some(peb32_addr)
        }
    }

    /// Returns the cwd and args for the process
    pub fn get_params(&self) -> Option<ProcParams> {
        self.get_params_impl(true)
    }

    fn get_params_impl(&self, include_argv: bool) -> Option<ProcParams> {
        match self.get_peb32_addr() {
            Some(peb32) => self.get_params_32(peb32, include_argv),
            None => self.get_params_64(include_argv),
        }
    }

    fn get_basic_info(&self) -> Option<PROCESS_BASIC_INFORMATION> {
        self.query_proc(ProcessBasicInformation)
    }

    fn get_peb(&self, info: &PROCESS_BASIC_INFORMATION) -> Option<PEB> {
        self.read_struct(info.PebBaseAddress as _)
    }

    fn get_proc_params(&self, peb: &PEB) -> Option<RTL_USER_PROCESS_PARAMETERS> {
        self.read_struct(peb.ProcessParameters as _)
    }

    /// Returns the cwd and args for a 64 bit process
    fn get_params_64(&self, include_argv: bool) -> Option<ProcParams> {
        let info = self.get_basic_info()?;
        let peb = self.get_peb(&info)?;
        let params = self.get_proc_params(&peb)?;

        let argv = read_optional_argv(include_argv, || {
            self.read_process_wchar(
                params.CommandLine.Buffer as _,
                params.CommandLine.Length as _,
            )
        })?;
        let cwd = self.read_process_wchar(
            params.CurrentDirectory.DosPath.Buffer as _,
            params.CurrentDirectory.DosPath.Length as _,
        )?;

        Some(ProcParams {
            argv,
            cwd: wstr_to_path(&cwd),
            console: params.ConsoleHandle,
        })
    }

    fn get_proc_params_32(&self, peb32: LPVOID) -> Option<RTL_USER_PROCESS_PARAMETERS32> {
        self.read_struct(peb32)
    }

    /// Returns the cwd and args for a 32 bit process
    fn get_params_32(&self, peb32: LPVOID, include_argv: bool) -> Option<ProcParams> {
        let params = self.get_proc_params_32(peb32)?;

        let argv = read_optional_argv(include_argv, || {
            self.read_process_wchar(
                params.CommandLine.Buffer as _,
                params.CommandLine.Length as _,
            )
        })?;
        let cwd = self.read_process_wchar(
            params.CurrentDirectory.DosPath.Buffer as _,
            params.CurrentDirectory.DosPath.Length as _,
        )?;

        Some(ProcParams {
            argv,
            cwd: wstr_to_path(&cwd),
            console: params.ConsoleHandle as _,
        })
    }

    /// Copies a sized WSTR from the address in the process.
    ///
    /// `UNICODE_STRING::Length` is a byte count and must be even. Rejecting
    /// malformed sizes before the FFI call keeps the destination allocation
    /// exactly as large as the requested read, while the explicit upper bound
    /// prevents a corrupt remote structure from causing an oversized
    /// allocation.
    fn read_process_wchar(&self, ptr: LPVOID, byte_size: usize) -> Option<Vec<u16>> {
        let wchar_count = wchar_read_len(byte_size)?;
        if wchar_count == 0 {
            return Some(vec![0]);
        }

        let mut buf = vec![0u16; wchar_count];
        let mut bytes_read = 0;

        // SAFETY: `self.proc` is a valid process handle with PROCESS_VM_READ;
        // `ptr` is an address in the target process; `buf.as_mut_ptr()` is a
        // valid writable buffer; `&mut bytes_read` is a valid out-pointer.
        // `wchar_read_len` rejected odd/oversized lengths before this call,
        // so `buf.len() * size_of::<u16>() == byte_size` for this read.
        let res = unsafe {
            ReadProcessMemory(
                self.proc,
                ptr as _,
                buf.as_mut_ptr() as _,
                byte_size,
                &mut bytes_read,
            )
        };
        if res == 0 {
            return None;
        }

        Some(finish_wchar_read(buf, bytes_read))
    }

    /// Retrieves the start time of the process
    fn start_time(&self) -> Option<u64> {
        const fn empty() -> FILETIME {
            FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            }
        }

        let mut start = empty();
        let mut exit = empty();
        let mut kernel = empty();
        let mut user = empty();

        // SAFETY: `self.proc` is a valid process handle; all out-pointers are
        // valid `*mut FILETIME`.
        let res =
            unsafe { GetProcessTimes(self.proc, &mut start, &mut exit, &mut kernel, &mut user) };
        if res == 0 {
            return None;
        }

        Some((start.dwHighDateTime as u64) << 32 | start.dwLowDateTime as u64)
    }
}

/// Cwd-only lookups must not read or parse the remote command line.
fn read_optional_argv(
    include: bool,
    read: impl FnOnce() -> Option<Vec<u16>>,
) -> Option<Vec<String>> {
    if include {
        Some(cmd_line_to_argv(&read()?))
    } else {
        Some(Vec::new())
    }
}

/// Parse a command line string into an argv array
fn cmd_line_to_argv(buf: &[u16]) -> Vec<String> {
    let mut argc = 0;
    // SAFETY: `buf.as_ptr()` is a valid pointer to a NUL-terminated wide
    // string; `&mut argc` is a valid out-pointer.
    let argvp = unsafe { CommandLineToArgvW(buf.as_ptr(), &mut argc) };
    if argvp.is_null() {
        return vec![];
    }

    // SAFETY: CommandLineToArgvW returned a non-null pointer to an array of
    // `argc` wide-string pointers. The array is valid until LocalFree is called.
    let argv = unsafe { std::slice::from_raw_parts(argvp, argc as usize) };
    let mut args = vec![];
    for &arg in argv {
        // SAFETY: `arg` is a valid NUL-terminated wide string pointer returned
        // by CommandLineToArgvW.
        let len = unsafe { libc::wcslen(arg) };
        // SAFETY: `arg` is valid for `len` u16 elements (wcslen counted them).
        let arg = unsafe { std::slice::from_raw_parts(arg, len) };
        args.push(wstr_to_string(arg));
    }
    // SAFETY: `argvp` was returned by CommandLineToArgvW and must be freed
    // via LocalFree.
    unsafe { LocalFree(argvp as _) };
    args
}

impl Drop for ProcHandle {
    fn drop(&mut self) {
        log::trace!("ProcHandle::drop(pid={} proc={:?})", self.pid, self.proc);
        // SAFETY: `self.proc` is a valid handle obtained from OpenProcess.
        unsafe { CloseHandle(self.proc) };
    }
}

mod snapshot;
pub use snapshot::{ProcessActivityStamp, ProcessTreeUsage};
/// Total installed physical RAM, in bytes. Memoized: this does not change
/// for the lifetime of the process, so there is no need to repeat the
/// `GlobalMemoryStatusEx` syscall on every periodic sample.
pub fn total_physical_memory_bytes() -> u64 {
    static TOTAL: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *TOTAL.get_or_init(|| {
        // SAFETY: MEMORYSTATUSEX is a plain POD struct of integers; the
        // all-zero bit pattern is a valid value, immediately overwritten
        // below (`dwLength` is required to be set before the API call).
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: `&mut status` is a valid out-pointer with `dwLength` set to
        // its own size, as the API requires.
        let res = unsafe { GlobalMemoryStatusEx(&mut status) };
        if res == 0 {
            0
        } else {
            status.ullTotalPhys
        }
    })
}
