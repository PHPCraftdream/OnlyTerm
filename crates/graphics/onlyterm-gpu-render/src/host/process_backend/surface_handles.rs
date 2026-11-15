//! Ownership of the DirectComposition surface handles this process creates:
//! one per child generation, closed as soon as nothing can use them any more.

use std::collections::HashMap;
use windows::Win32::Foundation::{CloseHandle, HANDLE};

/// A composition-surface handle owned by this process; closed on drop.
pub(super) struct SurfaceHandle(HANDLE);

impl SurfaceHandle {
    /// # Safety
    ///
    /// `handle` must be open, and nothing else may close it.
    pub(super) unsafe fn from_raw(handle: HANDLE) -> Self {
        Self(handle)
    }

    pub(super) fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for SurfaceHandle {
    fn drop(&mut self) {
        // SAFETY: `from_raw`'s contract: the handle is open and ours alone.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// The handles of one backend: those waiting for their generation's first
/// ack, and the one whose surface is on screen.
#[derive(Default)]
pub(super) struct SurfaceHandles {
    pending: HashMap<u64, SurfaceHandle>,
    displayed: Option<SurfaceHandle>,
}

impl SurfaceHandles {
    pub(super) fn add_pending(&mut self, generation: u64, handle: SurfaceHandle) {
        self.pending.insert(generation, handle);
    }

    /// Hands out `generation`'s handle for its first swap into the visual.
    ///
    /// A child generation that died before acking a frame leaves its handle
    /// here for good, and nothing newer can ever present through it: every
    /// handle of an older generation is closed now.
    pub(super) fn take_for_swap(&mut self, generation: u64) -> Option<SurfaceHandle> {
        let claimed = self.pending.remove(&generation);
        self.pending.retain(|&pending, _| pending > generation);
        claimed
    }

    /// Keeps the handle of the surface now on screen open for as long as it
    /// is shown; the one it replaces is closed.
    pub(super) fn set_displayed(&mut self, handle: SurfaceHandle) {
        self.displayed = Some(handle);
    }

    #[cfg(test)]
    pub(super) fn pending_generations(&self) -> Vec<u64> {
        let mut generations: Vec<u64> = self.pending.keys().copied().collect();
        generations.sort_unstable();
        generations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::GetHandleInformation;
    use windows::Win32::System::Threading::CreateEventW;

    /// A real kernel handle, so "closed" can be observed through the OS.
    fn new_handle() -> SurfaceHandle {
        // SAFETY: an unnamed event with default security; the returned
        // handle is open and owned by the wrapper alone.
        unsafe {
            let event = CreateEventW(None, true, false, PCWSTR::null()).expect("CreateEventW");
            SurfaceHandle::from_raw(event)
        }
    }

    fn is_open(handle: HANDLE) -> bool {
        let mut flags = 0u32;
        // SAFETY: `flags` is a live output slot; an invalid handle only
        // makes the call fail.
        unsafe { GetHandleInformation(handle, &mut flags) }.is_ok()
    }

    #[test]
    fn dropping_a_handle_closes_it() {
        let handle = new_handle();
        let raw = handle.raw();
        assert!(is_open(raw));
        drop(handle);
        assert!(!is_open(raw));
    }

    #[test]
    fn the_swap_claims_its_own_generation_and_closes_older_ones() {
        let mut handles = SurfaceHandles::default();
        let (g1, g2, g3) = (new_handle(), new_handle(), new_handle());
        let (r1, r2, r3) = (g1.raw(), g2.raw(), g3.raw());
        handles.add_pending(1, g1);
        handles.add_pending(2, g2);
        handles.add_pending(3, g3);

        let claimed = handles.take_for_swap(2).expect("generation 2 is pending");
        assert_eq!(claimed.raw(), r2);
        assert!(!is_open(r1), "an older generation's handle must be closed");
        assert!(is_open(r2), "the claimed handle is the caller's now");
        assert!(is_open(r3), "a newer generation's handle must stay");
        assert_eq!(handles.pending_generations(), vec![3]);
    }

    #[test]
    fn claiming_an_unknown_generation_still_closes_older_ones() {
        let mut handles = SurfaceHandles::default();
        let old = new_handle();
        let raw = old.raw();
        handles.add_pending(1, old);

        assert!(handles.take_for_swap(5).is_none());
        assert!(!is_open(raw));
        assert!(handles.pending_generations().is_empty());
    }

    #[test]
    fn the_displayed_handle_stays_open_until_replaced() {
        let mut handles = SurfaceHandles::default();
        let first = new_handle();
        let second = new_handle();
        let (r1, r2) = (first.raw(), second.raw());

        handles.set_displayed(first);
        assert!(is_open(r1));
        handles.set_displayed(second);
        assert!(!is_open(r1), "the replaced surface's handle must be closed");
        assert!(is_open(r2));

        drop(handles);
        assert!(!is_open(r2), "dropping the backend closes the shown handle");
    }
}
