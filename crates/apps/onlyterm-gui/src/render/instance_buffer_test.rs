use super::*;

fn test_device_queue() -> Option<(wgpu::Device, wgpu::Queue)> {
    let required = std::env::var_os("ONLYTERM_REQUIRE_GPU_TESTS").as_deref()
        == Some(std::ffi::OsStr::new("1"));
    let instance = wgpu::Instance::default();
    let adapter =
        match futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        })) {
            Ok(adapter) => adapter,
            Err(error) => {
                assert!(
                    !required,
                    "GPU tests required but no adapter is available: {}",
                    error
                );
                eprintln!("SKIP GPU integration test: no adapter ({})", error);
                return None;
            }
        };
    Some(
        futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            label: Some("instance buffer upload test"),
            memory_hints: Default::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("GPU device"),
    )
}

#[test]
fn write_instances_allocates_reuses_grows_and_handles_empty() {
    let Some((device, queue)) = test_device_queue() else {
        return;
    };
    let mut buffer = WebGpuInstanceBuffer::new(1);
    buffer.write_instances(&device, &queue, &[]);
    assert!(buffer.buffer().is_none());
    assert_eq!(buffer.used_instances(), 0);
    assert_eq!(buffer.allocation_count(), 0);
    let one = [crate::quad::QuadInstance::default()];
    buffer.write_instances(&device, &queue, &one);
    let _initial = buffer.buffer().expect("first upload allocates");
    assert_eq!(buffer.allocation_count(), 1);
    assert_eq!(buffer.used_instances(), 1);
    buffer.write_instances(&device, &queue, &one);
    assert_eq!(buffer.allocation_count(), 1);
    let larger = vec![crate::quad::QuadInstance::default(); 129];
    buffer.write_instances(&device, &queue, &larger);
    assert_eq!(buffer.allocation_count(), 2);
    assert!(buffer.capacity >= larger.len());
    assert_eq!(buffer.used_instances(), larger.len());
    let allocations_after_growth = buffer.allocation_count();
    buffer.write_instances(&device, &queue, &[]);
    assert_eq!(buffer.allocation_count(), allocations_after_growth);
    assert_eq!(buffer.used_instances(), 0);
}
