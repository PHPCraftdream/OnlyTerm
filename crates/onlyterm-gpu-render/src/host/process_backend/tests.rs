use super::*;
use crate::{FrameForm, RenderBackend};
use std::sync::atomic::Ordering;
use std::sync::Once;
use std::time::{Duration, Instant};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP,
};

/// Resolves the real `onlyterm-gui.exe` next to the test harness binary.
/// `std::env::current_exe()` under `cargo test` returns the harness
/// itself (which has no `gpu-tab-host` subcommand -- it parses its own
/// test-filter arguments instead), not the plain GUI executable, so
/// these tests need the real one built first (`cargo build -p
/// onlyterm-gui`, already a prerequisite for every check this session).
fn real_gui_exe() -> std::path::PathBuf {
    let mut path = std::env::current_exe().expect("current_exe");
    path.pop();
    if path.file_name().and_then(|n| n.to_str()) == Some("deps") {
        path.pop();
    }
    path.push("onlyterm-gui.exe");
    if path.exists() {
        return path;
    }

    // CI (windows_continuous) builds onlyterm-gui.exe as a separate,
    // explicit `cargo build --release` step before `cargo nextest run`
    // (no `--release`) builds and runs this very test in the debug
    // profile -- nextest's own build of this package for its test
    // harness does not also emit a plain (non-test) `target/debug/
    // onlyterm-gui.exe` binary, so the debug-profile path above is
    // absent even though the exe genuinely exists, just under
    // `target/release/` from that earlier step in the same job. Try
    // that before giving up.
    if path.parent().and_then(|p| p.file_name()) == Some(std::ffi::OsStr::new("debug")) {
        let release_path = path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("release")
            .join("onlyterm-gui.exe");
        if release_path.exists() {
            return release_path;
        }
    }

    panic!(
        "expected the real onlyterm-gui.exe already built at {:?} (also checked the \
             release profile) -- run `cargo build -p onlyterm-gui` first",
        path
    );
}

static REGISTER_CLASS: Once = Once::new();

unsafe extern "system" fn test_wndproc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    // SAFETY: forwarding the exact arguments the OS just handed this
    // window procedure to the default handler; no additional invariant
    // to uphold beyond what the OS already guarantees for any wndproc call.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// A minimal, invisible Win32 window good enough to hand
/// `IDCompositionDesktopDevice::CreateTargetForHwnd` -- see
/// `HostProcessBackend::spawn_for_hwnd_with_exe`'s doc comment for why a
/// real `::window::Window` isn't used here.
fn create_test_hwnd() -> isize {
    // SAFETY: `RegisterClassW`/`CreateWindowExW` are standard Win32 calls;
    // `class_name`/`instance` outlive this block (`w!` is a `'static`
    // wide-string literal, `instance` comes from `GetModuleHandleW`), and
    // `test_wndproc` matches the required `WNDPROC` signature.
    unsafe {
        let instance = GetModuleHandleW(None).expect("GetModuleHandleW");
        let class_name = windows::core::w!("HostProcessBackendTestWindow");
        REGISTER_CLASS.call_once(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(test_wndproc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassW(&wc);
        });
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            windows::core::w!("host-process-backend-test"),
            WS_POPUP,
            0,
            0,
            64,
            64,
            None,
            None,
            instance,
            None,
        )
        .expect("CreateWindowExW");
        hwnd.0 as isize
    }
}

/// Terminates a process by the exact PID the caller captured at spawn
/// time -- never by image name. These are `onlyterm-gui.exe` (in
/// `gpu-tab-host` mode) processes this test itself spawned moments ago
/// via `HostProcessBackend`, which is exactly the documented exception
/// to this repo's "never kill onlyterm-gui.exe" rule.
fn kill_process_by_captured_pid(pid: u32) {
    // SAFETY: `pid` is the exact PID this test captured moments ago from
    // its own `HostProcessBackend`'s `debug_current_child_pid()`, not
    // discovered by name/enumeration; `handle` is closed before this
    // block ends.
    unsafe {
        let handle =
            OpenProcess(PROCESS_TERMINATE, false, pid).expect("OpenProcess on our own child");
        TerminateProcess(handle, 1).ok();
        let _ = windows::Win32::Foundation::CloseHandle(handle);
    }
}

