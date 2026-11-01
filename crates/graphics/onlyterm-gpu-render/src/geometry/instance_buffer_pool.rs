//! Reusable instance buffers for the out-of-process GPU renderer.
//!
//! A wire frame contains one instance slice per draw.  The child process used
//! to create a new device-local buffer for every slice of every frame.  The
//! pool below keeps one `wgpu::Buffer` per draw slot and updates it with an
//! ordered `Queue::write_buffer` operation instead.  `wgpu::Buffer` is a
//! handle, so cloning it for the frame does not duplicate the allocation.

use std::cmp;
use std::convert::TryFrom;

const MIN_BUFFER_SIZE: u64 = 4;
const CAPACITY_HEADROOM_DIVISOR: u64 = 8;
const CAPACITY_ALIGNMENT: u64 = 4;

/// Buffers retained by one child GPU process.
pub(crate) struct InstanceBufferPool {
    core: BufferPoolCore,
    slots: Vec<InstanceBufferSlot>,
}

struct InstanceBufferSlot {
    buffer: wgpu::Buffer,
}

/// Device-independent capacity bookkeeping shared by the real GPU pool and
/// its deterministic tests.  Keeping this policy separate makes the tests
/// observe the same grow/shrink/truncate transitions as production.
#[derive(Default)]
struct BufferPoolCore {
    capacities: Vec<u64>,
}

impl BufferPoolCore {
    fn begin_frame(&mut self, draw_count: usize) {
        self.capacities.truncate(draw_count);
    }

    fn prepare_slot(&mut self, slot: usize, required: u64, max_capacity: u64) -> (u64, bool) {
        if self.capacities.len() <= slot {
            self.capacities.resize(slot + 1, 0);
        }
        let target = target_capacity(self.capacities[slot], required, max_capacity);
        let changed = target != self.capacities[slot];
        self.capacities[slot] = target;
        (target, changed)
    }

    fn total_bytes(&self) -> u64 {
        self.capacities.iter().sum()
    }

    #[cfg(test)]
    fn slot_count(&self) -> usize {
        self.capacities.len()
    }
}

impl InstanceBufferPool {
    pub(crate) fn new() -> Self {
        Self {
            core: BufferPoolCore::default(),
            slots: Vec::new(),
        }
    }

    /// Slots alive and the bytes of video memory their buffers were created
    /// with, for the memory log.
    pub(crate) fn usage(&self) -> (usize, u64) {
        (self.slots.len(), self.core.total_bytes())
    }

    /// Releases slots that are no longer used by this frame.
    ///
    /// A very large transient frame therefore does not keep an unbounded
    /// number of buffers alive when the next frame has fewer draw calls.  A
    /// slot that remains in use keeps its capacity so a stationary frame does
    /// not create a buffer after warm-up.
    pub(crate) fn begin_frame(&mut self, draw_count: usize) {
        self.core.begin_frame(draw_count);
        self.slots.truncate(draw_count);
    }

    /// Returns a clone of the buffer handle for `slot`, growing or shrinking
    /// that slot only when its capacity policy requires it.
    pub(crate) fn buffer_for(
        &mut self,
        device: &wgpu::Device,
        slot: usize,
        required_bytes: usize,
    ) -> anyhow::Result<wgpu::Buffer> {
        // The wire builder supplies contiguous slots. Reject a gap before
        // mutating the capacity bookkeeper, rather than indexing a missing
        // GPU slot if a future caller breaks that contract.
        if slot > self.slots.len() {
            anyhow::bail!("instance buffer slots must be requested contiguously");
        }
        let required = required_capacity(required_bytes, device.limits().max_buffer_size)?;
        let (target, recreate) =
            self.core
                .prepare_slot(slot, required, device.limits().max_buffer_size);
        let has_slot = self.slots.len() > slot;
        if !has_slot {
            self.slots.push(InstanceBufferSlot {
                buffer: create_buffer(device, target)?,
            });
        }

        let slot_state = &mut self.slots[slot];
        if has_slot && recreate {
            slot_state.buffer = create_buffer(device, target)?;
        }
        Ok(slot_state.buffer.clone())
    }
}

