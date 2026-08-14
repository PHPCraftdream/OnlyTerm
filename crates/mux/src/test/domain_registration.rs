//! Unit tests for domain registration symmetry: `add_domain` and `remove_domain`.
//!
//! These tests verify that domains can be added and removed cleanly, and that
//! both the `domains` and `domains_by_name` maps stay in sync.

use super::*;
use crate::{Domain, DomainId, Mux};
use async_trait::async_trait;
use portable_pty::CommandBuilder;
use std::sync::Arc;
use wezterm_term::TerminalSize;

/// A minimal Domain implementation for testing domain registration.
struct TestDomain {
    id: DomainId,
    name: String,
}

impl TestDomain {
    fn new(id: DomainId, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
        }
    }
}

#[async_trait(?Send)]
impl Domain for TestDomain {
    fn domain_id(&self) -> DomainId {
        self.id
    }

    fn domain_name(&self) -> &str {
        &self.name
    }

    async fn domain_label(&self) -> String {
        self.name.clone()
    }

    // All other Domain methods are unreachable in these tests.
    async fn spawn_pane(
        &self,
        _size: TerminalSize,
        _command: Option<CommandBuilder>,
        _command_dir: Option<String>,
    ) -> anyhow::Result<Arc<dyn crate::pane::Pane>> {
        unreachable!("TestDomain::spawn_pane should not be called in these tests")
    }

    fn detachable(&self) -> bool {
        unreachable!("TestDomain::detachable should not be called in these tests")
    }

    fn detach(&self) -> anyhow::Result<()> {
        unreachable!("TestDomain::detach should not be called in these tests")
    }

    async fn attach(&self, _window_id: Option<crate::window::WindowId>) -> anyhow::Result<()> {
        unreachable!("TestDomain::attach should not be called in these tests")
    }

    fn state(&self) -> crate::domain::DomainState {
        unreachable!("TestDomain::state should not be called in these tests")
    }

    fn spawnable(&self) -> bool {
        unreachable!("TestDomain::spawnable should not be called in these tests")
    }
}

#[test]
fn remove_domain_clears_both_maps() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    // Create and add two test domains.
    let domain1: Arc<dyn Domain> = Arc::new(TestDomain::new(1, "domain1"));
    let domain2: Arc<dyn Domain> = Arc::new(TestDomain::new(2, "domain2"));

    mux.add_domain(&domain1);
    mux.add_domain(&domain2);

    // Verify both domains are in both maps.
    assert!(mux.get_domain(1).is_some());
    assert!(mux.get_domain(2).is_some());
    assert!(mux.get_domain_by_name("domain1").is_some());
    assert!(mux.get_domain_by_name("domain2").is_some());

    // Remove the first domain.
    mux.remove_domain(&domain1);

    // Verify it's gone from both maps.
    assert!(mux.get_domain(1).is_none());
    assert!(mux.get_domain_by_name("domain1").is_none());

    // Verify the second domain is still there.
    assert!(mux.get_domain(2).is_some());
    assert!(mux.get_domain_by_name("domain2").is_some());

    Mux::shutdown();
}

#[test]
fn remove_domain_is_idempotent() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let domain: Arc<dyn Domain> = Arc::new(TestDomain::new(1, "domain"));

    // Add the domain.
    mux.add_domain(&domain);
    assert!(mux.get_domain(1).is_some());

    // Remove it once.
    mux.remove_domain(&domain);
    assert!(mux.get_domain(1).is_none());

    // Remove it again - should not panic.
    mux.remove_domain(&domain);
    assert!(mux.get_domain(1).is_none());

    Mux::shutdown();
}

#[test]
fn remove_domain_on_never_registered_domain_is_harmless() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let domain: Arc<dyn Domain> = Arc::new(TestDomain::new(1, "domain"));

    // Remove without ever adding - should not panic.
    mux.remove_domain(&domain);

    // Verify maps are empty.
    assert!(mux.get_domain(1).is_none());
    assert!(mux.get_domain_by_name("domain").is_none());

    Mux::shutdown();
}

#[test]
fn remove_domain_clears_default_domain_if_needed() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    // Create and add a domain - it becomes the default since there was none.
    let domain1: Arc<dyn Domain> = Arc::new(TestDomain::new(1, "domain1"));
    mux.add_domain(&domain1);
    assert_eq!(mux.default_domain().domain_id(), 1);

    // Add a second domain - it does NOT become the default.
    let domain2: Arc<dyn Domain> = Arc::new(TestDomain::new(2, "domain2"));
    mux.add_domain(&domain2);
    assert_eq!(mux.default_domain().domain_id(), 1);

    // Remove the default domain - the second domain should become the new default.
    mux.remove_domain(&domain1);
    assert_eq!(mux.default_domain().domain_id(), 2);

    // Remove the remaining domain - default should be cleared.
    mux.remove_domain(&domain2);
    // Verify that there is no default domain by checking the internal state.
    assert!(mux.default_domain.read().is_none());

    Mux::shutdown();
}

#[test]
fn remove_domain_with_non_default_does_not_change_default() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    let domain1: Arc<dyn Domain> = Arc::new(TestDomain::new(1, "domain1"));
    let domain2: Arc<dyn Domain> = Arc::new(TestDomain::new(2, "domain2"));

    mux.add_domain(&domain1);
    mux.add_domain(&domain2);

    assert_eq!(mux.default_domain().domain_id(), 1);

    // Remove the non-default domain - default should not change.
    mux.remove_domain(&domain2);
    assert_eq!(mux.default_domain().domain_id(), 1);

    // Verify the removed domain is gone.
    assert!(mux.get_domain(2).is_none());
    assert!(mux.get_domain_by_name("domain2").is_none());

    // Verify the default domain is still there.
    assert!(mux.get_domain(1).is_some());
    assert!(mux.get_domain_by_name("domain1").is_some());

    Mux::shutdown();
}

#[test]
fn add_domain_then_remove_domain_restores_state() {
    let _test_guard = TEST_LOCK.lock();
    let _mux_guard = MUX_TEST_GUARD.lock();

    let mux = Arc::new(Mux::new(None));
    Mux::set_mux(&mux);

    // Start with an empty mux.
    assert!(mux.domains.read().is_empty());
    assert!(mux.domains_by_name.read().is_empty());
    assert!(mux.default_domain.read().is_none());

    // Add a domain.
    let domain: Arc<dyn Domain> = Arc::new(TestDomain::new(1, "domain"));
    mux.add_domain(&domain);

    // State should reflect the added domain.
    assert_eq!(mux.domains.read().len(), 1);
    assert_eq!(mux.domains_by_name.read().len(), 1);
    assert!(mux.default_domain.read().is_some());

    // Remove the domain.
    mux.remove_domain(&domain);

    // State should be back to empty.
    assert!(mux.domains.read().is_empty());
    assert!(mux.domains_by_name.read().is_empty());
    assert!(mux.default_domain.read().is_none());

    Mux::shutdown();
}
