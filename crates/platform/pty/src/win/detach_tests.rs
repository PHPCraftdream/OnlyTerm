use super::*;
use crate::win::pseudocon::create_kill_on_close_job;
use winapi::um::winbase::CREATE_SUSPENDED;

struct TestProcess {
    handle: OwnedHandle,
    _thread: OwnedHandle,
    pid: u32,
}

impl TestProcess {
    fn spawn(job: Option<HANDLE>) -> Self {
        let mut command: Vec<u16> = "cmd.exe /c exit".encode_utf16().chain(Some(0)).collect();
        // SAFETY: Both WinAPI structs contain only zero-valid fields.
        let mut startup: STARTUPINFOEXW = unsafe { mem::zeroed() };
        // SAFETY: The exclusively owned PROCESS_INFORMATION is a zero-valid output.
        let mut info: PROCESS_INFORMATION = unsafe { mem::zeroed() };
        startup.StartupInfo.cb = mem::size_of_val(&startup.StartupInfo) as DWORD;
        // SAFETY: The command is mutable/NUL-terminated and output storage is valid.
        // CREATE_SUSPENDED keeps this owned fixture deterministic and idle.
        let ok = unsafe {
            CreateProcessW(
                ptr::null(),
                command.as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                FALSE,
                CREATE_SUSPENDED | CREATE_NO_WINDOW,
                ptr::null_mut(),
                ptr::null(),
                &mut startup.StartupInfo,
                &mut info,
            )
        };
        assert_ne!(
            ok,
            0,
            "Cannot create owned test process: {}",
            io::Error::last_os_error()
        );
        eprintln!("Owned detachment test PID {}", info.dwProcessId);
        // SAFETY: Successful CreateProcessW transferred these two handles to us.
        let handle = unsafe { OwnedHandle::from_raw_handle(info.hProcess as _) };
        // SAFETY: The new thread handle is separately owned by this fixture.
        let thread = unsafe { OwnedHandle::from_raw_handle(info.hThread as _) };
        let child = Self {
            handle,
            _thread: thread,
            pid: info.dwProcessId,
        };
        if let Some(job) = job {
            assign_protected(child.handle.as_raw_handle() as _, job).unwrap();
        }
        child
    }

    fn wait(&self, timeout: DWORD) -> DWORD {
        // SAFETY: This is the exact owned process handle captured at fixture launch.
        unsafe { WaitForSingleObject(self.handle.as_raw_handle() as _, timeout) }
    }
}

impl Drop for TestProcess {
    fn drop(&mut self) {
        if self.wait(0) == WAIT_TIMEOUT {
            // SAFETY: Only this fixture's launched PID is terminated through its
            // original owned handle; no process discovery or image-name kill occurs.
            unsafe { TerminateProcess(self.handle.as_raw_handle() as _, 1) };
            self.wait(5000);
        }
    }
}

#[test]
fn retained_job_preserves_only_protected_processes_until_holder_exits() {
    let root = create_kill_on_close_job().unwrap();
    // SAFETY: Null attributes/name request a new non-inheritable empty job.
    let raw = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
    assert!(!raw.is_null());
    // SAFETY: The newly created job is uniquely owned by this test.
    let protected = unsafe { OwnedHandle::from_raw_handle(raw as _) };
    let selected = TestProcess::spawn(Some(root.as_raw_handle() as _));
    let attached = TestProcess::spawn(Some(root.as_raw_handle() as _));
    assign_protected(
        selected.handle.as_raw_handle() as _,
        protected.as_raw_handle() as _,
    )
    .unwrap();
    let holder = root.try_clone().unwrap();
    drop(root);
    close_attached_processes(holder.as_raw_handle() as _, protected.as_raw_handle() as _).unwrap();
    assert_eq!(attached.wait(5000), WAIT_OBJECT_0);
    assert_eq!(selected.wait(0), WAIT_TIMEOUT);
    drop(holder);
    assert_eq!(selected.wait(5000), WAIT_OBJECT_0);
}

#[test]
fn detachment_rejects_recycled_identity_and_processes_outside_the_pane_job() {
    let root = create_kill_on_close_job().unwrap();
    let inside = TestProcess::spawn(Some(root.as_raw_handle() as _));
    let outside = TestProcess::spawn(None);
    let created = process_created(inside.handle.as_raw_handle() as _).unwrap();
    let stale = ProcessIdentity {
        pid: inside.pid,
        created: created ^ 1,
    };
    assert!(selected_processes(root.as_raw_handle() as _, &[stale]).is_err());
    let foreign = ProcessIdentity {
        pid: outside.pid,
        created: process_created(outside.handle.as_raw_handle() as _).unwrap(),
    };
    assert!(selected_processes(root.as_raw_handle() as _, &[foreign]).is_err());
    assert_eq!(inside.wait(0), WAIT_TIMEOUT);
    assert_eq!(outside.wait(0), WAIT_TIMEOUT);
}

#[test]
fn killer_without_process_handle_still_reaps_its_owned_job_children() {
    use crate::ChildKiller;
    let root = create_kill_on_close_job().unwrap();
    let child = TestProcess::spawn(Some(root.as_raw_handle() as _));
    let mut killer = crate::win::WinChildKiller {
        proc: None,
        job: Arc::new(std::sync::Mutex::new(crate::win::JobState::new(Some(root)))),
    };
    let mut cloned = killer.clone_killer();
    cloned.kill().unwrap();
    assert_eq!(child.wait(5000), WAIT_OBJECT_0);
    killer.kill().unwrap();
}
