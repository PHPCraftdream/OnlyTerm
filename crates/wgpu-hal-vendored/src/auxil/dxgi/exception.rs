use std::{
    borrow::Cow,
    cell::Cell,
    string::{String, ToString as _},
};

use parking_lot::Mutex;
use windows::Win32::{Foundation, System::Diagnostics::Debug};

// This is a mutex as opposed to an atomic as we need to completely
// lock everyone out until we have registered or unregistered the
// exception handler, otherwise really nasty races could happen.
//
// By routing all the registration through these functions we can guarantee
// there is either 1 or 0 exception handlers registered, not multiple.
static EXCEPTION_HANDLER_COUNT: Mutex<usize> = Mutex::new(0);

pub fn register_exception_handler() {
    let mut count_guard = EXCEPTION_HANDLER_COUNT.lock();
    if *count_guard == 0 {
        unsafe { Debug::AddVectoredExceptionHandler(0, Some(output_debug_string_handler)) };
    }
    *count_guard += 1;
}

pub fn unregister_exception_handler() {
    let mut count_guard = EXCEPTION_HANDLER_COUNT.lock();
    if *count_guard == 1 {
        unsafe { Debug::RemoveVectoredExceptionHandler(output_debug_string_handler as *mut _) };
    }
    *count_guard -= 1;
}

thread_local! {
    // Non-zero while this thread is inside a driver call this crate knows to
    // be a plausible raw-fault site (DXGI swapchain configure/present -- see
    // `RiskyDriverCallGuard`'s doc comment). A counter rather than a bool so
    // a guarded span that happens to call into another guarded span doesn't
    // have the inner guard's `Drop` clear a flag the outer one still needs;
    // not expected today, but free to make safe.
    static RISKY_DRIVER_CALL_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// RAII marker for "this thread is inside a DXGI/D3D12 driver call known to
/// raise a raw structured exception rather than return an error under some
/// conditions" -- e.g. `STATUS_NOT_IMPLEMENTED` observed live from inside
/// `CreateSwapChainForHwnd`, reached via `Surface::configure`, which took an
/// entire process down with no diagnostic beyond a bare Windows Error
/// Reporting entry.
///
/// This does **not** catch, recover from, or alter that exception in any
/// way -- see `log_if_fault_in_risky_driver_call`'s doc comment for why
/// attempting that is not safe here. It only tells
/// `output_debug_string_handler` (the sole vectored exception handler this
/// process installs) that if *some* exception reaches it right now, on this
/// thread, it is worth logging full diagnostic context before letting the
/// process go down the way it already would have.
pub struct RiskyDriverCallGuard(());

impl RiskyDriverCallGuard {
    pub fn enter() -> Self {
        RISKY_DRIVER_CALL_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self(())
    }
}

impl Drop for RiskyDriverCallGuard {
    fn drop(&mut self) {
        RISKY_DRIVER_CALL_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// The MSVC C++ EH exception code (the bytes spell "msc": `0xE0` + `'m' 's'
/// 'c'`). Rust's own SEH-based panic unwinding on this target reuses it, so
/// a panic caught by `catch_unwind` inside a `RiskyDriverCallGuard`-guarded
/// span raises exactly this code while unwinding through it. Excluded from
/// the logging below so a routine, already-handled panic -- which gets its
/// own log line from whichever `catch_unwind` caught it -- doesn't also
/// produce a misleading "risky driver call faulted" entry for the same,
/// perfectly recovered, event.
const CXX_EH_EXCEPTION_CODE: i32 = 0xE06D7363u32 as i32;

/// The actual decision of whether an exception is worth logging, factored
/// out to a pure function of primitives so it's testable without
/// constructing a Windows `EXCEPTION_RECORD` (a raw FFI struct with no safe
/// way to build a meaningful one outside a real exception context).
fn should_log_risky_fault(depth: u32, exception_code: i32) -> bool {
    depth > 0 && exception_code != CXX_EH_EXCEPTION_CODE
}

/// Best-effort diagnostic logging for an exception this handler is about to
/// let fall through to `EXCEPTION_CONTINUE_SEARCH` -- i.e. eventually to the
/// process's default unhandled-exception behaviour, which on Windows is
/// termination. Never attempts `EXCEPTION_CONTINUE_EXECUTION` or any other
/// form of recovery: resuming after an arbitrary fault inside opaque driver
/// code requires knowing precisely what partial state the faulting
/// instruction left behind, which a generic handler cannot know (see
/// `docs/per-tab-crash-isolation-investigation.md` §3 for the fuller
/// argument and sources). This is a crash *reporter*, not a crash
/// *handler*.
///
/// Only fires while this thread's `RISKY_DRIVER_CALL_DEPTH` is non-zero (see
/// `should_log_risky_fault`), so it cannot misfire for exceptions unrelated
/// to the fault this exists to diagnose -- other threads' panics, debugger
/// events, breakpoints, or anything else that happens to pass through this
/// process-wide handler outside the narrow span `RiskyDriverCallGuard`
/// marks.
fn log_if_fault_in_risky_driver_call(record: &Debug::EXCEPTION_RECORD) {
    if !should_log_risky_fault(RISKY_DRIVER_CALL_DEPTH.with(Cell::get), record.ExceptionCode.0) {
        return;
    }

    let module = module_name_containing(record.ExceptionAddress)
        .unwrap_or_else(|| "<unknown module>".to_string());

    // Matches this file's existing convention, a few lines below, of never
    // letting the act of logging itself bring the process down harder or
    // differently than the fault already would.
    let _ = std::panic::catch_unwind(|| {
        log::error!(
            "risky DXGI/D3D12 driver call raised exception {:#x} at {:?} in {} \
             ({} parameter(s)); not attempting recovery, letting it propagate",
            record.ExceptionCode.0,
            record.ExceptionAddress,
            module,
            record.NumberParameters,
        );
    });
}

/// Resolves a code address to the file name of the module containing it, via
/// `GetModuleHandleExW`'s by-address lookup. Best-effort: any failure (null
/// address, address not inside any loaded module, name unavailable) yields
/// `None` rather than panicking -- this runs inside a vectored exception
/// handler, the last place that should ever introduce a new way to fault.
fn module_name_containing(address: *mut core::ffi::c_void) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    };

    if address.is_null() {
        return None;
    }
    let mut hmodule = Default::default();
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(address as *const u16),
            &mut hmodule,
        )
        .ok()?;
    }
    let mut buf = [0u16; 260];
    let len = unsafe { GetModuleFileNameW(hmodule, &mut buf) };
    if len == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    Some(path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string())
}