fn wait_until(mut pred: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if pred() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn spawn_test_backend() -> HostProcessBackend {
    let hwnd = create_test_hwnd();
    let dimensions = Dimensions {
        pixel_width: 320,
        pixel_height: 240,
        dpi: 96,
    };
    HostProcessBackend::spawn_for_hwnd_with_exe(
        hwnd,
        dimensions,
        Box::new(|| {}),
        Box::new(|| {}),
        real_gui_exe(),
    )
    .expect("HostProcessBackend::spawn_for_hwnd_with_exe should succeed on this machine")
}

#[test]
fn a_forcibly_killed_child_is_respawned_without_the_backend_dying() {
    let backend = spawn_test_backend();
    assert!(!backend.render_thread_has_died());

    let pid_before = wait_until_child_pid(&backend).expect("a child should be running");
    kill_process_by_captured_pid(pid_before);

    assert!(
        wait_until(
            || backend
                .debug_current_child_pid()
                .is_some_and(|pid| pid != pid_before),
            Duration::from_secs(10)
        ),
        "expected a new child PID within 10s of forcibly killing the old one"
    );
    assert!(
        !backend.render_thread_has_died(),
        "a single forced death within budget must not demote the backend"
    );
}

#[test]
fn repeated_forced_deaths_exhaust_the_budget_and_demote_without_panicking() {
    let backend = spawn_test_backend();

    // Kill and wait for each respawn to actually land (a new, distinct
    // PID) before the next kill -- otherwise a kill can race an
    // in-flight respawn and land on an already-dead PID, undercounting
    // real death events against the budget.
    for _ in 0..MAX_RESPAWNS_PER_WINDOW {
        let pid = wait_until_child_pid(&backend).expect("a child should be running");
        kill_process_by_captured_pid(pid);
        assert!(
            wait_until(
                || backend
                    .debug_current_child_pid()
                    .is_some_and(|new_pid| new_pid != pid),
                Duration::from_secs(10)
            ),
            "expected a respawn within budget"
        );
    }

    // This last kill is the one that exhausts the budget -- the backend
    // demotes instead of respawning again, so there is no new PID to
    // wait for here, only the death report flipping.
    let pid = wait_until_child_pid(&backend).expect("a child should be running");
    kill_process_by_captured_pid(pid);

    assert!(
        wait_until(|| backend.render_thread_has_died(), Duration::from_secs(15)),
        "expected the backend to demote itself after exhausting its respawn budget \
             ({} respawns within {:?})",
        MAX_RESPAWNS_PER_WINDOW,
        RESPAWN_WINDOW
    );
}

/// Finding C's parent-half contract: a submitted frame that never gets
/// an ack reads as hung once it outlives the configured hang threshold
/// (so the window's supervisor can rebuild), but a respawn that is
/// already in flight gates the check off -- its backoff window is what
/// resolves the stall, and a full rebuild piled on top of it would
/// defeat it.
#[test]
fn render_thread_is_hung_tracks_unacked_submit_unless_a_respawn_is_pending() {
    let backend = spawn_test_backend();
    let shared = &backend.shared;

    // Nothing submitted: never hung.
    assert!(!backend.render_thread_is_hung());

    // A frame submitted longer ago than the configured hang threshold
    // and never acked: hung.
    let over_threshold =
        Duration::from_millis(config::configuration().render_thread_hang_threshold_ms + 1_000);
    *shared.submit_started_at.lock() = Some(Instant::now() - over_threshold);
    assert!(
        backend.render_thread_is_hung(),
        "an un-acked frame older than the hang threshold must read as hung"
    );

    // ...unless a respawn is already in flight, which resets the clock
    // itself and must not be stomped by the supervisor.
    shared.respawn_pending.store(true, Ordering::SeqCst);
    assert!(
        !backend.render_thread_is_hung(),
        "a pending respawn must gate the hang check off"
    );

    // Once the respawn attempt has run (gate cleared), the stale frame
    // is what the supervisor sees again.
    shared.respawn_pending.store(false, Ordering::SeqCst);
    assert!(
        backend.render_thread_is_hung(),
        "the hang check must come back once the respawn gate clears"
    );
}

/// Finding D's backend-half contract: a fresh generation must answer
/// `full_resync: true` until its first frame is built. The GUI skips
/// identical frames by signature, and that skip must never swallow the
/// first frame of a new generation -- it is what carries the atlas
/// reset and produces the first ack that swaps the DirectComposition
/// visual. The same flag is what the in-frame-loss paths (a send that
/// could not be queued, a child-reported recoverable submit failure)
/// re-arm, so the replacement frame also bypasses the signature skip.
#[test]
fn frame_form_requests_full_resync_until_the_first_frame_is_built() {
    let backend = spawn_test_backend();

    assert!(
        matches!(backend.frame_form(), FrameForm::Wire { full_resync: true }),
        "a fresh backend must request a full resync"
    );
    // Consumed by the ask: the next frame is a normal incremental one.
    assert!(
        matches!(backend.frame_form(), FrameForm::Wire { full_resync: false }),
        "after the resync frame is built, frame_form must go back to normal"
    );

    // Re-arming the flag (what on_failed / send_frame's queue-failure
    // path do) makes the next frame a full resync again.
    backend
        .shared
        .needs_full_resync
        .store(true, Ordering::Release);
    assert!(matches!(
        backend.frame_form(),
        FrameForm::Wire { full_resync: true }
    ));
}

/// Waits (briefly) for a child to actually be running and returns its
/// PID -- immediately after spawning, `debug_current_child_pid` can
/// observe a moment where the previous generation was just replaced and
/// the new `Child` handle isn't stored yet.
fn wait_until_child_pid(backend: &HostProcessBackend) -> Option<u32> {
    let mut pid = None;
    wait_until(
        || {
            pid = backend.debug_current_child_pid();
            pid.is_some()
        },
        Duration::from_secs(5),
    );
    pid
}
