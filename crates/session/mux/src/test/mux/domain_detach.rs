//! Regression coverage for `Mux::domain_was_detached`.
//!
//! A sibling fork of onlyterm (wakamex/wakterm) had a version of this
//! function that held `windows.write()` for the duration of the call to
//! `tab.kill_panes_in_domain(domain)`, and `kill_panes_in_domain` could
//! transitively try to lock `windows` again (e.g. via a notification
//! callback), which deadlocks a non-reentrant lock such as
//! `parking_lot::RwLock` (this is also the lock type backing
//! `Mux::windows` here).
//!
//! In this codebase, `remove_pane_if` (which backs `kill_panes_in_domain`)
//! never touches `Mux::windows` synchronously: the only `Mux::get()` use
//! is deferred via `onlyterm_promise::spawn::spawn_into_main_thread(..).detach()`.
//! In the real GUI, that scheduler (`window::spawn::SpawnQueue`,
//! registered via `register_promise_schedulers`) only *enqueues* the
//! runnable and wakes the event loop; the runnable itself is polled to
//! completion later, on a separate turn of the loop, strictly after
//! `domain_was_detached` (and the `windows` write guard it holds) has
//! returned. This test's scheduler mirrors that: it queues runnables into
//! a `Vec` instead of running them inline, so the assertions below
//! reflect the same "deferred, not reentrant" contract the production
//! scheduler provides. (An earlier version of this test used a
//! run-inline scheduler and deadlocked afterward, in
//! `Mux::remove_pane` -> `prune_dead_windows` -> `windows.try_write()`
//! -- not because production is unsafe, but because running the
//! "deferred" closure synchronously, inside `domain_was_detached`'s own
//! `windows.write()` critical section, reintroduces exactly the wakterm
//! hazard as a test-harness artifact. Draining the queue only after
//! `domain_was_detached` returns is what makes the test represent
//! production's actual threading model.)
//!
//! This test pins the "pane teardown never re-enters `windows`" property
//! directly: the removed ("victim") pane's `kill()` -- which only runs
//! from the deferred closure, after `domain_was_detached` has returned --
//! probes `Mux::windows` with `try_write` and records whether it was
//! free. If a future change ever made that teardown (or a notification
//! callback reachable from it) run synchronously from within
//! `domain_was_detached`'s `windows.write()` critical section instead
//! -- reintroducing the wakterm pattern -- this probe would observe the
//! lock as unavailable and fail the assertion below.
use super::*;
use crate::domain::{Domain, DomainState, SplitSource};
use crate::pane::{CloseReason, Pane, PaneId};
use crate::tab::{SplitDirection, SplitRequest, SplitSize};
use crate::window::Window;
use crate::{DomainId, Mux, MuxNotification};
use async_trait::async_trait;
use onlyterm_config::keyassignment::SpawnTabDomain;
use onlyterm_term::color::ColorPalette;
use onlyterm_term::{KeyCode, KeyModifiers, MouseEvent, StableRowIndex, TerminalSize};
use parking_lot::MappedMutexGuard;
use portable_pty::CommandBuilder;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Queues `Runnable`s instead of running them inline, so that
/// `spawn_into_main_thread` behaves like the production GUI scheduler:
/// deferred to a later, explicit drain rather than run synchronously on
/// the calling thread. See the module doc for why this distinction
/// matters for this particular test.
struct QueueingScheduler {
    queue: Mutex<Vec<onlyterm_promise::spawn::Runnable>>,
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

static NEXT_PANE_ID: AtomicUsize = AtomicUsize::new(9000);

/// A pane double whose `kill()` probes whether `Mux::windows` is
/// currently free via `try_write`, and records the outcome.
///
/// `kill()` is the right hook to probe: for the removed ("victim") pane,
/// it only runs from inside the *deferred* `spawn_into_main_thread`
/// closure (see `remove_pane_if` in `mux/src/tab.rs`), i.e. strictly
/// after `domain_was_detached` has returned and dropped its `windows`
/// write guard. If a future change ever called pane teardown (or a
/// notification callback that reaches `Mux::windows`) synchronously
/// from within `domain_was_detached`'s critical section instead, this
/// probe would observe the lock as held.
///
/// (`resize()` is deliberately *not* instrumented here: siblings are
/// resized synchronously while `domain_was_detached` still holds
/// `windows.write()`, and that is expected, harmless behavior as long
/// as `resize()` itself never tries to acquire `windows` -- which is
/// exactly what this probe would catch if it did.)
struct ProbePane {
    id: PaneId,
    domain_id: DomainId,
    windows_lock_was_free_on_kill: Mutex<Option<bool>>,
}

impl ProbePane {
    fn new(domain_id: DomainId) -> Arc<Self> {
        Arc::new(Self {
            id: NEXT_PANE_ID.fetch_add(1, Ordering::Relaxed) as PaneId,
            domain_id,
            windows_lock_was_free_on_kill: Mutex::new(None),
        })
    }
}

impl Pane for ProbePane {
    fn pane_id(&self) -> PaneId {
        self.id
    }
    fn is_dead(&self) -> bool {
        false
    }
    fn get_cursor_position(&self) -> StableCursorPosition {
        unreachable!()
    }
    fn get_current_seqno(&self) -> SequenceNo {
        unreachable!()
    }
    fn get_changed_since(
        &self,
        _lines: Range<StableRowIndex>,
        _seqno: SequenceNo,
    ) -> RangeSet<StableRowIndex> {
        unreachable!()
    }
    fn get_lines(&self, _lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
        unreachable!()
    }
    fn with_lines_mut(&self, _lines: Range<StableRowIndex>, _with_lines: &mut dyn WithPaneLines) {
        unreachable!()
    }
    fn for_each_logical_line_in_stable_range_mut(
        &self,
        _lines: Range<StableRowIndex>,
        _for_line: &mut dyn ForEachPaneLogicalLine,
    ) {
        unreachable!()
    }
    fn get_logical_lines(&self, _lines: Range<StableRowIndex>) -> Vec<LogicalLine> {
        unreachable!()
    }
    fn get_dimensions(&self) -> RenderableDimensions {
        unreachable!()
    }
    fn get_title(&self) -> String {
        unreachable!()
    }
    fn send_paste(&self, _text: &str) -> anyhow::Result<()> {
        unreachable!()
    }
    fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>> {
        Ok(None)
    }
    fn writer(&self) -> MappedMutexGuard<'_, dyn std::io::Write> {
        unreachable!()
    }
    fn resize(&self, _size: TerminalSize) -> anyhow::Result<()> {
        Ok(())
    }
    fn key_down(&self, _key: KeyCode, _mods: KeyModifiers) -> anyhow::Result<()> {
        unreachable!()
    }
    fn key_up(&self, _key: KeyCode, _mods: KeyModifiers) -> anyhow::Result<()> {
        unreachable!()
    }
    fn mouse_event(&self, _event: MouseEvent) -> anyhow::Result<()> {
        unreachable!()
    }
    fn palette(&self) -> ColorPalette {
        unreachable!()
    }
    fn domain_id(&self) -> DomainId {
        self.domain_id
    }
    fn get_current_working_dir(&self, _policy: CachePolicy) -> Option<Url> {
        None
    }
    fn is_mouse_grabbed(&self) -> bool {
        false
    }
    fn is_alt_screen_active(&self) -> bool {
        false
    }
    fn kill(&self) {
        // For the victim pane, this runs from inside the deferred
        // `spawn_into_main_thread` closure that `remove_pane_if` queues
        // -- i.e. only after `domain_was_detached` has returned and its
        // `windows` write guard has already been dropped. This is the
        // most direct analogue of the wakterm callback-reentrancy path.
        let free = Mux::get().probe_windows_try_write();
        *self.windows_lock_was_free_on_kill.lock() = Some(free);
    }
    fn can_close_without_prompting(&self, _reason: CloseReason) -> bool {
        true
    }
}

