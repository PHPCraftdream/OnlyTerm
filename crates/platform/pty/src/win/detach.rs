use super::procthreadattr::ProcThreadAttributeList;
use crate::cmdbuilder::CommandBuilder;
use anyhow::{bail, ensure, Context};
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle, RawHandle};
use std::path::Path;
use std::sync::Arc;
use std::{io, mem, ptr};
use winapi::shared::minwindef::{BOOL, DWORD, FALSE, FILETIME, TRUE};
use winapi::shared::winerror::{ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, ERROR_NO_MORE_FILES};
use winapi::um::handleapi::{DuplicateHandle, GetHandleInformation, INVALID_HANDLE_VALUE};
use winapi::um::jobapi::IsProcessInJob;
use winapi::um::jobapi2::{AssignProcessToJobObject, CreateJobObjectW, QueryInformationJobObject};
use winapi::um::processthreadsapi::{
    CreateProcessW, GetCurrentProcess, GetProcessId, GetProcessTimes, OpenProcess,
    TerminateProcess, PROCESS_INFORMATION,
};
use winapi::um::synchapi::{CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject};
use winapi::um::tlhelp32::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use winapi::um::winbase::{
    QueryFullProcessImageNameW, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, HANDLE_FLAG_INHERIT, INFINITE, STARTF_USESTDHANDLES,
    STARTUPINFOEXW,
};
use winapi::um::winnt::{
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList, DUPLICATE_SAME_ACCESS,
    HANDLE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
};

pub const PROCESS_KEEPER_ARG: &str = "--pty-process-keeper";
const WAIT_OBJECT_0: DWORD = 0;
const WAIT_TIMEOUT: DWORD = 258;
const SYNCHRONIZE: DWORD = 0x00100000;
const HANDLE_COUNT: usize = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub created: u64,
}

#[derive(Clone, Debug)]
pub struct PtyProcessInfo {
    pub identity: ProcessIdentity,
    pub parent_pid: u32,
    pub name: String,
    pub detached: bool,
}

pub(crate) fn process_in_job(process: HANDLE, job: HANDLE) -> anyhow::Result<bool> {
    let mut member: BOOL = FALSE;
    // SAFETY: Both handles are live; IsProcessInJob only writes the BOOL output.
    let ok = unsafe { IsProcessInJob(process, job, &mut member) };
    if ok == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(member != FALSE)
}

fn process_created(process: HANDLE) -> anyhow::Result<u64> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut created = zero;
    let mut exited = zero;
    let mut kernel = zero;
    let mut user = zero;
    // SAFETY: The process handle permits queries; all four FILETIME outputs are live.
    let ok = unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
    if ok == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

fn open_process(pid: u32, rights: DWORD) -> anyhow::Result<Option<OwnedHandle>> {
    // SAFETY: OpenProcess takes a PID and access mask; no pointer inputs are used.
    let raw = unsafe { OpenProcess(rights, FALSE, pid) };
    if raw.is_null() {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            return Ok(None);
        }
        return Err(error.into());
    }
    // SAFETY: OpenProcess returned a new, uniquely owned process handle.
    Ok(Some(unsafe { OwnedHandle::from_raw_handle(raw as _) }))
}

pub(crate) fn job_process_ids(job: HANDLE) -> anyhow::Result<Vec<u32>> {
    let offset = mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList);
    let header_words = offset.div_ceil(mem::size_of::<usize>());
    let mut capacity = 32usize;
    loop {
        let words = header_words
            .checked_add(capacity)
            .context("Process list overflow")?;
        let mut storage = vec![0usize; words];
        let bytes = DWORD::try_from(
            words
                .checked_mul(mem::size_of::<usize>())
                .context("Process list overflow")?,
        )?;
        // SAFETY: Pointer-aligned initialized storage fits the variable-length WinAPI
        // structure; QueryInformationJobObject does not retain this buffer.
        let ok = unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicProcessIdList,
                storage.as_mut_ptr().cast(),
                bytes,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) {
                capacity = capacity.checked_mul(2).context("Process list overflow")?;
                continue;
            }
            return Err(error.into());
        }
        // SAFETY: A successful query initialized the header in the aligned buffer.
        let count = unsafe {
            (*storage.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>()).NumberOfProcessIdsInList
        } as usize;
        ensure!(count <= capacity, "Invalid job process count");
        return storage[header_words..header_words + count]
            .iter()
            .map(|&pid| u32::try_from(pid).map_err(Into::into))
            .collect();
    }
}

