use super::*;
use crate::pane::PaneId;
use crate::{Mux, MuxNotification, PANE_OUTPUT_MAX_SYNC_ROUNDS};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// Regression coverage for task #148: `Mux::notify` no longer holds
// `subscribers.write()` while invoking callbacks, and repeated
// `MuxNotification::PaneOutput` notifications for the same pane are
// coalesced while one is already queued for delivery.
//
// Context (see the module docs on `Mux::notify`/`Mux::notify_from_any_thread`
// in `mux/src/lib.rs`): `parse_buffered_data` calls
// `Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane_id))` once
// per parser flush (default coalesce delay
// `mux_output_parser_coalesce_delay_ms` = 3ms), which, prior to this
// change, unconditionally scheduled a `spawn_into_main_thread` task for
// *every* flush. Each of those hops fans out through
// `subscribe_to_pane_updates`'s own `spawn_into_main_thread` callback and
// then `window.notify()` / `Connection::with_window_inner`, i.e. three
// main-thread message-pump iterations of pure scheduling overhead for
// what ultimately amounts to a single `InvalidateRect`. This module pins:
//
//  1. `Mux::notify` invokes every subscriber and does not hold
//     `subscribers.write()` while doing so (a subscriber that tries to
//     take the same lock from inside its callback -- eg. via
//     `Mux::subscribe` or a recursive `Mux::notify` -- must not
//     deadlock).
//  2. Repeated `PaneOutput` notifications for one pane collapse to a
//     single queued delivery while that delivery is in flight, but a
//     fresh notification is accepted again once the queued one has been
//     delivered.

/// Routes `spawn_into_main_thread`/`spawn_into_main_thread_with_low_priority`
/// through a queue that the test drains explicitly, mirroring the
/// production GUI scheduler (`window::spawn::SpawnQueue`), which enqueues
/// work and defers running it to a later turn of the message pump rather
/// than running it synchronously on the calling thread.
struct QueueingScheduler {
    queue: Mutex<Vec<promise::spawn::Runnable>>,
}

impl QueueingScheduler {
    fn drain(&self) {
        loop {
            let next = self.queue.lock().pop();
            match next {
                Some(runnable) => {
                    runnable.run();
                }
                None => break,
            }
        }
    }
}

static SCHEDULER_QUEUE: QueueingScheduler = QueueingScheduler {
    queue: Mutex::new(Vec::new()),
};

fn install_queueing_scheduler() {
    SCHEDULER_QUEUE.queue.lock().clear();
    promise::spawn::set_schedulers(
        Box::new(|runnable| {
            SCHEDULER_QUEUE.queue.lock().push(runnable);
        }),
        Box::new(|runnable| {
            SCHEDULER_QUEUE.queue.lock().push(runnable);
        }),
    );
}

/// `Mux::notify` must invoke every live subscriber, and must not hold
/// `subscribers.write()` while doing so. We prove the "no lock held"
/// half directly: one subscriber, when invoked, probes
/// `Mux::probe_subscribers_try_write()` (a `#[cfg(test)]`-only accessor
/// that mirrors the existing `probe_windows_try_write` pattern used by
/// the `domain_detach` regression test) and records whether the lock was
/// free. If `notify` still held `subscribers.write()` for the duration of
/// the callback loop -- as a naive `retain`-based implementation would --
/// this probe would observe the lock as unavailable.
#[test]
fn notify_invokes_all_subscribers_without_holding_write_lock() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();
    install_queueing_scheduler();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let calls_a = Arc::new(AtomicUsize::new(0));
    let calls_b = Arc::new(AtomicUsize::new(0));
    let lock_was_free_when_b_ran = Arc::new(Mutex::new(None));

    {
        let calls_a = Arc::clone(&calls_a);
        mux.subscribe(move |_n| {
            calls_a.fetch_add(1, Ordering::SeqCst);
            true
        });
    }
    {
        let calls_b = Arc::clone(&calls_b);
        let lock_was_free_when_b_ran = Arc::clone(&lock_was_free_when_b_ran);
        mux.subscribe(move |_n| {
            // This is the crux of the regression check: if `notify` were
            // still holding `subscribers.write()` here, this `try_write`
            // would fail (return `None`), which is exactly the deadlock
            // hazard that a real GUI subscriber could hit (eg. a
            // callback that ends up calling `Mux::subscribe` again, or
            // that re-enters `Mux::notify` synchronously).
            let free = Mux::get().probe_subscribers_try_write();
            *lock_was_free_when_b_ran.lock() = Some(free);
            calls_b.fetch_add(1, Ordering::SeqCst);
            true
        });
    }

    mux.notify(MuxNotification::PaneOutput(1));

    assert_eq!(calls_a.load(Ordering::SeqCst), 1, "subscriber A must run");
    assert_eq!(calls_b.load(Ordering::SeqCst), 1, "subscriber B must run");
    assert_eq!(
        *lock_was_free_when_b_ran.lock(),
        Some(true),
        "Mux::notify must not hold subscribers.write() while invoking callbacks; \
         a subscriber that touches Mux::subscribers from its own callback would \
         otherwise deadlock against this non-reentrant RwLock"
    );

    Mux::shutdown();
}

