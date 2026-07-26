use chrono::prelude::*;
use std::sync::Mutex;

use config::ConfigSubscription;

lazy_static::lazy_static! {
    static ref CONFIG_SUBSCRIPTION: Mutex<Option<ConfigSubscription>> = Mutex::new(None);
}

/// We contrive to call this from the main thread in response to the
/// config being reloaded. It spawns a task for each of the timers that have
/// been configured by the user via `wezterm.time.call_after`.
///
/// NOTE: `wezterm.time.call_after` has no rhai equivalent yet (this crate has
/// no `register_rhai`). `schedule_all` is a no-op until that port lands; it is
/// kept as the skeleton a future rhai `call_after` port would build on.
fn schedule_all() {
    // Intentionally a no-op now; see the doc comment above.
}

/// Helper to schedule !Send futures to run with access to the config
/// on the main thread.
fn schedule_trampoline() {
    schedule_all();
}

/// Called by the config subsystem when the config is reloaded.
/// We use it to schedule our setup function that will schedule
/// the call_after functions from the main thread.
pub fn config_was_reloaded() -> bool {
    if promise::spawn::is_scheduler_configured() {
        promise::spawn::spawn_into_main_thread(async move {
            schedule_trampoline();
        })
        .detach();
    }

    true
}

#[derive(Clone, Debug)]
pub struct Time {
    pub utc: DateTime<Utc>,
}
