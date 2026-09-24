use super::*;
// See `crate::test::MUX_TEST_GUARD`: `allocate` reaches into the
// process-global `Mux` singleton (`Mux::set_mux`/`Mux::get`), so tests
// that install one must run serially with every other such test in the
// crate, not just within this module.
use crate::test::MUX_TEST_GUARD;

fn test_term_config() -> Arc<dyn TerminalConfiguration + Send + Sync> {
    Arc::new(config::TermConfig::new())
}

#[test]
fn allocate_succeeds_under_normal_conditions() {
    let _test_guard = crate::test::TEST_LOCK.lock();
    let _guard = MUX_TEST_GUARD.lock();

    // `allocate` starts the pane reader. Dropping its writer at the end of
    // the test makes that reader schedule its EOF cleanup through the
    // process-global promise scheduler. Queue the runnable so this test can
    // run it while the mux singleton is still installed.
    let (schedule_tx, schedule_rx) = std::sync::mpsc::channel();
    promise::spawn::set_schedulers(
        Box::new({
            let schedule_tx = schedule_tx.clone();
            move |runnable| {
                let _ = schedule_tx.send(runnable);
            }
        }),
        Box::new(move |runnable| {
            let _ = schedule_tx.send(runnable);
        }),
    );

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let size = TerminalSize::default();
    let result = allocate(size, test_term_config());
    assert!(
        result.is_ok(),
        "allocate() should succeed when fds/pipes are available: {:?}",
        result.err()
    );

    let (term, _pane) = result.expect("allocate succeeded above");
    drop(term);
    schedule_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("reader EOF should schedule mux cleanup")
        .run();

    // The other parser tests use an inline scheduler, so leave that
    // process-global default in place once this queued task is drained.
    promise::spawn::set_schedulers(
        Box::new(|runnable| {
            runnable.run();
        }),
        Box::new(|runnable| {
            runnable.run();
        }),
    );
    Mux::shutdown();
}
