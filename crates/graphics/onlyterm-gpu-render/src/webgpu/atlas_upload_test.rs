use super::*;

#[test]
fn mirrored_atlas_writes_do_not_touch_the_parent_gpu_texture() {
    let Some(adapter) = crate::test_gpu::adapter() else {
        return;
    };
    let (device, queue) =
        futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            label: Some("atlas upload routing test"),
            memory_hints: Default::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("GPU device");
    let queue = Arc::new(queue);
    for mirrored in [false, true] {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas upload routing test"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas = WebGpuTexture {
            texture,
            width: 1,
            height: 1,
            queue: Arc::clone(&queue),
            mirroring_enabled: std::sync::atomic::AtomicBool::new(false),
            mirror: parking_lot::Mutex::new(AtlasMirrorLog::default()),
        };
        if mirrored {
            atlas.enable_mirroring();
        }
        // Any accidental local upload now produces a real validation error.
        atlas.texture.destroy();
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pixels = window::Image::new(1, 1);
        atlas.write(
            window::Rect::new(window::Point::new(0, 0), window::Size::new(1, 1)),
            &pixels,
        );
        let error = futures::executor::block_on(device.pop_error_scope());
        assert_eq!(
            error.is_none(),
            mirrored,
            "upload must target only the renderer's device"
        );
        let updates = atlas.drain_dirty_updates();
        if mirrored {
            assert_eq!(updates.len(), 1);
            assert_eq!(&*updates[0].pixels, pixels.pixel_data_slice());
        } else {
            assert!(updates.is_empty());
        }
    }
}