#[test]
fn domain_was_detached_does_not_hold_windows_lock_during_pane_teardown() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();

    // Route `spawn_into_main_thread` through a queue that we drain
    // explicitly, rather than a scheduler that runs runnables inline.
    // This mirrors the production GUI scheduler (`window::spawn`),
    // which enqueues and defers to a later loop turn instead of running
    // synchronously on the calling thread -- see the module doc for why
    // an inline scheduler here would produce a false-positive hang.
    SCHEDULER_QUEUE.queue.lock().clear();
    onlyterm_promise::spawn::set_schedulers(
        Box::new(|runnable| {
            SCHEDULER_QUEUE.queue.lock().push(runnable);
        }),
        Box::new(|runnable| {
            SCHEDULER_QUEUE.queue.lock().push(runnable);
        }),
    );

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let domain_id: DomainId = 4242;
    let size = TerminalSize::default();

    let victim = ProbePane::new(domain_id);
    let survivor = ProbePane::new(domain_id + 1);

    let tab = Arc::new(crate::tab::Tab::new(&size));
    tab.assign_pane(&(Arc::clone(&victim) as Arc<dyn Pane>));
    tab.split_and_insert(
        0,
        SplitRequest {
            direction: SplitDirection::Horizontal,
            target_is_second: true,
            top_level: false,
            size: SplitSize::Percent(50),
        },
        Arc::clone(&survivor) as Arc<dyn Pane>,
    )
    .expect("split should succeed");

    mux.add_pane(&(Arc::clone(&victim) as Arc<dyn Pane>))
        .unwrap();
    mux.add_pane(&(Arc::clone(&survivor) as Arc<dyn Pane>))
        .unwrap();

    let mut window = Window::new(None, None);
    let window_id = window.window_id();
    window.push(&tab);
    mux.insert_window_for_test(window_id, window);

    // Sanity: both panes are visible in the tab prior to detaching the
    // victim's domain.
    assert!(tab.contains_pane(victim.pane_id()));
    assert!(tab.contains_pane(survivor.pane_id()));

    // This must not hang and must not observe `windows` as locked from
    // within the synchronous teardown path.
    mux.domain_was_detached(domain_id);

    // Only now -- after `domain_was_detached` has fully returned and its
    // `windows` write guard has been dropped -- do we drain the deferred
    // `spawn_into_main_thread` work, exactly as the GUI's message loop
    // would on its next turn.
    SCHEDULER_QUEUE.drain();

    assert!(
        !tab.contains_pane(victim.pane_id()),
        "the detached domain's pane should have been removed from the tab"
    );
    assert!(
        tab.contains_pane(survivor.pane_id()),
        "panes on other domains must be left alone"
    );

    // The victim's `kill()` must have run (via the drained queue) and
    // must have observed `Mux::windows` as free -- proving that nothing
    // in this codebase's `domain_was_detached` -> `kill_panes_in_domain`
    // -> deferred pane teardown path re-enters `windows` while it is
    // still held, which is exactly the hazard that produced a real
    // deadlock in the wakterm fork.
    let victim_probe = *victim.windows_lock_was_free_on_kill.lock();
    assert_eq!(
        victim_probe,
        Some(true),
        "victim pane's kill() observed Mux::windows as write-locked (try_write failed); \
         this is exactly the wakterm lock-ordering hazard: pane teardown driven by \
         domain_was_detached must never run while `windows` is still held",
    );
    assert_eq!(
        *survivor.windows_lock_was_free_on_kill.lock(),
        None,
        "the survivor pane is on a different domain and must not be killed"
    );

    Mux::shutdown();
}