fn active_processes(job: HANDLE) -> anyhow::Result<DWORD> {
    // SAFETY: This WinAPI accounting structure contains only zero-valid scalar fields.
    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { mem::zeroed() };
    // SAFETY: The job handle and fixed-size writable output are valid.
    let ok = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            mem::size_of_val(&info) as DWORD,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(info.ActiveProcesses)
}

fn parent_processes() -> anyhow::Result<HashMap<u32, u32>> {
    // SAFETY: TH32CS_SNAPPROCESS requests a snapshot, with no borrowed storage.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: A successful snapshot call returned an owned handle.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw as _) };
    // SAFETY: PROCESSENTRY32W consists of zero-valid scalars and a WCHAR array.
    let mut entry: PROCESSENTRY32W = unsafe { mem::zeroed() };
    entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as DWORD;
    let mut parents = HashMap::new();
    // SAFETY: The snapshot is live and the initialized entry declares its size.
    let mut ok = unsafe { Process32FirstW(snapshot.as_raw_handle() as _, &mut entry) };
    loop {
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                return Err(error.into());
            }
            break;
        }
        parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
        // SAFETY: The same live snapshot and correctly sized entry remain valid.
        ok = unsafe { Process32NextW(snapshot.as_raw_handle() as _, &mut entry) };
    }
    Ok(parents)
}

fn process_name(process: HANDLE) -> String {
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as DWORD;
    // SAFETY: The query writes at most `length` WCHARs to the live buffer.
    let ok = unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
    if ok == 0 {
        return "(имя недоступно)".to_string();
    }
    let path = std::ffi::OsString::from_wide(&buffer[..length as usize]);
    Path::new(&path)
        .file_name()
        .unwrap_or(&path)
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn processes(
    job: HANDLE,
    protected: Option<HANDLE>,
) -> anyhow::Result<Vec<PtyProcessInfo>> {
    let parents = parent_processes()?;
    let mut result = vec![];
    for pid in job_process_ids(job)? {
        let Some(process) = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE)?
        else {
            continue;
        };
        // SAFETY: This owned process handle permits synchronization.
        if unsafe { WaitForSingleObject(process.as_raw_handle() as _, 0) } == WAIT_OBJECT_0 {
            continue;
        }
        if !process_in_job(process.as_raw_handle() as _, job)? {
            continue;
        }
        let created = process_created(process.as_raw_handle() as _)?;
        let detached = match protected {
            Some(protected) => process_in_job(process.as_raw_handle() as _, protected)?,
            None => false,
        };
        result.push(PtyProcessInfo {
            identity: ProcessIdentity { pid, created },
            parent_pid: parents.get(&pid).copied().unwrap_or(0),
            name: process_name(process.as_raw_handle() as _),
            detached,
        });
    }
    Ok(result)
}

pub(crate) fn selected_processes(
    job: HANDLE,
    selected: &[ProcessIdentity],
) -> anyhow::Result<Vec<(ProcessIdentity, OwnedHandle)>> {
    let rows = processes(job, None)?;
    let by_pid: HashMap<_, _> = rows.iter().map(|row| (row.identity.pid, row)).collect();
    for identity in selected {
        ensure!(
            by_pid
                .get(&identity.pid)
                .is_some_and(|row| row.identity == *identity),
            "PID {} завершился или сменил владельца",
            identity.pid
        );
    }
    let mut result = vec![];
    for row in &rows {
        let mut current = Some(row);
        let mut matched = false;
        for _ in 0..rows.len() {
            let Some(ancestor) = current else { break };
            if selected.contains(&ancestor.identity) {
                matched = true;
                break;
            }
            current = by_pid
                .get(&ancestor.parent_pid)
                .copied()
                .filter(|parent| parent.identity.created < ancestor.identity.created);
        }
        if !matched {
            continue;
        }
        let process = open_process(
            row.identity.pid,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA | PROCESS_TERMINATE | SYNCHRONIZE,
        )?
        .with_context(|| format!("PID {} уже завершился", row.identity.pid))?;
        ensure!(
            process_created(process.as_raw_handle() as _)? == row.identity.created
                && process_in_job(process.as_raw_handle() as _, job)?,
            "PID {} больше не принадлежит вкладке",
            row.identity.pid
        );
        result.push((row.identity, process));
    }
    ensure!(!result.is_empty(), "Не выбраны работающие процессы");
    Ok(result)
}

