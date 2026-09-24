use super::*;

impl Mux {
    pub fn subscribe<F>(&self, subscriber: F)
    where
        F: Fn(MuxNotification) -> bool + 'static + Send + Sync,
    {
        let sub_id = LAST_SUBSCRIBER_ID.fetch_add(1, Ordering::Relaxed);
        self.subscribers
            .write()
            .insert(sub_id, Arc::new(subscriber));
    }

    pub fn notify(&self, notification: MuxNotification) {
        let mut notification = notification;
        // Consecutive rounds delivered synchronously in this call, without
        // returning to the event loop. Bounded by PANE_OUTPUT_MAX_SYNC_ROUNDS
        // below -- see its doc comment for why this loop exists instead of
        // the reschedule calling back into `notify` recursively.
        let mut sync_rounds: usize = 0;

        loop {
            let (pane_id, initial_coalesce_count) =
                if let MuxNotification::PaneOutput(pane_id) = &notification {
                    // Capture the coalesce_count BEFORE we start delivering.
                    // If it increases during delivery, output arrived during the callback.
                    let coalesce_count = {
                        let state = self.pane_output_notify_state.lock();
                        state.get(pane_id).map(|(_, _, count)| *count).unwrap_or(0)
                    };
                    (Some(*pane_id), coalesce_count)
                } else {
                    (None, 0)
                };

            // Snapshot the subscriber ids+callbacks under a short-lived read
            // lock, then release it before invoking any callbacks. Subscriber
            // callbacks are arbitrary code (eg. GUI code that ends up calling
            // back into `Mux::get_window`, or even `Mux::subscribe` /
            // `Mux::notify` again); if we held `subscribers.write()` for the
            // duration of those calls -- as a naive `retain`-based
            // implementation would -- any callback that (transitively) touches
            // `self.subscribers` would deadlock against this non-reentrant
            // `RwLock`, and every other subscriber would be blocked from
            // observing the notification until the slowest callback returns.
            //
            // Note: cloning the `Arc<Fn>` payload (rather than the raw
            // `Box<Fn>`) is what makes a cheap snapshot possible without
            // moving the callbacks out of the map, so a second pass can later
            // reconcile the live set using the same ids.
            let snapshot: Vec<(usize, MuxSubscriber)> = {
                let subscribers = self.subscribers.read();
                subscribers
                    .iter()
                    .map(|(id, notify)| (*id, Arc::clone(notify)))
                    .collect()
            };

            // Invoke the callbacks with no mux lock held at all, recording
            // which subscriber ids asked to be removed (by returning `false`).
            let mut dead = Vec::new();
            for (id, notify) in &snapshot {
                if !notify(notification.clone()) {
                    dead.push(*id);
                }
            }

            // Reconcile: drop only the ids that asked to unsubscribe *and*
            // are still present (a concurrent `subscribe` could have reused
            // an id only via `LAST_SUBSCRIBER_ID`, which never repeats, so
            // this is simply "remove if still there").
            if !dead.is_empty() {
                let mut subscribers = self.subscribers.write();
                for id in dead {
                    subscribers.remove(&id);
                }
            }

            let pane_id = match pane_id {
                Some(pane_id) => pane_id,
                None => return,
            };

            // After all subscribers have been notified, clear the pending marker
            // for PaneOutput notifications and check if we need to schedule a
            // follow-up delivery. We schedule a follow-up only if coalesce_count
            // increased DURING the delivery (meaning output arrived from within
            // the callback), not if it was already set BEFORE the delivery started.
            let should_reschedule = {
                let mut state = self.pane_output_notify_state.lock();
                if let Some((_, has_more_output, coalesce_count)) = state.get_mut(&pane_id) {
                    // Check if coalesce_count increased during delivery.
                    // If it did, output arrived from within the callback.
                    let coalesce_count_increased = *coalesce_count > initial_coalesce_count;
                    if coalesce_count_increased {
                        // Keep in_flight=true for the new delivery, reset everything else
                        *coalesce_count = 0;
                        *has_more_output = false;
                    } else {
                        // No new output during delivery; clean up
                        state.remove(&pane_id);
                    }
                    coalesce_count_increased
                } else {
                    false
                }
            };

            if !should_reschedule {
                return;
            }

            sync_rounds += 1;
            if sync_rounds >= PANE_OUTPUT_MAX_SYNC_ROUNDS {
                // A pane with continuous output could otherwise keep this
                // loop -- or, before this change, a `notify`/`dispatch_notification`
                // recursion -- going indefinitely, growing the stack without
                // bound and starving every other subscriber/window of a
                // chance to run. Hand the next round to the event loop
                // instead of continuing synchronously: `spawn_into_main_thread`
                // runs on a fresh call stack once this call has returned, so
                // stack depth stays O(1) regardless of how long the output
                // keeps coming, and other pending work gets to run in between.
                metrics::counter!("mux.pane_output.yielded").increment(1);
                let notification = MuxNotification::PaneOutput(pane_id);
                promise::spawn::spawn_into_main_thread(async move {
                    if let Some(mux) = Mux::try_get() {
                        mux.notify(notification);
                    }
                })
                .detach();
                return;
            }

            // More output arrived while we were delivering; loop back and
            // deliver another round in this same call, rather than
            // recursing into `dispatch_notification` -> `notify` again.
            metrics::counter!("mux.pane_output.rescheduled").increment(1);
            notification = MuxNotification::PaneOutput(pane_id);
        }
    }