struct RoutingDomain {
    id: DomainId,
    name: &'static str,
    spawnable: bool,
}

#[async_trait(?Send)]
impl Domain for RoutingDomain {
    fn domain_id(&self) -> DomainId {
        self.id
    }

    fn domain_name(&self) -> &str {
        self.name
    }

    async fn spawn_pane(
        &self,
        _size: TerminalSize,
        _command: Option<CommandBuilder>,
        _command_dir: Option<String>,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        anyhow::bail!("routing test must not spawn a pane")
    }

    async fn split_pane(
        &self,
        _source: SplitSource,
        _tab: crate::tab::TabId,
        _pane_id: PaneId,
        _request: SplitRequest,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        anyhow::bail!("selected domain {}", self.name)
    }

    async fn attach(&self, _window_id: Option<crate::window::WindowId>) -> anyhow::Result<()> {
        anyhow::bail!("routing test must not attach a domain")
    }

    fn state(&self) -> DomainState {
        DomainState::Attached
    }

    fn detachable(&self) -> bool {
        false
    }

    fn detach(&self) -> anyhow::Result<()> {
        anyhow::bail!("routing test must not detach a domain")
    }

    fn spawnable(&self) -> bool {
        self.spawnable
    }
}

#[test]
fn split_preserves_non_spawnable_pane_domain_while_new_tab_falls_back() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    for (id, name, spawnable) in [
        (1, "default", true),
        (2, "isolated", false),
        (3, "elevated", false),
    ] {
        let domain: Arc<dyn Domain> = Arc::new(RoutingDomain {
            id,
            name,
            spawnable,
        });
        mux.add_domain(&domain);
    }

    for (domain_id, expected_name) in [(2, "isolated"), (3, "elevated")] {
        let pane = ProbePane::new(domain_id);
        let pane_ref: Arc<dyn Pane> = pane.clone();
        let tab = Arc::new(crate::tab::Tab::new(&TerminalSize::default()));
        tab.assign_pane(&pane_ref);
        mux.add_tab_no_panes(&tab);
        mux.add_pane(&pane_ref).unwrap();

        let mut window = Window::new(None, None);
        let window_id = window.window_id();
        window.push(&tab);
        mux.insert_window_for_test(window_id, window);

        let new_tab_domain = mux
            .resolve_spawn_tab_domain(Some(pane.pane_id()), &SpawnTabDomain::CurrentPaneDomain)
            .unwrap();
        assert_eq!(new_tab_domain.domain_id(), 1);

        let error = onlyterm_promise::spawn::block_on(mux.split_pane(
            pane.pane_id(),
            SplitRequest::default(),
            SplitSource::Spawn {
                command: None,
                command_dir: Some("routing-test".to_string()),
            },
            SpawnTabDomain::CurrentPaneDomain,
        ))
        .err()
        .expect("routing domain deliberately rejects the split");
        assert_eq!(
            error.to_string(),
            format!("selected domain {}", expected_name)
        );
    }

    Mux::shutdown();
}