fn create_event() -> anyhow::Result<OwnedHandle> {
    // SAFETY: Null attributes/name request an unnamed non-inheritable manual-reset event.
    let raw = unsafe { CreateEventW(ptr::null_mut(), TRUE, FALSE, ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: CreateEventW returned a new owned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw as _) })
}

fn duplicate_handle(handle: HANDLE, inherit: BOOL) -> anyhow::Result<OwnedHandle> {
    let mut duplicate = ptr::null_mut();
    // SAFETY: The source is live in this process; DuplicateHandle returns a new
    // independently owned handle with the same rights and explicit inheritance.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut duplicate,
            0,
            inherit,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: DuplicateHandle transferred unique ownership of its returned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(duplicate as _) })
}

pub(crate) fn duplicate_kernel_handle(handle: HANDLE) -> anyhow::Result<OwnedHandle> {
    duplicate_handle(handle, FALSE)
}

fn duplicate_inheritable(handle: HANDLE) -> anyhow::Result<OwnedHandle> {
    duplicate_handle(handle, TRUE)
}

pub(crate) struct DetachedSession {
    pub root_job: OwnedHandle,
    pub protected_job: OwnedHandle,
    process: OwnedHandle,
    closed: OwnedHandle,
    committed: OwnedHandle,
}

