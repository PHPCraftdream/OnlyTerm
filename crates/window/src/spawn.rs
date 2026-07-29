#[cfg(windows)]
use crate::os::windows::event::EventHandle;
#[cfg(target_os = "macos")]
use core_foundation::runloop::*;
use promise::spawn::{Runnable, SpawnFunc};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;
#[cfg(all(unix, not(target_os = "macos")))]
use {
    filedescriptor::{FileDescriptor, Pipe},
    std::os::unix::io::AsRawFd,
};

lazy_static::lazy_static! {
    pub(crate) static ref SPAWN_QUEUE: Arc<SpawnQueue> = Arc::new(SpawnQueue::new().expect("failed to create SpawnQueue"));
}

struct InstrumentedSpawnFunc {
    func: SpawnFunc,
    at: Instant,
}

pub(crate) struct SpawnQueue {
    spawned_funcs: Mutex<VecDeque<InstrumentedSpawnFunc>>,
    spawned_funcs_low_pri: Mutex<VecDeque<InstrumentedSpawnFunc>>,

    #[cfg(windows)]
    pub event_handle: EventHandle,

    #[cfg(all(unix, not(target_os = "macos")))]
    write: Mutex<FileDescriptor>,
    #[cfg(all(unix, not(target_os = "macos")))]
    read: Mutex<FileDescriptor>,
}

fn schedule_with_pri(runnable: Runnable, high_pri: bool) {
    SPAWN_QUEUE.spawn_impl(
        Box::new(move || {
            runnable.run();
        }),
        high_pri,
    );
}

impl SpawnQueue {
    pub fn new() -> anyhow::Result<Self> {
        Self::new_impl()
    }

    pub fn register_promise_schedulers(&self) {
        promise::spawn::set_schedulers(
            Box::new(|runnable| {
                schedule_with_pri(runnable, true);
            }),
            Box::new(|runnable| {
                schedule_with_pri(runnable, false);
            }),
        );
    }

    pub fn run(&self) -> bool {
        self.run_impl()
    }

    // This needs to be a separate function from the loop in `run`
    // in order for the lock to be released before we call the
    // returned function
    fn pop_func(&self) -> Option<SpawnFunc> {
        if let Some(func) = self.spawned_funcs.lock().unwrap().pop_front() {
            metrics::histogram!("executor.spawn_delay").record(func.at.elapsed());
            Some(func.func)
        } else if let Some(func) = self.spawned_funcs_low_pri.lock().unwrap().pop_front() {
            metrics::histogram!("executor.spawn_delay.low_pri").record(func.at.elapsed());
            Some(func.func)
        } else {
            None
        }
    }

    fn queue_func(&self, f: SpawnFunc, high_pri: bool) {
        let f = InstrumentedSpawnFunc {
            func: f,
            at: Instant::now(),
        };
        if high_pri {
            self.spawned_funcs.lock().unwrap()
        } else {
            self.spawned_funcs_low_pri.lock().unwrap()
        }
        .push_back(f);
    }

    fn has_any_queued(&self) -> bool {
        !self.spawned_funcs.lock().unwrap().is_empty()
            || !self.spawned_funcs_low_pri.lock().unwrap().is_empty()
    }
}

#[cfg(windows)]
impl SpawnQueue {
    fn new_impl() -> anyhow::Result<Self> {
        let spawned_funcs = Mutex::new(VecDeque::new());
        let spawned_funcs_low_pri = Mutex::new(VecDeque::new());
        let event_handle = EventHandle::new_manual_reset().expect("EventHandle creation failed");
        Ok(Self {
            spawned_funcs,
            spawned_funcs_low_pri,
            event_handle,
        })
    }

    fn spawn_impl(&self, f: SpawnFunc, high_pri: bool) {
        self.queue_func(f, high_pri);
        self.event_handle.set_event();
    }