fn create_buffer(device: &wgpu::Device, capacity: u64) -> anyhow::Result<wgpu::Buffer> {
    if capacity > device.limits().max_buffer_size {
        anyhow::bail!(
            "instance buffer requires {capacity} bytes, but this device supports at most {}",
            device.limits().max_buffer_size
        );
    }
    Ok(device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu-tab-host reusable instance buffer"),
        size: capacity,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    }))
}

fn required_capacity(required_bytes: usize, max_capacity: u64) -> anyhow::Result<u64> {
    let required = u64::try_from(required_bytes)
        .map_err(|_| anyhow::anyhow!("instance buffer size does not fit in wgpu::BufferAddress"))?;
    let required = cmp::max(required, MIN_BUFFER_SIZE);
    if required > max_capacity {
        anyhow::bail!(
            "instance data requires {required} bytes, but this device supports at most {max_capacity}"
        );
    }
    Ok(required)
}

// New allocations stay within 1.125x demand plus 3 bytes; retained capacity stays within 1.5x demand.
// Tiny requests may exceed the ratio due to alignment. The 12.5% headroom amortizes incremental
// growth, while a separate shrink threshold avoids churn from small oscillations.
fn allocation_capacity(required: u64, max_capacity: u64) -> u64 {
    let with_headroom = required.saturating_add(required / CAPACITY_HEADROOM_DIVISOR);
    let aligned = with_headroom
        .checked_add(CAPACITY_ALIGNMENT - 1)
        .map(|value| value / CAPACITY_ALIGNMENT * CAPACITY_ALIGNMENT)
        .unwrap_or(u64::MAX);
    aligned.min(max_capacity).max(required.min(max_capacity))
}