impl DetachedSession {
    pub fn launch(
        helper: &Path,
        conpty: [HANDLE; 3],
        input: HANDLE,
        output: HANDLE,
        job: HANDLE,
    ) -> anyhow::Result<Self> {
        // SAFETY: Null attributes/name request a new unnamed job with no UI limits.
        let raw = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        // SAFETY: The new job handle is uniquely owned by this session.
        let protected_job = unsafe { OwnedHandle::from_raw_handle(raw as _) };
        let root_job = duplicate_kernel_handle(job)?;
        let closed = create_event()?;
        let ready = create_event()?;
        let committed = create_event()?;
        // SAFETY: GetCurrentProcess is a valid pseudo-handle, accepted by DuplicateHandle.
        let owner = unsafe { GetCurrentProcess() };
        let raw_handles = [
            conpty[0],
            conpty[1],
            conpty[2],
            input,
            output,
            job,
            protected_job.as_raw_handle() as _,
            closed.as_raw_handle() as _,
            ready.as_raw_handle() as _,
            committed.as_raw_handle() as _,
            owner,
        ];
        let inherited: Vec<OwnedHandle> = raw_handles
            .iter()
            .map(|&h| duplicate_inheritable(h))
            .collect::<anyhow::Result<_>>()?;
        let values: Vec<HANDLE> = inherited.iter().map(|h| h.as_raw_handle() as _).collect();
        let mut attributes = ProcThreadAttributeList::with_capacity(1)?;
        attributes.set_inherited_handles(values)?;
        let mut command = CommandBuilder::new(helper);
        command.arg(PROCESS_KEEPER_ARG);
        for handle in &inherited {
            command.arg((handle.as_raw_handle() as usize).to_string());
        }
        let (mut exe, mut command_line) = command.cmdline()?;
        // SAFETY: STARTUPINFOEXW and PROCESS_INFORMATION contain zero-valid fields.
        let mut startup: STARTUPINFOEXW = unsafe { mem::zeroed() };
        // SAFETY: The output struct is zero-valid and exclusively owned here.
        let mut info: PROCESS_INFORMATION = unsafe { mem::zeroed() };
        startup.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as DWORD;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
        startup.lpAttributeList = attributes.as_mut_ptr();
        // SAFETY: The command/environment buffers and attribute storage outlive
        // CreateProcessW. HANDLE_LIST limits inheritance to the eleven owned copies.
        let ok = unsafe {
            CreateProcessW(
                exe.as_mut_ptr(),
                command_line.as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                TRUE,
                CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                command.environment_block().as_mut_ptr().cast(),
                ptr::null(),
                &mut startup.StartupInfo,
                &mut info,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error().into());
        }
        // SAFETY: Successful CreateProcessW returned two independently owned handles.
        let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess as _) };
        // SAFETY: The new thread handle is owned by this caller and no longer needed.
        drop(unsafe { OwnedHandle::from_raw_handle(info.hThread as _) });
        let wait_handles = [
            ready.as_raw_handle() as HANDLE,
            process.as_raw_handle() as HANDLE,
        ];
        // SAFETY: Both synchronization handles and the array remain valid throughout the wait.
        let wait = unsafe { WaitForMultipleObjects(2, wait_handles.as_ptr(), FALSE, 5000) };
        if wait != WAIT_OBJECT_0 {
            // No processes have been assigned to the protected job at this point.
            // SAFETY: This is the exact helper process handle returned by our CreateProcessW.
            unsafe { TerminateProcess(process.as_raw_handle() as _, 1) };
            bail!(
                "Не удалось запустить держатель процессов (PID {})",
                info.dwProcessId
            );
        }
        log::info!("Process keeper started as PID {}", info.dwProcessId);
        Ok(Self {
            root_job,
            protected_job,
            process,
            closed,
            committed,
        })
    }

    pub fn is_alive(&self) -> anyhow::Result<bool> {
        // SAFETY: The owned helper process handle permits synchronization.
        match unsafe { WaitForSingleObject(self.process.as_raw_handle() as _, 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(io::Error::last_os_error().into()),
        }
    }

    pub fn commit(&self) -> anyhow::Result<()> {
        // SAFETY: The owned manual-reset event is live and writable.
        if unsafe { SetEvent(self.committed.as_raw_handle() as _) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }

    pub fn close(&self) -> anyhow::Result<()> {
        // SAFETY: The owned manual-reset event is live and writable.
        if unsafe { SetEvent(self.closed.as_raw_handle() as _) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }
}

pub(crate) fn assign_protected(process: HANDLE, job: HANDLE) -> anyhow::Result<()> {
    if process_in_job(process, job)? {
        return Ok(());
    }
    // SAFETY: The owned process has SET_QUOTA/TERMINATE access; the new job has
    // no UI limits. Windows validates whether its existing job chain can be nested.
    let ok = unsafe { AssignProcessToJobObject(job, process) };
    if ok == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

fn close_attached_processes(root: HANDLE, protected: HANDLE) -> anyhow::Result<()> {
    for pid in job_process_ids(root)? {
        let process = match open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE)
        {
            Ok(Some(process)) => process,
            Ok(None) => continue,
            Err(error) => {
                log::error!("Cannot inspect attached PID {pid}: {error:#}");
                continue;
            }
        };
        let attached = process_in_job(process.as_raw_handle() as _, root).and_then(|member| {
            if member {
                process_in_job(process.as_raw_handle() as _, protected).map(|protected| !protected)
            } else {
                Ok(false)
            }
        });
        let attached = match attached {
            Ok(attached) => attached,
            Err(error) => {
                log::error!("Cannot verify attached PID {pid}: {error:#}");
                continue;
            }
        };
        if attached {
            // SAFETY: This owned handle was verified against this PTY's root job
            // and excluded from its protected job; PID reuse cannot change the handle.
            let ok = unsafe { TerminateProcess(process.as_raw_handle() as _, 1) };
            if ok == 0 {
                let error = io::Error::last_os_error();
                log::warn!("Cannot close attached PID {pid}: {error}");
            }
        }
    }
    Ok(())
}

fn wait_for_commit(
    committed: &OwnedHandle,
    owner: &OwnedHandle,
    closed: &OwnedHandle,
) -> anyhow::Result<bool> {
    let handles = [
        committed.as_raw_handle() as HANDLE,
        owner.as_raw_handle() as HANDLE,
        closed.as_raw_handle() as HANDLE,
    ];
    // SAFETY: All three owned synchronization handles outlive the blocking wait.
    match unsafe { WaitForMultipleObjects(3, handles.as_ptr(), FALSE, INFINITE) } {
        WAIT_OBJECT_0 | 1 => Ok(true),
        2 => Ok(false),
        _ => Err(io::Error::last_os_error().into()),
    }
}

/// Private entry point for the GUI executable's inherited-handle keeper mode.
pub fn run_process_keeper() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args_os().skip(2).collect();
    ensure!(
        arguments.len() == HANDLE_COUNT,
        "Invalid process keeper handle count"
    );
    let mut owned = vec![];
    for argument in arguments {
        let value: usize = argument
            .to_str()
            .context("Invalid inherited handle")?
            .parse()?;
        let raw = value as RawHandle;
        let mut flags = 0;
        // SAFETY: GetHandleInformation validates an opaque kernel handle value;
        // it does not dereference the pointer-shaped HANDLE.
        let ok = unsafe { GetHandleInformation(raw as _, &mut flags) };
        if ok == 0 {
            return Err(io::Error::last_os_error().into());
        }
        ensure!(
            flags & HANDLE_FLAG_INHERIT != 0,
            "Keeper handle was not inherited"
        );
        // SAFETY: These eleven HANDLE_LIST entries were duplicated specifically
        // for this child. Each validated inherited handle is adopted exactly once.
        owned.push(unsafe { OwnedHandle::from_raw_handle(raw) });
    }
    let [signal, reference, console, input, output, root, protected, closed, ready, committed, owner]: [OwnedHandle; HANDLE_COUNT] = owned.try_into().map_err(|_| anyhow::anyhow!("Invalid keeper handles"))?;
    // Query both jobs before acknowledging startup; incorrect handle types fail here.
    active_processes(root.as_raw_handle() as _)?;
    active_processes(protected.as_raw_handle() as _)?;
    // SAFETY: The inherited owner is a process handle; a query validates its type.
    if unsafe { GetProcessId(owner.as_raw_handle() as _) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let committed = Arc::new(committed);
    let owner = Arc::new(owner);
    let closed = Arc::new(closed);
    // SAFETY: This is the uniquely inherited read pipe. File adopts it without
    // probing ConPTY handles or querying a pipe with a pending read.
    let mut reader = unsafe { std::fs::File::from_raw_handle(output.into_raw_handle()) };
    let drain_commit = Arc::clone(&committed);
    let drain_owner = Arc::clone(&owner);
    let drain_closed = Arc::clone(&closed);
    let drain = std::thread::Builder::new()
        .name("detached-pty-drain".into())
        .spawn(move || {
            match wait_for_commit(&drain_commit, &drain_owner, &drain_closed) {
                Ok(true) => {}
                Ok(false) => return,
                Err(error) => {
                    log::error!("Detached PTY startup wait failed: {error:#}");
                    return;
                }
            }
            let handles = [
                drain_closed.as_raw_handle() as HANDLE,
                drain_owner.as_raw_handle() as HANDLE,
            ];
            // SAFETY: The Arc-owned handles remain valid throughout the wait.
            let ended = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), FALSE, INFINITE) };
            if ended > 1 {
                log::error!(
                    "Detached PTY lifetime wait failed: {}",
                    io::Error::last_os_error()
                );
                return;
            }
            if let Err(error) = io::copy(&mut reader, &mut io::sink()) {
                log::warn!("Detached PTY output ended: {error}");
            }
        })?;
    // Process-bound drainer; it never consumes output before the pane/GUI closes.
    drop(drain);
    // SAFETY: The inherited ready event is live; all startup resources now exist.
    if unsafe { SetEvent(ready.as_raw_handle() as _) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    if !wait_for_commit(&committed, &owner, &closed)? {
        return Ok(());
    }
    let lifetime = [
        closed.as_raw_handle() as HANDLE,
        owner.as_raw_handle() as HANDLE,
    ];
    // SAFETY: Both Arc-owned synchronization handles remain valid.
    let ended = unsafe { WaitForMultipleObjects(2, lifetime.as_ptr(), FALSE, INFINITE) };
    ensure!(ended <= 1, "Keeper lifetime wait failed");
    if let Err(error) =
        close_attached_processes(root.as_raw_handle() as _, protected.as_raw_handle() as _)
    {
        // Cleanup errors must not release the kill-on-close job under protected processes.
        log::error!("Attached process cleanup failed: {error:#}");
    }
    while active_processes(protected.as_raw_handle() as _)? != 0 {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    // Root kill-on-close cleans up any unprotected descendants created during shutdown.
    drop(root);
    drop(protected);
    drop(input);
    drop(signal);
    drop(reference);
    drop(console);
    log::info!("Process keeper finished");
    Ok(())
}

#[cfg(test)]
#[path = "detach_tests.rs"]
mod tests;