    fn run_impl(&self) -> bool {
        self.event_handle.reset_event();
        // On Windows we only ever process one item at a time, so that
        // we return promptly to the caller's message loop and let it
        // service `PeekMessageW`/`DispatchMessageW` in between each
        // spawned task. Mirrors the analogous fixes already applied to
        // the X11 (4e1cfe01a) and macOS (b3032f8a5) backends: without
        // this, a sustained stream of pty output (e.g. an AI coding
        // tool printing thousands of lines quickly) keeps this queue
        // perpetually non-empty, and draining it in a tight `while`
        // loop here would starve the OS message pump indefinitely,
        // freezing input/paint handling for the whole window.
        if let Some(func) = self.pop_func() {
            func();
        }
        let more = self.has_any_queued();
        if more {
            // Keep the event signalled so that the caller's message
            // loop comes right back here instead of blocking in
            // `wait_message`, while still giving it a chance to pump
            // real Windows messages on each iteration.
            self.event_handle.set_event();
        }
        more
    }
}

#[cfg(all(test, windows))]
mod test {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Regression test for UP-42: a sustained stream of queued work
    /// (analogous to a burst of `PaneOutput` notifications produced by
    /// a pty flooding stdout) must be drained one item per `run()` call
    /// on Windows, exactly like the X11 and macOS backends already do.
    /// If `run()` ever goes back to draining the whole queue in a tight
    /// loop, the caller's `PeekMessageW`/`DispatchMessageW` pump never
    /// gets a chance to run in between, which is precisely the "GUI
    /// freezes while a fast/large stream of output arrives" symptom.
    #[test]
    fn run_drains_only_one_item_per_call() {
        let queue = SpawnQueue::new().expect("failed to create SpawnQueue");

        let counter = Arc::new(AtomicUsize::new(0));
        const N: usize = 5000;
        for _ in 0..N {
            let counter = Arc::clone(&counter);
            queue.spawn_impl(
                Box::new(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                }),
                true,
            );
        }

        // Each call to run() must execute at most one queued item, so
        // that a message-loop caller regains control between items.
        let mut iterations = 0;
        let mut more = true;
        while more {
            let before = counter.load(Ordering::SeqCst);
            more = queue.run_impl();
            let after = counter.load(Ordering::SeqCst);
            assert!(
                after - before <= 1,
                "run() executed {} items in a single call; it must drain at most one \
                 so that the Windows message pump is not starved under sustained load",
                after - before
            );
            iterations += 1;
            // Guard against an infinite loop turning this test into a hang
            // if `has_any_queued`/`pop_func` ever disagree about queue state.
            assert!(iterations <= N + 1, "run() did not converge after draining the queue");
        }

