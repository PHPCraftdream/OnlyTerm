use promise::spawn::Task;
use std::sync::Arc;

pub(super) fn preceding_chunk(
    end: onlyterm_term::StableRowIndex,
    earliest: onlyterm_term::StableRowIndex,
) -> Option<std::ops::Range<onlyterm_term::StableRowIndex>> {
    (end > earliest).then(|| end.saturating_sub(super::SEARCH_CHUNK_SIZE).max(earliest)..end)
}

/// Own disposable search work; async-task 4.7 cancels on handle drop.
#[derive(Default)]
pub(super) struct SearchJobs {
    generation: Arc<()>,
    worker: Option<Task<()>>,
    debounce: Option<Task<()>>,
}

impl SearchJobs {
    pub fn cancel(&mut self) {
        *self = Self::default();
    }

    pub fn generation(&self) -> Arc<()> {
        Arc::clone(&self.generation)
    }

    pub fn is_current(&self, generation: &Arc<()>) -> bool {
        Arc::ptr_eq(&self.generation, generation)
    }

    pub fn set_worker(&mut self, task: Task<()>) {
        self.worker = Some(task);
    }

    pub fn set_debounce(&mut self, task: Task<()>) {
        self.debounce = Some(task);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_stops_when_remaining_history_was_evicted() {
        assert_eq!(preceding_chunk(100, 101), None);
        assert_eq!(preceding_chunk(100, 100), None);
        assert_eq!(preceding_chunk(100, 90), Some(90..100));
        assert_eq!(preceding_chunk(2000, 0), Some(1000..2000));
    }

    struct OnDrop(smol::channel::Sender<()>);

    impl Drop for OnDrop {
        fn drop(&mut self) {
            let _ = self.0.try_send(());
        }
    }

    fn pending_job(executor: &smol::Executor<'static>, tx: smol::channel::Sender<()>) -> Task<()> {
        let guard = OnDrop(tx);
        executor.spawn(async move {
            let _guard = guard;
            smol::future::pending::<()>().await;
        })
    }

    #[test]
    fn replacing_or_dropping_search_cancels_worker_and_debounce() {
        for replace in [false, true] {
            let executor = smol::Executor::new();
            let (tx, rx) = smol::channel::bounded(2);
            let mut jobs = SearchJobs::default();
            jobs.set_worker(pending_job(&executor, tx.clone()));
            jobs.set_debounce(pending_job(&executor, tx));
            while executor.try_tick() {}
            assert!(rx.try_recv().is_err());
            if replace {
                jobs.cancel();
            } else {
                drop(jobs);
            }
            while executor.try_tick() {}
            assert!(
                rx.try_recv().is_ok(),
                "first task resources must be released"
            );
            assert!(
                rx.try_recv().is_ok(),
                "second task resources must be released"
            );
        }
    }

    #[test]
    fn completed_notifications_cannot_cross_search_or_overlay_generations() {
        let mut jobs = SearchJobs::default();
        let queued_completion = jobs.generation();
        assert!(jobs.is_current(&queued_completion));
        jobs.cancel();
        assert!(!jobs.is_current(&queued_completion));
        let reopened_overlay = SearchJobs::default();
        assert!(!reopened_overlay.is_current(&jobs.generation()));
    }
}
