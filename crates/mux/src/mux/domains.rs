use super::*;

impl Mux {
    pub fn default_domain(&self) -> Arc<dyn Domain> {
        self.default_domain.read().as_ref().map(Arc::clone).unwrap()
    }

    pub fn set_default_domain(&self, domain: &Arc<dyn Domain>) {
        *self.default_domain.write() = Some(Arc::clone(domain));
    }

    pub fn get_domain(&self, id: DomainId) -> Option<Arc<dyn Domain>> {
        self.domains.read().get(&id).cloned()
    }

    pub fn get_domain_by_name(&self, name: &str) -> Option<Arc<dyn Domain>> {
        self.domains_by_name.read().get(name).cloned()
    }

    /// Removes a domain from the mux, clearing it from both the `domains`
    /// and `domains_by_name` maps. If the removed domain is the current
    /// default domain, the default is cleared (or updated to another
    /// domain if any remain).
    ///
    /// This is safe to call on a domain that was never registered, or
    /// on a domain that has already been removed; the function is idempotent.
    ///
    /// # Panics
    /// This function does not panic, but calling it while holding a lock
    /// on `self.windows` (as `domain_was_detached` does) requires care to
    /// avoid re-entrancy.
    pub fn remove_domain(&self, domain: &Arc<dyn Domain>) {
        let domain_id = domain.domain_id();
        let domain_name = domain.domain_name();

        let was_in_domains = self.domains.write().remove(&domain_id).is_some();

        // Only drop the name entry if it still points at *this* domain.
        // `add_domain` inserts by name unconditionally, so the name map is
        // last-writer-wins: if two domains ever shared a name, removing the
        // older one must not evict the live newer one's entry.
        {
            let mut by_name = self.domains_by_name.write();
            if by_name
                .get(domain_name)
                .is_some_and(|d| d.domain_id() == domain_id)
            {
                by_name.remove(domain_name);
            }
        }

        // Choose the replacement default *before* taking the `default_domain`
        // lock, so this never holds two of these locks at once. `add_domain`
        // acquires them in the opposite order (default_domain, then domains),
        // and nesting them here the other way round is how a lock-order
        // inversion gets built by accident.
        //
        // Note "first remaining" is arbitrary, not stable: `domains` is a
        // HashMap, so iteration order is unspecified and varies between runs.
        // Any surviving domain is an equally valid default, and in practice
        // this branch should not be reachable at all -- the local domain is
        // registered first at startup and is never removed, so a per-tab
        // domain is never the default. It is handled anyway, because leaving a
        // dangling default behind would be a worse bug than the leak this
        // function exists to fix.
        if was_in_domains {
            let replacement = self.domains.read().values().next().map(Arc::clone);
            let mut default_guard = self.default_domain.write();
            let is_default = default_guard
                .as_ref()
                .is_some_and(|current| current.domain_id() == domain_id);
            if is_default {
                *default_guard = replacement;
            }
        }
    }

    pub fn add_domain(&self, domain: &Arc<dyn Domain>) {
        if self.default_domain.read().is_none() {
            *self.default_domain.write() = Some(Arc::clone(domain));
        }
        self.domains
            .write()
            .insert(domain.domain_id(), Arc::clone(domain));
        self.domains_by_name
            .write()
            .insert(domain.domain_name().to_string(), Arc::clone(domain));
    }

    pub fn set_mux(mux: &Arc<Mux>) {
        MUX.lock().replace(Arc::clone(mux));
    }

    pub fn shutdown() {
        MUX.lock().take();
    }

    pub fn get() -> Arc<Mux> {
        Self::try_get().unwrap()
    }

    pub fn try_get() -> Option<Arc<Mux>> {
        MUX.lock().as_ref().map(Arc::clone)
    }
}
