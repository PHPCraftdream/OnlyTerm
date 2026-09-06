pub(crate) fn adapter() -> Option<wgpu::Adapter> {
    let instance = wgpu::Instance::default();
    adapter_from(
        &instance,
        std::env::var_os("ONLYTERM_REQUIRE_GPU_TESTS").as_deref()
            == Some(std::ffi::OsStr::new("1")),
    )
}

fn adapter_from(instance: &wgpu::Instance, required: bool) -> Option<wgpu::Adapter> {
    match futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    })) {
        Ok(adapter) => Some(adapter),
        Err(err) => {
            unavailable(required, &err.to_string());
            None
        }
    }
}

fn unavailable(required: bool, reason: &str) {
    assert!(
        !required,
        "GPU tests required but no adapter is available: {}",
        reason
    );
    eprintln!("SKIP GPU integration test: no adapter ({})", reason);
}

#[test]
fn headless_runner_can_report_missing_adapter() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::empty(),
        ..Default::default()
    });
    assert!(adapter_from(&instance, false).is_none());
}

#[test]
fn required_gpu_run_does_not_silently_skip() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::empty(),
        ..Default::default()
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        adapter_from(&instance, true)
    }))
    .expect_err("required GPU validation must fail on a headless backend");
    let message = failure.downcast_ref::<String>().expect("panic message");
    assert!(message.contains("GPU tests required but no adapter is available"));
}
