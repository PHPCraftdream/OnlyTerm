//! Periodic memory diagnostics, written to the per-PID log.
//!
//! A long-running process once held ~1 GiB of GPU blocks and ~700 MiB of
//! heap that could only be explained from a crash dump. One `info` line per
//! interval shows where the memory is without one.

use super::{TermWindow, TermWindowNotif};
use ::window::{Window, WindowOps};
use smol::Timer;
use std::time::{Duration, Instant};

const MIB: u64 = 1024 * 1024;

/// Bytes per `QuadInstance`, the unit of the CPU-side render caches.
const QUAD_BYTES: u64 = std::mem::size_of::<onlyterm_gpu_render::QuadInstance>() as u64;

/// What one report line is made of; plain numbers so formatting is testable.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct MemorySnapshot {
    pub working_set: u64,
    /// Committed private bytes (what Task Manager calls "Commit size").
    pub private_bytes: u64,
    /// Video memory blocks reserved / used by the device's sub-allocator;
    /// `None` when the backend does not report them.
    pub gpu_reserved: Option<u64>,
    pub gpu_allocated: Option<u64>,
    /// Side of the square glyph atlas, in pixels.
    pub atlas_side: usize,
    pub shape_cache: usize,
    pub line_to_ele_shape_cache: usize,
    pub shape_hash_cache: usize,
    pub line_quad_cache: usize,
    pub retained_panes: usize,
    pub retained_rows: usize,
    pub retained_quads: u64,
}

pub(super) fn format_memory_report(s: &MemorySnapshot) -> String {
    let gpu = match (s.gpu_reserved, s.gpu_allocated) {
        (Some(reserved), Some(allocated)) => format!(
            "gpu_blocks_reserved={} MiB gpu_blocks_used={} MiB",
            reserved / MIB,
            allocated / MIB
        ),
        _ => "gpu_blocks=n/a".to_string(),
    };
    let atlas_bytes = (s.atlas_side as u64) * (s.atlas_side as u64) * 4;
    format!(
        "memory: working_set={} MiB private={} MiB {} atlas={}px ({} MiB) \
         shape_cache={} line_to_ele_shape_cache={} shape_hash_cache={} \
         line_quad_cache={} retained_panes={} retained_rows={} \
         retained_quads={} ({} MiB)",
        s.working_set / MIB,
        s.private_bytes / MIB,
        gpu,
        s.atlas_side,
        atlas_bytes / MIB,
        s.shape_cache,
        s.line_to_ele_shape_cache,
        s.shape_hash_cache,
        s.line_quad_cache,
        s.retained_panes,
        s.retained_rows,
        s.retained_quads,
        s.retained_quads * QUAD_BYTES / MIB,
    )
}

/// Working set and committed private bytes of this process.
#[cfg(windows)]
fn process_memory() -> (u64, u64) {
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: `counters` is a live, writable PROCESS_MEMORY_COUNTERS_EX, whose
    // first member is a PROCESS_MEMORY_COUNTERS: the documented way to ask for
    // the extended form is to pass its address with `cb` set to its size. The
    // pseudo-handle from `GetCurrentProcess` needs no closing.
    let result = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    };
    match result {
        Ok(()) => (counters.WorkingSetSize as u64, counters.PrivateUsage as u64),
        Err(_) => (0, 0),
    }
}

#[cfg(not(windows))]
fn process_memory() -> (u64, u64) {
    (0, 0)
}

impl TermWindow {
    /// How often a window writes its memory line to the log.
    const MEMORY_REPORT_INTERVAL: Duration = Duration::from_secs(600);

    /// Arms the next report. Self-rearming like `schedule_process_usage_tick`
    /// and, like it, guarded so at most one chain is pending per window; it
    /// does not depend on focus, since it only reads counters.
    pub(super) fn schedule_memory_report(&self, window: &Window) {
        if self.memory_report_scheduled.get() {
            return;
        }
        self.memory_report_scheduled.set(true);

        let next = Instant::now() + Self::MEMORY_REPORT_INTERVAL;
        let window = window.clone();
        onlyterm_promise::spawn::spawn(async move {
            Timer::at(next).await;
            let win = window.clone();
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.memory_report_tick(&win);
            })));
        })
        .detach();
    }

    fn memory_report_tick(&mut self, window: &Window) {
        self.memory_report_scheduled.set(false);
        log::info!("{}", format_memory_report(&self.memory_snapshot()));
        self.schedule_memory_report(window);
    }

    fn memory_snapshot(&self) -> MemorySnapshot {
        let (working_set, private_bytes) = process_memory();
        let mut snapshot = MemorySnapshot {
            working_set,
            private_bytes,
            shape_cache: self.shape_cache.borrow().len(),
            line_to_ele_shape_cache: self.line_to_ele_shape_cache.borrow().len(),
            shape_hash_cache: self.shape_hash_cache.borrow().len(),
            line_quad_cache: self.line_quad_cache.borrow().len(),
            ..Default::default()
        };

        let retained = self.retained_rows.borrow();
        snapshot.retained_panes = retained.len();
        for pane in retained.values() {
            for row in pane.rows.iter().flatten() {
                snapshot.retained_rows += 1;
                snapshot.retained_quads += row.quads.quad_count() as u64;
            }
        }

        if let Some(render_state) = self.render_state.as_ref() {
            snapshot.atlas_side = render_state.glyph_cache.borrow().atlas.size();
            if let Some(report) = render_state.context.0.device().generate_allocator_report() {
                snapshot.gpu_reserved = Some(report.total_reserved_bytes);
                snapshot.gpu_allocated = Some(report.total_allocated_bytes);
            }
        }
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_line_shows_sizes_in_mib_and_every_cache() {
        let line = format_memory_report(&MemorySnapshot {
            working_set: 600 * MIB,
            private_bytes: 900 * MIB,
            gpu_reserved: Some(24 * MIB),
            gpu_allocated: Some(5 * MIB),
            atlas_side: 2048,
            shape_cache: 11,
            line_to_ele_shape_cache: 12,
            shape_hash_cache: 13,
            line_quad_cache: 14,
            retained_panes: 2,
            retained_rows: 100,
            retained_quads: 1_000_000,
        });
        assert_eq!(
            line,
            "memory: working_set=600 MiB private=900 MiB \
             gpu_blocks_reserved=24 MiB gpu_blocks_used=5 MiB atlas=2048px (16 MiB) \
             shape_cache=11 line_to_ele_shape_cache=12 shape_hash_cache=13 \
             line_quad_cache=14 retained_panes=2 retained_rows=100 \
             retained_quads=1000000 (80 MiB)"
        );
    }

    #[test]
    fn report_line_says_so_when_the_backend_reports_no_blocks() {
        let line = format_memory_report(&MemorySnapshot::default());
        assert!(line.contains("gpu_blocks=n/a"), "{}", line);
        assert!(!line.contains("gpu_blocks_reserved"), "{}", line);
    }

    #[test]
    fn quad_unit_is_the_wire_size() {
        assert_eq!(QUAD_BYTES, 84);
    }
}