    /// Schedules `notification` for delivery on the main thread (or
    /// delivers it synchronously if already there).
    ///
    /// `PaneOutput` gets special-cased to coalesce: `parse_buffered_data`
    /// calls this once per parser flush (coalesce delay
    /// `mux_output_parser_coalesce_delay_ms`, default 3ms), which under
    /// sustained pty output can mean hundreds of calls per second per
    /// pane. Each call that reaches the `spawn_into_main_thread` branch
    /// below costs a hop through the main-thread spawn queue. To bound
    /// the queue depth, we track pending delivery per pane and only
    /// spawn when no delivery is already in-flight. When a delivery
    /// completes, we check if more output arrived and schedule exactly
    /// one more delivery if needed. `mux_pane_output_event` always
    /// inspects current pane state (not a snapshot carried by the
    /// notification), so the single delivery that does go through still
    /// reflects whatever is the latest state by the time it runs --
    /// coalescing only elides redundant wakeups, it never drops data.
    pub fn notify_from_any_thread(notification: MuxNotification) {
        if let MuxNotification::PaneOutput(pane_id) = &notification {
            let mux = match Mux::try_get() {
                Some(mux) => mux,
                None => {
                    return Self::dispatch_notification(notification);
                }
            };

            let should_schedule = {
                let mut state = mux.pane_output_notify_state.lock();
                let entry = state.entry(*pane_id).or_insert((false, false, 0));
                if entry.0 {
                    // Already delivering; just mark that more arrived
                    entry.1 = true;
                    entry.2 += 1; // Track that we coalesced during this delivery
                    metrics::counter!("mux.pane_output.coalesced").increment(1);
                    false
                } else {
                    // Not delivering; schedule delivery now
                    entry.0 = true;
                    entry.2 = 0; // Reset coalesce counter for new delivery
                    metrics::counter!("mux.pane_output.scheduled").increment(1);
                    true
                }
            };

            if !should_schedule {
                return;
            }
        }
        Self::dispatch_notification(notification);
    }

    fn dispatch_notification(notification: MuxNotification) {
        if let Some(mux) = Mux::try_get() {
            if mux.is_main_thread() {
                mux.notify(notification);
                return;
            }
        }
        promise::spawn::spawn_into_main_thread(async {
            if let Some(mux) = Mux::try_get() {
                mux.notify(notification);
            }
        })
        .detach();
    }
}
