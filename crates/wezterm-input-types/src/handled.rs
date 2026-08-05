use alloc::sync::Arc;
use core::sync::atomic::AtomicBool;

#[derive(Debug, Clone)]
pub struct Handled(Arc<AtomicBool>);

impl Default for Handled {
    fn default() -> Self {
        Self::new()
    }
}

impl Handled {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn set_handled(&self) {
        self.0.store(true, core::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_handled(&self) -> bool {
        self.0.load(core::sync::atomic::Ordering::Relaxed)
    }
}

impl PartialEq for Handled {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Handled {}