/// A subscriber that returns `false` asks to be unsubscribed. This must
/// keep working now that `notify` snapshots callbacks under a read lock
/// and reconciles removals in a second pass, rather than doing both in a
/// single `retain` under a write lock.
#[test]
fn notify_still_unsubscribes_callbacks_that_return_false() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();
    install_queueing_scheduler();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let calls = Arc::new(AtomicUsize::new(0));
    {
        let calls = Arc::clone(&calls);
        mux.subscribe(move |_n| {
            calls.fetch_add(1, Ordering::SeqCst);
            // Ask to be unsubscribed after the very first delivery.
            false
        });
    }

    mux.notify(MuxNotification::PaneOutput(1));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // A second notification must *not* reach the now-unsubscribed
    // callback.
    mux.notify(MuxNotification::PaneOutput(1));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "callback that returned false must have been removed from subscribers"
    );

    Mux::shutdown();
}

/// Repeated `PaneOutput` notifications for the same pane, issued while
/// the first one is still queued (not yet delivered), must collapse to a
/// single queued delivery -- this is the amplification `notify_from_any_thread`
/// is meant to remove. Once that delivery runs (draining the scheduler
/// queue), a fresh `PaneOutput` for the same pane must be accepted again
/// rather than being permanently suppressed.
#[test]
fn repeated_pane_output_notifications_coalesce_while_queued() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();
    install_queueing_scheduler();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let pane_id: PaneId = 77;
    let calls = Arc::new(AtomicUsize::new(0));
    {
        let calls = Arc::clone(&calls);
        mux.subscribe(move |n| {
            if matches!(n, MuxNotification::PaneOutput(id) if id == pane_id) {
                calls.fetch_add(1, Ordering::SeqCst);
            }
            true
        });
    }

    // notify_from_any_thread only schedules a spawn_into_main_thread
    // task (the path where coalescing matters) when called from a
    // thread other than the one that constructed the Mux -- exactly
    // parse_buffered_data's situation, where the pty parser runs on its
    // own dedicated thread rather than the GUI/main thread. Route the
    // calls through a real spawned thread so this test exercises that
    // branch instead of the same-thread synchronous fast path.
    //
    // Simulate parse_buffered_data flushing several times in a row
    // before the GUI has had a chance to drain the spawn queue: this is
    // exactly the "coalesce delay elapses repeatedly while the main
    // thread is busy" scenario from task #148.
    thread::spawn(move || {
        Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane_id));
    })
    .join()
    .unwrap();
    assert!(
        mux.pane_output_notify_is_pending(pane_id),
        "first notification should be marked pending"
    );

    thread::spawn(move || {
        for _ in 0..9 {
            Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane_id));
        }
    })
    .join()
    .unwrap();

    // Exactly one task should have reached the scheduler queue for this
    // pane: the other nine calls must have been suppressed because a
    // notification was already pending delivery.
    assert_eq!(
        SCHEDULER_QUEUE.queue.lock().len(),
        1,
        "9 redundant PaneOutput notifications while one was already queued \
         must not enqueue 9 additional spawn_into_main_thread tasks"
    );

    // Deliver the single queued notification.
    SCHEDULER_QUEUE.drain();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the subscriber should observe exactly one PaneOutput delivery for \
         the coalesced burst"
    );
    assert!(
        !mux.pane_output_notify_is_pending(pane_id),
        "the pending marker must be cleared once the notification has been delivered"
    );

    // After delivery, a new flush must be able to schedule again -- the
    // pane must not be permanently stuck as "coalesced away".
    thread::spawn(move || {
        Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane_id));
    })
    .join()
    .unwrap();
    assert_eq!(SCHEDULER_QUEUE.queue.lock().len(), 1);
    SCHEDULER_QUEUE.drain();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "a PaneOutput notification issued after the prior one was delivered \
         must reach subscribers"
    );

    Mux::shutdown();
}

/// Distinct panes must not be coalesced against each other: a burst of
/// `PaneOutput` for pane A must not suppress a `PaneOutput` for pane B.
#[test]
fn pane_output_coalescing_is_scoped_per_pane() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();
    install_queueing_scheduler();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let pane_a: PaneId = 501;
    let pane_b: PaneId = 502;

    thread::spawn(move || {
        Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane_a));
        Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane_a));
        Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane_b));
    })
    .join()
    .unwrap();

    assert_eq!(
        SCHEDULER_QUEUE.queue.lock().len(),
        2,
        "pane A's redundant notification should coalesce, but pane B's \
         independent notification must still be queued"
    );

    SCHEDULER_QUEUE.drain();
    Mux::shutdown();
}

