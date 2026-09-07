use super::*;

impl LocalPane {
    // Release terminal.lock between chunks; retain resize_guard for the batch.
    pub(super) fn perform_actions_chunked(&self, actions: Vec<Action>, chunk_size: usize) {
        self.with_action_chunks(actions, chunk_size, |chunk| {
            lock_terminal_timed(
                &self.terminal,
                "localpane.terminal_lock.wait.perform_actions",
                |term| term.perform_actions_reusing(chunk),
            );
        });
    }

    // Both production and measurement use this discipline. apply drains its chunk.
    fn with_action_chunks(
        &self,
        mut actions: Vec<Action>,
        chunk_size: usize,
        mut apply: impl FnMut(&mut Vec<Action>),
    ) {
        let chunk_size = chunk_size.max(1);
        if actions.len() <= chunk_size {
            apply(&mut actions);
            return;
        }
        let _resize_guard = self.resize_guard.lock();
        let mut chunk = Vec::with_capacity(chunk_size);
        for action in actions {
            chunk.push(action);
            if chunk.len() == chunk_size {
                apply(&mut chunk);
            }
        }
        if !chunk.is_empty() {
            apply(&mut chunk);
        }
    }

    #[cfg(test)]
    pub(crate) fn perform_actions_chunked_timed(
        &self,
        actions: Vec<Action>,
        chunk_size: usize,
    ) -> Vec<(Duration, Duration)> {
        self.perform_actions_chunked_measured(actions, chunk_size).0
    }

    #[cfg(test)]
    pub(crate) fn perform_actions_chunked_measured(
        &self,
        actions: Vec<Action>,
        chunk_size: usize,
    ) -> (Vec<(Duration, Duration)>, Vec<usize>) {
        let mut samples = Vec::new();
        let mut sizes = Vec::new();
        self.with_action_chunks(actions, chunk_size, |chunk| {
            let wait_start = Instant::now();
            let mut term = self.terminal.lock();
            let waited = wait_start.elapsed();
            let hold_start = Instant::now();
            sizes.push(chunk.len());
            term.perform_actions_reusing(chunk);
            samples.push((waited, hold_start.elapsed()));
        });
        (samples, sizes)
    }
}
