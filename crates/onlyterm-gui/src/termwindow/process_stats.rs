use std::time::Instant;

/// One sample of this process tree's cumulative CPU time, taken at a known
/// wall-clock instant. CPU *time* is monotonically increasing and cheap to
/// read (`GetProcessTimes` per process); CPU *percentage* only exists as the
/// derivative between two samples, so this is the state a periodic sampler
/// must keep between ticks.
#[derive(Debug, Clone, Copy)]
pub(super) struct UsageSample {
    pub at: Instant,
    pub total_cpu_time_100ns: u64,
}

/// Builds the window-title suffix from two CPU-time samples plus the
/// current memory reading. Pure and unit-testable: no WinAPI calls here,
/// only arithmetic and formatting.
///
/// `cpu_percent` is normalized against the *whole machine's* capacity
/// (`logical_cpus` cores over the sampled wall-clock interval), matching
/// Task Manager's per-process display convention, so the maximum possible
/// value is 100% regardless of how many cores this process tree is using.
///
/// The returned string has no leading separator -- the caller joins it to
/// the base title with " — " (an em dash, distinct from the plain hyphens
/// used between the values inside this suffix).
pub(super) fn format_usage_suffix(
    prev: &UsageSample,
    now: &UsageSample,
    working_set_bytes: u64,
    total_ram_bytes: u64,
    logical_cpus: usize,
) -> String {
    let elapsed_100ns = now
        .at
        .saturating_duration_since(prev.at)
        .as_nanos()
        .saturating_div(100)
        .max(1) as u64;
    let cpu_delta_100ns = now
        .total_cpu_time_100ns
        .saturating_sub(prev.total_cpu_time_100ns);
    let capacity_100ns = elapsed_100ns.saturating_mul(logical_cpus.max(1) as u64);
    let cpu_percent = if capacity_100ns == 0 {
        0
    } else {
        ((cpu_delta_100ns as u128 * 100) / capacity_100ns as u128).min(100) as u32
    };

    let mem_percent = if total_ram_bytes == 0 {
        0
    } else {
        ((working_set_bytes as u128 * 100) / total_ram_bytes as u128).min(100) as u32
    };
    let mem_gb = working_set_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

    format!("CPU {cpu_percent}% - RAM {mem_gb:.1} GB - {mem_percent}%")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Builds a sample `offset_secs` after a shared `base` instant. Callers
    /// must derive both samples of a pair from the *same* `base` call --
    /// two independent `Instant::now()` calls pick up whatever tiny real
    /// wall-clock time elapses between them on top of the intended offset,
    /// which is exactly the kind of skew these tests are checking the
    /// production math is *not* sensitive to by construction.
    fn sample(base: Instant, offset_secs: u64, total_cpu_time_100ns: u64) -> UsageSample {
        UsageSample {
            at: base + Duration::from_secs(offset_secs),
            total_cpu_time_100ns,
        }
    }

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn idle_process_tree_reports_zero_percent_cpu() {
        let base = Instant::now();
        let prev = sample(base, 0, 1_000_000);
        let now = sample(base, 5, 1_000_000); // no CPU time consumed in 5s
        let suffix = format_usage_suffix(&prev, &now, GIB, 16 * GIB, 4);
        assert_eq!(suffix, "CPU 0% - RAM 1.0 GB - 6%");
    }

    #[test]
    fn one_core_fully_busy_on_a_four_core_machine_is_25_percent() {
        let base = Instant::now();
        let prev = sample(base, 0, 0);
        // 5 seconds of wall-clock, one full core's worth of CPU time burned
        // (5s in 100ns units = 5 * 10_000_000).
        let now = sample(base, 5, 5 * 10_000_000);
        let suffix = format_usage_suffix(&prev, &now, GIB, 4 * GIB, 4);
        assert_eq!(suffix, "CPU 25% - RAM 1.0 GB - 25%");
    }

    #[test]
    fn cpu_percent_is_clamped_to_100_even_if_all_cores_are_saturated() {
        let base = Instant::now();
        let prev = sample(base, 0, 0);
        // 5 seconds wall-clock, 4 cores fully busy (4x the elapsed time).
        let now = sample(base, 5, 4 * 5 * 10_000_000);
        let suffix = format_usage_suffix(&prev, &now, GIB, 4 * GIB, 4);
        assert!(suffix.starts_with("CPU 100% -"));
    }

    #[test]
    fn memory_percent_is_clamped_to_100() {
        let base = Instant::now();
        let prev = sample(base, 0, 0);
        let now = sample(base, 1, 0);
        // Working set larger than "total RAM" shouldn't happen in practice,
        // but must not overflow the percentage or panic.
        let suffix = format_usage_suffix(&prev, &now, 8 * GIB, 4 * GIB, 4);
        assert!(suffix.ends_with("- 100%"));
    }

    #[test]
    fn memory_gb_is_formatted_with_one_decimal_place() {
        let base = Instant::now();
        let prev = sample(base, 0, 0);
        let now = sample(base, 1, 0);
        let suffix = format_usage_suffix(&prev, &now, GIB + GIB / 2, 16 * GIB, 4);
        assert!(suffix.contains("RAM 1.5 GB"), "{}", suffix);
    }

    #[test]
    fn zero_elapsed_time_does_not_divide_by_zero() {
        let base = Instant::now();
        let prev = sample(base, 0, 0);
        let now = sample(base, 0, 0); // identical instant
        let suffix = format_usage_suffix(&prev, &now, 0, 0, 4);
        assert_eq!(suffix, "CPU 0% - RAM 0.0 GB - 0%");
    }
}
