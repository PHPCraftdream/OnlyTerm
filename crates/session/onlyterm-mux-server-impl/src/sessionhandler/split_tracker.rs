use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub(crate) struct SplitTracker {
    active: AtomicUsize,
    idle_tx: smol::channel::Sender<()>,
    idle_rx: smol::channel::Receiver<()>,
}

pub(crate) struct SplitCompletion {
    tracker: Arc<SplitTracker>,
}

impl Drop for SplitCompletion {
    fn drop(&mut self) {
        if self.tracker.active.fetch_sub(1, Ordering::SeqCst) == 1 {
            let _ = self.tracker.idle_tx.try_send(());
        }
    }
}

impl SplitTracker {
    pub(crate) fn new() -> Arc<Self> {
        let (idle_tx, idle_rx) = smol::channel::bounded(1);
        Arc::new(Self {
            active: AtomicUsize::new(0),
            idle_tx,
            idle_rx,
        })
    }

    pub(crate) fn start(self: &Arc<Self>) -> SplitCompletion {
        self.active.fetch_add(1, Ordering::SeqCst);
        SplitCompletion {
            tracker: Arc::clone(self),
        }
    }

    pub(crate) fn active(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    /// cancel-safe: yes; the count remains authoritative if waiting stops.
    pub(crate) async fn wait_idle(&self) {
        while self.active() > 0 {
            self.idle_rx
                .recv()
                .await
                .expect("tracker retains the idle sender");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::task::{Context, Poll};

    #[test]
    fn idle_signal_waits_for_the_last_split() {
        let tracker = SplitTracker::new();
        let first = tracker.start();
        let second = tracker.start();
        assert_eq!(tracker.active(), 2);

        drop(first);
        assert_eq!(tracker.active(), 1);
        assert!(tracker.idle_rx.try_recv().is_err());

        drop(second);
        assert_eq!(tracker.active(), 0);
        assert!(tracker.idle_rx.try_recv().is_ok());
    }

    #[test]
    fn waiting_for_idle_is_event_driven() {
        let tracker = SplitTracker::new();
        let completion = tracker.start();
        let mut wait = Box::pin(tracker.wait_idle());
        let waker = futures::task::noop_waker();
        let mut context = Context::from_waker(&waker);

        assert!(matches!(wait.as_mut().poll(&mut context), Poll::Pending));
        drop(completion);
        assert!(matches!(wait.as_mut().poll(&mut context), Poll::Ready(())));
    }
}