        assert_eq!(counter.load(Ordering::SeqCst), N);
        // The queue is empty, so a further call must report nothing left to do.
        assert!(!queue.has_any_queued());
    }

    /// When work remains after processing one item, run() must leave the
    /// event signalled so that the caller's `wait_message()` (which blocks
    /// on this handle) returns immediately instead of sleeping while a
    /// backlog of pty-output-driven tasks is still pending.
    #[test]
    fn run_resignals_event_when_work_remains() {
        let queue = SpawnQueue::new().expect("failed to create SpawnQueue");

        queue.spawn_impl(Box::new(|| {}), true);
        queue.spawn_impl(Box::new(|| {}), true);

        // First call pops one item; one remains, so the event must stay set.
        let more = queue.run_impl();
        assert!(more, "run() should report that work remains");
        assert!(
            queue.event_handle.is_signalled(),
            "event must remain signalled while items are still queued"
        );

        // Second call drains the last item; nothing remains.
        let more = queue.run_impl();
        assert!(!more, "run() should report the queue is now empty");
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
impl SpawnQueue {
    fn new_impl() -> anyhow::Result<Self> {
        // On linux we have a slightly sloppy wakeup mechanism;
        // we have a non-blocking pipe that we can use to get
        // woken up after some number of enqueues.  We don't
        // guarantee a 1:1 enqueue to wakeup with this mechanism
        // but in practical terms it does guarantee a wakeup
        // if the main thread is asleep and we enqueue some
        // number of items.
        // We can't affort to use a blocking pipe for the wakeup
        // because the write needs to hold a mutex and that
        // can block reads as well as other writers.
        let mut pipe = Pipe::new()?;
        pipe.write.set_non_blocking(true)?;
        pipe.read.set_non_blocking(true)?;
        Ok(Self {
            spawned_funcs: Mutex::new(VecDeque::new()),
            spawned_funcs_low_pri: Mutex::new(VecDeque::new()),
            write: Mutex::new(pipe.write),
            read: Mutex::new(pipe.read),
        })
    }

    fn spawn_impl(&self, f: SpawnFunc, high_pri: bool) {
        use std::io::Write;

        self.queue_func(f, high_pri);
        while let Err(err) = self.write.lock().unwrap().write(b"x") {
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            log::warn!("Failed to signal spawn queue pipe: {:#}", err);
            break;
        }
    }

    fn run_impl(&self) -> bool {
        // On linux we only ever process one at at time, so that
        // we can return to the main loop and process messages
        // from the X server
        if let Some(func) = self.pop_func() {
            func();
        }

        // try to drain the pipe.
        // We do this regardless of whether we popped an item
        // so that we avoid being in a perpetually signalled state.
        // It is ok if we completely drain the pipe because the
        // main loop uses the return value to set the sleep
        // interval and will unconditionally call us on each
        // iteration.
        let mut byte = [0u8; 64];
        use std::io::Read;
        self.read.lock().unwrap().read(&mut byte).ok();

        self.has_any_queued()
    }

    pub(crate) fn raw_fd(&self) -> std::os::unix::io::RawFd {
        self.read.lock().unwrap().as_raw_fd()
    }
}

#[cfg(target_os = "macos")]
impl SpawnQueue {
    fn new_impl() -> anyhow::Result<Self> {
        let spawned_funcs = Mutex::new(VecDeque::new());
        let spawned_funcs_low_pri = Mutex::new(VecDeque::new());

        // SAFETY: null allocator/context args use defaults; `SpawnQueue::trigger`
        // is a valid `CFRunLoopObserverCallBack` and the activity/mode/order args
        // are valid constants. The observer is added to the main run loop below.
        let observer = unsafe {
            CFRunLoopObserverCreate(
                std::ptr::null(),
                kCFRunLoopAllActivities,
                1,
                0,
                SpawnQueue::trigger,
                std::ptr::null_mut(),
            )
        };
        // SAFETY: `CFRunLoopGetMain()` returns the main run loop and `observer`
        // was just created; `kCFRunLoopCommonModes` is a valid mode constant.
        unsafe {
            CFRunLoopAddObserver(CFRunLoopGetMain(), observer, kCFRunLoopCommonModes);
        }

        Ok(Self {
            spawned_funcs,
            spawned_funcs_low_pri,
        })
    }

    extern "C" fn trigger(
        _observer: *mut __CFRunLoopObserver,
        _: CFRunLoopActivity,
        _: *mut std::ffi::c_void,
    ) {
        if SPAWN_QUEUE.run() {
            Self::queue_wakeup();
        }
    }

    fn queue_wakeup() {
        // SAFETY: `CFRunLoopGetMain()` returns the main run loop; waking it is
        // always safe.
        unsafe {
            CFRunLoopWakeUp(CFRunLoopGetMain());
        }
    }

    fn spawn_impl(&self, f: SpawnFunc, high_pri: bool) {
        self.queue_func(f, high_pri);
        Self::queue_wakeup();
    }

    fn run_impl(&self) -> bool {
        if let Some(func) = self.pop_func() {
            func();
        }
        self.has_any_queued()
    }
}