struct LosingTargetDomain {
    tab: Arc<crate::tab::Tab>,
    target: PaneId,
    spawned: Arc<ProbePane>,
    register_spawned: bool,
}

#[async_trait(?Send)]
impl Domain for LosingTargetDomain {
    fn domain_id(&self) -> DomainId {
        self.spawned.domain_id
    }

    fn domain_name(&self) -> &str {
        "losing-target"
    }

    async fn spawn_pane(
        &self,
        _size: TerminalSize,
        _command: Option<CommandBuilder>,
        _command_dir: Option<String>,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        self.tab.remove_pane(self.target);
        let pane: Arc<dyn Pane> = self.spawned.clone();
        if self.register_spawned {
            Mux::get().add_pane(&pane)?;
        }
        Ok(pane)
    }

    async fn attach(&self, _window_id: Option<crate::window::WindowId>) -> anyhow::Result<()> {
        Ok(())
    }

    fn state(&self) -> DomainState {
        DomainState::Attached
    }

    fn detachable(&self) -> bool {
        false
    }

    fn detach(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn assert_failed_split_discards_spawned_pane(register_spawned: bool) {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);
    let target = ProbePane::new(1);
    let spawned = ProbePane::new(1);
    let tab = Arc::new(crate::tab::Tab::new(&TerminalSize::default()));
    tab.assign_pane(&(target.clone() as Arc<dyn Pane>));
    mux.add_tab_no_panes(&tab);
    mux.add_pane(&(target.clone() as Arc<dyn Pane>)).unwrap();
    let mut window = Window::new(None, None);
    let window_id = window.window_id();
    window.push(&tab);
    mux.insert_window_for_test(window_id, window);

    let domain = LosingTargetDomain {
        tab: Arc::clone(&tab),
        target: target.pane_id(),
        spawned: Arc::clone(&spawned),
        register_spawned,
    };
    let result = onlyterm_promise::spawn::block_on(domain.split_pane(
        SplitSource::Spawn {
            command: None,
            command_dir: None,
        },
        tab.tab_id(),
        target.pane_id(),
        SplitRequest::default(),
    ));
    assert!(result.is_err());
    assert_eq!(*spawned.windows_lock_was_free_on_kill.lock(), Some(true));
    assert!(mux.get_pane(spawned.pane_id()).is_none());
    Mux::shutdown();
}

#[test]
fn failed_split_discards_registered_spawned_pane() {
    assert_failed_split_discards_spawned_pane(true);
}

#[test]
fn failed_split_kills_unregistered_spawned_pane() {
    assert_failed_split_discards_spawned_pane(false);
}

#[test]
fn split_rejects_dimensions_without_room_before_spawning() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();