fn target_capacity(current: u64, required: u64, max_capacity: u64) -> u64 {
    if current == 0 {
        return allocation_capacity(required, max_capacity);
    }
    if required > current {
        return allocation_capacity(required, max_capacity);
    }
    // Retain up to 1.5x raw demand; larger slack is released on the next request.
    let shrink_threshold = required.saturating_add(required / 2);
    if current > shrink_threshold {
        return allocation_capacity(required, max_capacity);
    }
    current
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use super::{
        allocation_capacity, required_capacity, target_capacity, BufferPoolCore,
        CAPACITY_ALIGNMENT, MIN_BUFFER_SIZE,
    };

    #[test]
    fn capacity_uses_bounded_headroom_and_alignment() {
        assert_eq!(required_capacity(0, 1024).unwrap(), MIN_BUFFER_SIZE);
        assert_eq!(required_capacity(4, 1024).unwrap(), 4);
        assert_eq!(required_capacity(5, 1024).unwrap(), 5);
        assert_eq!(allocation_capacity(64, 1024), 72);
        assert_eq!(required_capacity(8, 1024).unwrap(), 8);
        assert_eq!(required_capacity(9, 1024).unwrap(), 9);
        assert_eq!(required_capacity(300, 300).unwrap(), 300);
        assert!(required_capacity(301, 300).is_err());
    }

    #[test]
    fn capacity_never_exceeds_device_limit() {
        assert_eq!(required_capacity(300, 300).unwrap(), 300);
        assert_eq!(
            required_capacity(301, 300).unwrap_err().to_string(),
            "instance data requires 301 bytes, but this device supports at most 300"
        );
        assert!(required_capacity(0, 3).is_err());

        let mut core = BufferPoolCore::default();
        assert_eq!(prepare_frame(&mut core, &[300], 300), 1);
        assert_eq!(core.total_bytes(), 300);
        assert_eq!(prepare_frame(&mut core, &[300], 300), 0);
    }

    #[test]
    fn rounding_slack_does_not_recreate_during_slow_growth() {
        let mut core = BufferPoolCore::default();
        let mut recreations = 0;
        for size in 1024..=8192 {
            let raw = required_capacity(size, 16_384).unwrap();
            recreations += usize::from(core.prepare_slot(0, raw, 16_384).1);
        }
        assert!(recreations > 0);
        assert!(recreations < 32);
        let warmed = core.capacities[0];
        for _ in 0..100 {
            let raw = required_capacity(8192, 16_384).unwrap();
            assert_eq!(core.prepare_slot(0, raw, 16_384), (warmed, false));
        }
    }

    #[test]
    fn small_oscillation_keeps_capacity_until_shrink_threshold() {
        let mut core = BufferPoolCore::default();
        let large = required_capacity(400, 1024).unwrap();
        assert_eq!(
            core.prepare_slot(0, large, 1024),
            (allocation_capacity(large, 1024), true)
        );
        let warmed = core.capacities[0];
        for _ in 0..100 {
            for size in [390, 410, 395, 405] {
                let required = required_capacity(size, 1024).unwrap();
                assert_eq!(core.prepare_slot(0, required, 1024), (warmed, false));
            }
        }
    }

    #[test]
    fn bounded_waste_for_fresh_and_retained_capacities() {
        for required_bytes in 16..=65_536 {
            let raw = required_capacity(required_bytes, 1_000_000).unwrap();
            let mut core = BufferPoolCore::default();
            let (capacity, _) = core.prepare_slot(0, raw, 1_000_000);
            assert!(capacity >= raw);
            assert!(capacity <= 1_000_000);
            assert!(
                capacity
                    <= raw
                        .saturating_add(raw / 8)
                        .saturating_add(CAPACITY_ALIGNMENT - 1)
            );
        }

        for required_bytes in 4..16 {
            let raw = required_capacity(required_bytes, 1_000_000).unwrap();
            let (capacity, _) = BufferPoolCore::default().prepare_slot(0, raw, 1_000_000);
            assert!(capacity >= raw);
            assert!(capacity <= raw.saturating_add(raw / 8).saturating_add(3));
        }

        let mut core = BufferPoolCore::default();
        core.prepare_slot(0, 65_536, 1_000_000);
        for required_bytes in (16..=65_536).rev() {
            let raw = required_capacity(required_bytes, 1_000_000).unwrap();
            let (capacity, _) = core.prepare_slot(0, raw, 1_000_000);
            assert!(capacity >= raw);
            assert!(capacity <= 1_000_000);
            assert!(capacity <= raw.saturating_add(raw / 2));
        }
    }

    #[test]
    fn capacity_handles_overflow_and_unaligned_device_caps() {
        let max_request = required_capacity(usize::MAX, u64::MAX).unwrap();
        assert_eq!(max_request, u64::try_from(usize::MAX).unwrap());
        assert_eq!(required_capacity(296, 301).unwrap(), 296);
        assert!(required_capacity(302, 301).is_err());
        assert_eq!(allocation_capacity(296, 301), 301);
        assert_eq!(allocation_capacity(u64::MAX, u64::MAX), u64::MAX);
        assert_eq!(target_capacity(0, u64::MAX, u64::MAX), u64::MAX);
        assert_eq!(
            target_capacity(u64::MAX, u64::MAX / 2, u64::MAX),
            allocation_capacity(u64::MAX / 2, u64::MAX)
        );
        assert_eq!(target_capacity(u64::MAX, 1, u64::MAX), 4);
        assert_eq!(target_capacity(150, 100, 1_000), 150);
        assert_eq!(target_capacity(151, 100, 1_000), 112);
    }

    #[test]
    fn stable_size_does_not_recreate_or_shrink() {
        let mut core = BufferPoolCore::default();
        assert_eq!(core.prepare_slot(0, 1024, 2048), (1152, true));
        assert_eq!(core.prepare_slot(0, 1024, 2048), (1152, false));
        assert_eq!(core.prepare_slot(0, 800, 2048), (1152, false));
        assert_eq!(core.prepare_slot(0, 700, 2048), (788, true));
    }

    #[test]
    fn minimum_size_remains_stable_without_alignment_churn() {
        let mut core = BufferPoolCore::default();
        assert_eq!(prepare_frame(&mut core, &[0, 1, 4], 16), 3);
        assert_eq!(core.total_bytes(), 12);
        for _ in 0..100 {
            assert_eq!(prepare_frame(&mut core, &[0, 1, 4], 16), 0);
            assert_eq!(core.total_bytes(), 12);
        }
    }

    #[test]
    fn a_rare_large_buffer_is_released_after_the_frame_shrinks() {
        let mut core = BufferPoolCore::default();
        assert_eq!(
            core.prepare_slot(0, 16 * 1024 * 1024, 32 * 1024 * 1024),
            (18 * 1024 * 1024, true)
        );
        assert_eq!(core.prepare_slot(0, 4096, 32 * 1024 * 1024), (4608, true));
    }

    #[test]
    fn stationary_frames_have_zero_allocations_after_warmup() {
        let mut core = BufferPoolCore::default();
        assert_eq!(prepare_frame(&mut core, &[64, 128], 1024), 2);
        for _ in 0..100 {
            assert_eq!(prepare_frame(&mut core, &[64, 128], 1024), 0);
        }
    }

    #[test]
    fn total_bytes_follows_slot_capacities_through_growth_and_truncation() {
        let mut core = BufferPoolCore::default();
        assert_eq!(core.total_bytes(), 0);
        assert_eq!(prepare_frame(&mut core, &[64, 128], 1024), 2);
        assert_eq!(core.total_bytes(), 72 + 144);
        assert_eq!(prepare_frame(&mut core, &[64, 200], 1024), 1);
        assert_eq!(core.total_bytes(), 72 + 228);
        assert_eq!(prepare_frame(&mut core, &[64], 1024), 0);
        assert_eq!(core.total_bytes(), 72);
    }

    #[test]
    fn empty_frames_drop_unused_slots() {
        let mut core = BufferPoolCore::default();
        assert_eq!(prepare_frame(&mut core, &[64, 128], 1024), 2);
        assert_eq!(prepare_frame(&mut core, &[], 1024), 0);
        assert_eq!(core.slot_count(), 0);
    }

    #[test]
    fn distinct_draw_slots_are_reused_independently() {
        let mut core = BufferPoolCore::default();
        assert_eq!(prepare_frame(&mut core, &[64, 128], 1024), 2);
        assert_eq!(prepare_frame(&mut core, &[128, 64], 1024), 2);
        assert_eq!(prepare_frame(&mut core, &[128, 64], 1024), 0);
        assert_eq!(prepare_frame(&mut core, &[140, 64], 1024), 0);
        assert_eq!(core.slot_count(), 2);
    }

    #[test]
    fn production_pool_reuses_distinct_buffers_and_releases_unused_slots() {
        let Some(adapter) = crate::test_gpu::adapter() else {
            return;
        };
        let (device, _queue) =
            futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits:
                    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                label: Some("instance buffer pool test"),
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            }))
            .expect("device creation");

        let mut pool = super::InstanceBufferPool::new();
        pool.begin_frame(2);
        assert!(pool.buffer_for(&device, 1, 64).is_err());
        let first = pool.buffer_for(&device, 0, 64).expect("first buffer");
        let second = pool.buffer_for(&device, 1, 128).expect("second buffer");
        assert_ne!(first, second, "different draw slots need different buffers");

        pool.begin_frame(2);
        let first_again = pool.buffer_for(&device, 0, 64).expect("reused buffer");
        let second_again = pool.buffer_for(&device, 1, 128).expect("reused buffer");
        assert_eq!(
            first, first_again,
            "stationary draw must not allocate again"
        );
        assert_eq!(
            second, second_again,
            "stationary draw must not allocate again"
        );

        assert!(pool.buffer_for(&device, 0, usize::MAX).is_err());
        assert_eq!(first, pool.buffer_for(&device, 0, 64).unwrap());
        let grown = pool.buffer_for(&device, 0, 1024).unwrap();
        assert_ne!(first, grown, "growth must replace the undersized buffer");
        assert!(grown.size() >= 1024);
        let shrunk = pool.buffer_for(&device, 0, 64).unwrap();
        assert_ne!(
            grown, shrunk,
            "a transient large allocation must be released"
        );
        assert_eq!(shrunk.size(), 72);

        pool.begin_frame(0);
        assert_eq!(pool.core.slot_count(), 0);
        assert!(pool.slots.is_empty());
        let after_empty = pool.buffer_for(&device, 0, 64).unwrap();
        assert_ne!(shrunk, after_empty);
    }

    fn prepare_frame(core: &mut BufferPoolCore, sizes: &[usize], max_capacity: u64) -> usize {
        core.begin_frame(sizes.len());
        sizes
            .iter()
            .enumerate()
            .map(|(slot, size)| {
                let required = required_capacity(*size, max_capacity).unwrap();
                let changed = core.prepare_slot(slot, required, max_capacity).1;
                usize::from(changed)
            })
            .sum()
    }
}
