//! Regression tests for the video memory the process-wide device holds.
//!
//! With wgpu's default `MemoryHints::Performance`, gpu-allocator reserves
//! 256 MiB (GPU) and 64 MiB (host) blocks per memory type and never frees
//! the last one: a long-running process held about 1 GiB for ~30 MiB of data.

use super::context::device_descriptor;

const MIB: u64 = 1024 * 1024;

#[test]
fn process_device_asks_for_the_memory_usage_hint() {
    let descriptor = device_descriptor(wgpu::Limits::default());
    assert!(
        matches!(descriptor.memory_hints, wgpu::MemoryHints::MemoryUsage),
        "got {:?}",
        descriptor.memory_hints
    );
}

#[test]
fn process_device_reserves_small_video_memory_blocks() {
    let Some(adapter) = crate::test_gpu::adapter() else {
        return;
    };
    let (device, _queue) =
        futures::executor::block_on(adapter.request_device(&device_descriptor(adapter.limits())))
            .expect("GPU device");

    // What the renderer allocates first: an instance buffer and an atlas.
    let _buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("device memory test buffer"),
        size: 64 * 1024,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let _atlas = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("device memory test atlas"),
        size: wgpu::Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let Some(report) = device.generate_allocator_report() else {
        eprintln!("SKIP device memory test: this backend does not report sub-allocation");
        return;
    };
    if report.blocks.is_empty() {
        eprintln!("SKIP device memory test: this backend did not sub-allocate");
        return;
    }
    // Two 8 MiB GPU blocks (buffers, textures) plus a 4 MiB host block fit
    // well under this; a single default block (256 MiB) does not.
    assert!(
        report.total_reserved_bytes <= 64 * MIB,
        "reserved {} MiB of video memory blocks for {} KiB of resources",
        report.total_reserved_bytes / MIB,
        report.total_allocated_bytes / 1024
    );
}