    for (cols, rows, request) in [
        (2, 24, SplitRequest::default()),
        (
            80,
            2,
            SplitRequest {
                direction: SplitDirection::Vertical,
                ..Default::default()
            },
        ),
        (
            80,
            24,
            SplitRequest {
                size: SplitSize::Cells(usize::MAX),
                ..Default::default()
            },
        ),
    ] {
        let mux = Arc::new(Mux::new(None));
        Mux::set_mux(&mux);
        let size = TerminalSize {
            cols,
            rows,
            ..Default::default()
        };
        let target = ProbePane::new(1);
        let spawned = ProbePane::new(1);
        let tab = Arc::new(crate::tab::Tab::new(&size));
        tab.assign_pane(&(target.clone() as Arc<dyn Pane>));
        mux.add_tab_no_panes(&tab);
        mux.add_pane(&(target.clone() as Arc<dyn Pane>)).unwrap();
        let mut window = Window::new(None, None);
        let window_id = window.window_id();
        window.push(&tab);
        mux.insert_window_for_test(window_id, window);

        let domain = LosingTargetDomain {
            tab: Arc::clone(&tab),
            target: target.pane_id(),
            spawned: Arc::clone(&spawned),
            register_spawned: false,
        };
        let result = onlyterm_promise::spawn::block_on(domain.split_pane(
            SplitSource::Spawn {
                command: None,
                command_dir: None,
            },
            tab.tab_id(),
            target.pane_id(),
            request,
        ));
        assert!(result.is_err());
        assert!(tab.contains_pane(target.pane_id()));
        assert_eq!(*spawned.windows_lock_was_free_on_kill.lock(), None);
        Mux::shutdown();
    }
}

