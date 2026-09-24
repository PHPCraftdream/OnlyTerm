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
const SHRINK_FACTOR: u64 = 4;

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

    fn prepare_slot(&mut self, slot: usize, required: u64) -> (u64, bool) {
        if self.capacities.len() <= slot {
            self.capacities.resize(slot + 1, 0);
        }
        let target = target_capacity(self.capacities[slot], required);
        let changed = target != self.capacities[slot];
        self.capacities[slot] = target;
        (target, changed)
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
        let (target, recreate) = self.core.prepare_slot(slot, required);
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
    Ok(required
        .checked_next_power_of_two()
        .unwrap_or(u64::MAX)
        .min(max_capacity))
}

fn target_capacity(current: u64, required: u64) -> u64 {
    if current == 0 {
        return required;
    }
    if required > current {
        return required;
    }
    if current > required.saturating_mul(SHRINK_FACTOR) {
        return required;
    }
    current
}

#[cfg(test)]
mod tests {
    use super::{required_capacity, BufferPoolCore, MIN_BUFFER_SIZE};

    #[test]
    fn capacity_grows_to_power_of_two() {
        assert_eq!(required_capacity(0, 1024).unwrap(), MIN_BUFFER_SIZE);
        assert_eq!(required_capacity(5, 1024).unwrap(), 8);
        assert_eq!(required_capacity(8, 1024).unwrap(), 8);
        assert_eq!(required_capacity(9, 1024).unwrap(), 16);
    }

    #[test]
    fn capacity_never_exceeds_device_limit() {
        assert_eq!(required_capacity(300, 300).unwrap(), 300);
        assert!(required_capacity(301, 300).is_err());
    }

    #[test]
    fn stable_size_does_not_recreate_or_shrink() {
        let mut core = BufferPoolCore::default();
        assert_eq!(core.prepare_slot(0, 1024), (1024, true));
        assert_eq!(core.prepare_slot(0, 1024), (1024, false));
        assert_eq!(core.prepare_slot(0, 800), (1024, false));
        assert_eq!(core.prepare_slot(0, 256), (1024, false));
    }

    #[test]
    fn a_rare_large_buffer_is_released_after_the_frame_shrinks() {
        let mut core = BufferPoolCore::default();
        assert_eq!(
            core.prepare_slot(0, 16 * 1024 * 1024),
            (16 * 1024 * 1024, true)
        );
        assert_eq!(core.prepare_slot(0, 4096), (4096, true));
    }

    #[test]
    fn stationary_frames_have_zero_allocations_after_warmup() {
        let mut core = BufferPoolCore::default();
        assert_eq!(prepare_frame(&mut core, &[64, 128], 1024), 2);
        assert_eq!(prepare_frame(&mut core, &[64, 128], 1024), 0);
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
        assert_eq!(prepare_frame(&mut core, &[128, 64], 1024), 1);
        assert_eq!(prepare_frame(&mut core, &[128, 64], 1024), 0);
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
        assert_eq!(shrunk.size(), 64);

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
                usize::from(core.prepare_slot(slot, required).1)
            })
            .sum()
    }
}
