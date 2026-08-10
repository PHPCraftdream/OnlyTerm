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
    let _guard = MUX_TEST_GUARD.lock();
    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let size = TerminalSize::default();
    let result = allocate(size, test_term_config());
    assert!(
        result.is_ok(),
        "allocate() should succeed when fds/pipes are available: {:?}",
        result.err()
    );

    Mux::shutdown();
}