#[test]
fn empty_notification_waits_for_the_last_pane_in_a_split_tab() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();
    SCHEDULER_QUEUE.queue.lock().clear();
    onlyterm_promise::spawn::set_schedulers(
        Box::new(|runnable| SCHEDULER_QUEUE.queue.lock().push(runnable)),
        Box::new(|runnable| SCHEDULER_QUEUE.queue.lock().push(runnable)),
    );

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);
    let size = TerminalSize::default();
    let first = ProbePane::new(1);
    let second = ProbePane::new(1);
    let tab = Arc::new(crate::tab::Tab::new(&size));
    tab.assign_pane(&(first.clone() as Arc<dyn Pane>));
    tab.split_and_insert(0, SplitRequest::default(), second.clone() as Arc<dyn Pane>)
        .unwrap();
    mux.add_tab_no_panes(&tab);
    mux.add_pane(&(first.clone() as Arc<dyn Pane>)).unwrap();
    mux.add_pane(&(second.clone() as Arc<dyn Pane>)).unwrap();
    let mut window = Window::new(None, None);
    let window_id = window.window_id();
    window.push(&tab);
    mux.insert_window_for_test(window_id, window);

    let empty_count = Arc::new(AtomicUsize::new(0));
    let notifications = Arc::clone(&empty_count);
    mux.subscribe(move |notification| {
        if matches!(notification, MuxNotification::Empty) {
            notifications.fetch_add(1, Ordering::SeqCst);
        }
        true
    });

    mux.remove_pane(first.pane_id());
    SCHEDULER_QUEUE.drain();
    assert_eq!(empty_count.load(Ordering::SeqCst), 0);
    assert!(mux.get_pane(second.pane_id()).is_some());

    mux.remove_pane(second.pane_id());
    SCHEDULER_QUEUE.drain();
    assert!(empty_count.load(Ordering::SeqCst) > 0);
    Mux::shutdown();
}

struct ResizeDuringSpawnDomain {
    tab: Arc<crate::tab::Tab>,
    size: TerminalSize,
    spawned: Arc<ProbePane>,
}

#[async_trait(?Send)]
impl Domain for ResizeDuringSpawnDomain {
    fn domain_id(&self) -> DomainId {
        self.spawned.domain_id
    }

    fn domain_name(&self) -> &str {
        "resize-during-spawn"
    }

    async fn spawn_pane(
        &self,
        _size: TerminalSize,
        _command: Option<CommandBuilder>,
        _command_dir: Option<String>,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        self.tab.resize(self.size);
        let pane: Arc<dyn Pane> = self.spawned.clone();
        Mux::get().add_pane(&pane)?;
        Ok(pane)
    }

    async fn attach(&self, _window_id: Option<crate::window::WindowId>) -> anyhow::Result<()> {
        Ok(())
    }

    fn state(&self) -> DomainState {
        DomainState::Attached
    }

    fn detachable(&self) -> bool {
        false
    }

    fn detach(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[test]
fn resize_during_spawn_cleans_up_a_split_that_no_longer_fits() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();
    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);
    let target = ProbePane::new(1);
    let spawned = ProbePane::new(1);
    let tab = Arc::new(crate::tab::Tab::new(&TerminalSize::default()));
    tab.assign_pane(&(target.clone() as Arc<dyn Pane>));
    mux.add_tab_no_panes(&tab);
    mux.add_pane(&(target.clone() as Arc<dyn Pane>)).unwrap();
    let mut window = Window::new(None, None);
    let window_id = window.window_id();
    window.push(&tab);
    mux.insert_window_for_test(window_id, window);

    let domain = ResizeDuringSpawnDomain {
        tab: Arc::clone(&tab),
        size: TerminalSize {
            cols: 2,
            ..Default::default()
        },
        spawned: Arc::clone(&spawned),
    };
    let result = onlyterm_promise::spawn::block_on(domain.split_pane(
        SplitSource::Spawn {
            command: None,
            command_dir: None,
        },
        tab.tab_id(),
        target.pane_id(),
        SplitRequest::default(),
    ));
    assert!(result.is_err());
    assert_eq!(tab.get_size().cols, 2);
    assert!(tab.contains_pane(target.pane_id()));
    assert!(mux.get_pane(spawned.pane_id()).is_none());
    assert_eq!(*spawned.windows_lock_was_free_on_kill.lock(), Some(true));
    Mux::shutdown();
}