/// When output arrives *during* a PaneOutput delivery (in-flight race),
/// the follow-up delivery must be scheduled. This test has a subscriber
/// that triggers another PaneOutput for the same pane from within the
/// delivery callback, simulating concurrent output. The follow-up must
/// not be dropped. Note that if we're already on the main thread, the
/// follow-up may run synchronously; the key assertion is that it runs at
/// all (the callback count reaches 2), not how it's scheduled.
#[test]
fn pane_output_arriving_during_delivery_schedules_followup() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();
    install_queueing_scheduler();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let pane_id: PaneId = 601;
    let calls = Arc::new(AtomicUsize::new(0));
    let should_recurse = Arc::new(AtomicBool::new(true));

    {
        let calls = Arc::clone(&calls);
        let should_recurse = Arc::clone(&should_recurse);
        mux.subscribe(move |n| {
            if matches!(n, MuxNotification::PaneOutput(id) if id == pane_id) {
                let count = calls.fetch_add(1, Ordering::SeqCst);
                // Only recurse on the first delivery to avoid infinite loop
                if count == 0 && should_recurse.load(Ordering::SeqCst) {
                    // Simulate output arriving concurrently with delivery
                    Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane_id));
                }
            }
            true
        });
    }

    // Schedule first delivery from a spawned thread
    thread::spawn(move || {
        Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane_id));
    })
    .join()
    .unwrap();

    // Drain once: first delivery runs, queues/invokes second due to in-flight notify
    SCHEDULER_QUEUE.drain();

    // Both deliveries should have completed (even if second ran synchronously)
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "both the original delivery and the follow-up must be observed"
    );

    // Queue should be empty (no stuck pending state)
    assert_eq!(
        SCHEDULER_QUEUE.queue.lock().len(),
        0,
        "pending state should not get stuck after follow-up completes"
    );

    Mux::shutdown();
}

/// Sustained, continuous output (not just one in-flight repeat) must not
/// turn `Mux::notify`'s reschedule path into unbounded recursion. A
/// subscriber re-triggers `PaneOutput` for the same pane on every single
/// delivery, simulating a pane that never goes quiet, for many more rounds
/// than `PANE_OUTPUT_MAX_SYNC_ROUNDS`. Every round must still be delivered
/// (no output silently dropped), but delivery must periodically yield back
/// to the scheduler queue -- observed here as more than one queued task
/// running to completion -- rather than resolving in a single recursive
/// call. Before the fix, this looped through `dispatch_notification` ->
/// `notify` recursively without bound, which would eventually blow the
/// stack on real sustained output (eg. `yes` piped into a pane); this test
/// instead proves the round count is bounded by asserting more than one
/// hop through the scheduler queue occurred.
#[test]
fn pane_output_sustained_output_yields_to_event_loop() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();
    install_queueing_scheduler();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let pane_id: PaneId = 909;
    // Enough rounds to force several yields back to the event loop.
    let total_rounds = PANE_OUTPUT_MAX_SYNC_ROUNDS * 3 + 5;
    let calls = Arc::new(AtomicUsize::new(0));

    {
        let calls = Arc::clone(&calls);
        mux.subscribe(move |n| {
            if matches!(n, MuxNotification::PaneOutput(id) if id == pane_id) {
                let count = calls.fetch_add(1, Ordering::SeqCst) + 1;
                if count < total_rounds {
                    // Simulate more output having arrived while this
                    // delivery was in flight.
                    Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane_id));
                }
            }
            true
        });
    }

    thread::spawn(move || {
        Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane_id));
    })
    .join()
    .unwrap();

    // Drain the scheduler queue to completion, counting how many separate
    // tasks actually ran. Bounded by a generous safety cap so a real
    // regression (the old unbounded-recursion behavior, or a bug that
    // drops the pending state) fails the test instead of hanging it.
    let mut hops = 0usize;
    for _ in 0..(total_rounds + 16) {
        let next = SCHEDULER_QUEUE.queue.lock().pop();
        match next {
            Some(runnable) => {
                hops += 1;
                runnable.run();
            }
            None => break,
        }
    }

    assert_eq!(
        calls.load(Ordering::SeqCst),
        total_rounds,
        "every round of sustained output must still be delivered"
    );
    assert!(
        hops > 1,
        "sustained output spanning {} rounds (> PANE_OUTPUT_MAX_SYNC_ROUNDS = {}) must yield \
         back to the event loop more than once, not resolve as a single unbounded recursive \
         call; observed {} hop(s)",
        total_rounds,
        PANE_OUTPUT_MAX_SYNC_ROUNDS,
        hops
    );
    assert!(
        hops < total_rounds,
        "yielding must still batch multiple rounds per hop, not fall back to one event-loop \
         round-trip per output event; observed {} hops for {} rounds",
        hops,
        total_rounds
    );
    assert_eq!(
        SCHEDULER_QUEUE.queue.lock().len(),
        0,
        "no stray pending task should remain after all rounds have been delivered"
    );

    Mux::shutdown();
}