const MESSAGE_PREFIXES: &[(&str, log::Level)] = &[
    ("CORRUPTION", log::Level::Error),
    ("ERROR", log::Level::Error),
    ("WARNING", log::Level::Warn),
    ("INFO", log::Level::Info),
    ("MESSAGE", log::Level::Debug),
];

unsafe extern "system" fn output_debug_string_handler(
    exception_info: *mut Debug::EXCEPTION_POINTERS,
) -> i32 {
    // See https://stackoverflow.com/a/41480827
    let record = unsafe { &*(*exception_info).ExceptionRecord };
    if record.NumberParameters != 2 {
        log_if_fault_in_risky_driver_call(record);
        return Debug::EXCEPTION_CONTINUE_SEARCH;
    }
    let message = match record.ExceptionCode {
        Foundation::DBG_PRINTEXCEPTION_C => {
            String::from_utf8_lossy(bytemuck::cast_slice(&record.ExceptionInformation))
        }
        Foundation::DBG_PRINTEXCEPTION_WIDE_C => Cow::Owned(String::from_utf16_lossy(
            bytemuck::cast_slice(&record.ExceptionInformation),
        )),
        _ => {
            log_if_fault_in_risky_driver_call(record);
            return Debug::EXCEPTION_CONTINUE_SEARCH;
        }
    };

    let message = match message.strip_prefix("D3D12 ") {
        Some(msg) => msg
            .trim_end_matches("\n\0")
            .trim_end_matches("[ STATE_CREATION WARNING #0: UNKNOWN]"),
        None => return Debug::EXCEPTION_CONTINUE_SEARCH,
    };

    let (message, level) = match MESSAGE_PREFIXES
        .iter()
        .find(|&&(prefix, _)| message.starts_with(prefix))
    {
        Some(&(prefix, level)) => (&message[prefix.len() + 2..], level),
        None => (message, log::Level::Debug),
    };

    if level == log::Level::Warn && message.contains("#82") {
        // This is are useless spammy warnings (#820, #821):
        // "The application did not pass any clear value to resource creation"
        return Debug::EXCEPTION_CONTINUE_SEARCH;
    }

    if level == log::Level::Warn && message.contains("DRAW_EMPTY_SCISSOR_RECTANGLE") {
        // This is normal, WebGPU allows passing empty scissor rectangles.
        return Debug::EXCEPTION_CONTINUE_SEARCH;
    }

    let _ = std::panic::catch_unwind(|| {
        log::log!(level, "{}", message);
    });

    if cfg!(debug_assertions) && level == log::Level::Error {
        // Set canary and continue
        crate::VALIDATION_CANARY.add(message.to_string());
    }

    Debug::EXCEPTION_CONTINUE_EXECUTION
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Outside any `RiskyDriverCallGuard`-guarded span, nothing should ever
    /// be logged -- this is what keeps the diagnostic scoped to the exact
    /// call sites it was added for, instead of firing for any exception
    /// anywhere in the process.
    #[test]
    fn nothing_logs_outside_a_risky_call() {
        assert!(!should_log_risky_fault(0, 0xC0000005u32 as i32));
        assert!(!should_log_risky_fault(0, CXX_EH_EXCEPTION_CODE));
    }

    /// A genuine fault inside a guarded span must be logged.
    #[test]
    fn a_real_fault_inside_a_risky_call_logs() {
        assert!(should_log_risky_fault(1, 0xC0000005u32 as i32)); // access violation
        assert!(should_log_risky_fault(1, 0xC0000002u32 as i32)); // STATUS_NOT_IMPLEMENTED, crash #3
    }

    /// A caught Rust panic unwinding through a guarded span raises the MSVC
    /// C++ EH code as it unwinds -- it must NOT be logged here, since the
    /// `catch_unwind` that caught it already produces its own log line, and
    /// double-reporting the same, already-recovered event as though it were
    /// a fresh, unhandled fault would be actively misleading.
    #[test]
    fn a_caught_panic_unwinding_through_a_risky_call_does_not_log() {
        assert!(!should_log_risky_fault(1, CXX_EH_EXCEPTION_CODE));
    }

    /// Nested guards (not expected in practice today, but cheap to make
    /// safe) must not have the inner guard's `Drop` clear a depth the outer
    /// guard still needs.
    #[test]
    fn nested_guards_do_not_clear_each_others_depth() {
        RISKY_DRIVER_CALL_DEPTH.with(|d| assert_eq!(d.get(), 0, "test isolation assumption"));

        let outer = RiskyDriverCallGuard::enter();
        assert!(should_log_risky_fault(
            RISKY_DRIVER_CALL_DEPTH.with(Cell::get),
            0xC0000005u32 as i32
        ));
        {
            let inner = RiskyDriverCallGuard::enter();
            assert!(should_log_risky_fault(
                RISKY_DRIVER_CALL_DEPTH.with(Cell::get),
                0xC0000005u32 as i32
            ));
            drop(inner);
        }
        // The inner guard dropped, but the outer one is still live: a fault
        // right now is still inside a risky call as far as the outer caller
        // is concerned.
        assert!(should_log_risky_fault(
            RISKY_DRIVER_CALL_DEPTH.with(Cell::get),
            0xC0000005u32 as i32
        ));
        drop(outer);
        assert!(!should_log_risky_fault(
            RISKY_DRIVER_CALL_DEPTH.with(Cell::get),
            0xC0000005u32 as i32
        ));
    }

    /// Guards against a future accidental edit of the magic constant: it's
    /// the documented MSVC C++ EH exception code (the bytes spell "msc"),
    /// which Rust's own SEH-based unwinding on this target reuses.
    #[test]
    fn cxx_eh_exception_code_is_the_documented_value() {
        assert_eq!(CXX_EH_EXCEPTION_CODE as u32, 0xE06D7363);
    }
}
