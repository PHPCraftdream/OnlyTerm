//! Memory counters for the per-PID log, shared by the GUI process and the
//! `--gpu-tab-host` child so both report the same fields the same way.
//!
//! Each hosted window's GPU device and GPU memory live in its child; the GUI
//! process creates a device only for in-process fallback windows.

use crate::WebGpuState;

pub const MIB: u64 = 1024 * 1024;

/// Working set and committed private bytes of this process.
#[cfg(windows)]
pub fn process_memory() -> (u64, u64) {
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
pub fn process_memory() -> (u64, u64) {
    (0, 0)
}

/// The adapter a device lives on and what its sub-allocator holds.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DeviceMemory {
    /// "name (type, backend)".
    pub adapter: String,
    /// Video memory blocks reserved / used by the sub-allocator; `None` when
    /// the backend does not report them.
    pub blocks_reserved: Option<u64>,
    pub blocks_used: Option<u64>,
}

impl DeviceMemory {
    pub fn of(state: &WebGpuState) -> Self {
        let info = state.adapter_info();
        let report = state.device().generate_allocator_report();
        Self {
            adapter: format!("{} ({:?}, {:?})", info.name, info.device_type, info.backend),
            blocks_reserved: report.as_ref().map(|r| r.total_reserved_bytes),
            blocks_used: report.as_ref().map(|r| r.total_allocated_bytes),
        }
    }
    pub fn from_host_adapter(info: &crate::wire::WireAdapterInfo) -> Self {
        Self {
            adapter: format!("{} ({}, {})", info.name, info.device_type, info.backend),
            blocks_reserved: None,
            blocks_used: None,
        }
    }
}

/// `None` is a window with no device (yet).
pub fn format_device_memory(device: Option<&DeviceMemory>) -> String {
    let Some(device) = device else {
        return "gpu_adapter=\"n/a\" gpu_blocks=n/a".to_string();
    };
    match (device.blocks_reserved, device.blocks_used) {
        (Some(reserved), Some(used)) => format!(
            "gpu_adapter=\"{}\" gpu_blocks_reserved={} MiB gpu_blocks_used={} MiB",
            device.adapter,
            reserved / MIB,
            used / MIB
        ),
        _ => format!("gpu_adapter=\"{}\" gpu_blocks=n/a", device.adapter),
    }
}

/// What one `--gpu-tab-host` report line is made of.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct HostMemorySnapshot {
    pub working_set: u64,
    pub private_bytes: u64,
    pub device: Option<DeviceMemory>,
    /// Size of the mirrored glyph atlas; `None` before the first frame.
    pub atlas: Option<(usize, usize)>,
    pub instance_slots: usize,
    pub instance_bytes: u64,
}

pub(crate) fn format_host_memory_report(s: &HostMemorySnapshot) -> String {
    let atlas = match s.atlas {
        Some((width, height)) => format!("{}x{}", width, height),
        None => "n/a".to_string(),
    };
    format!(
        "gpu-tab-host: memory: working_set={} MiB private={} MiB {} atlas={} \
         instance_buffers={} ({} KiB)",
        s.working_set / MIB,
        s.private_bytes / MIB,
        format_device_memory(s.device.as_ref()),
        atlas,
        s.instance_slots,
        s.instance_bytes / 1024,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(reserved: Option<u64>, used: Option<u64>) -> DeviceMemory {
        DeviceMemory {
            adapter: "Intel(R) UHD Graphics (IntegratedGpu, Dx12)".to_string(),
            blocks_reserved: reserved,
            blocks_used: used,
        }
    }

    #[test]
    fn device_line_shows_adapter_and_block_sizes_in_mib() {
        assert_eq!(
            format_device_memory(Some(&device(Some(24 * MIB), Some(5 * MIB)))),
            "gpu_adapter=\"Intel(R) UHD Graphics (IntegratedGpu, Dx12)\" \
             gpu_blocks_reserved=24 MiB gpu_blocks_used=5 MiB"
        );
    }

    #[test]
    fn device_line_keeps_the_adapter_when_the_backend_reports_no_blocks() {
        assert_eq!(
            format_device_memory(Some(&device(None, None))),
            "gpu_adapter=\"Intel(R) UHD Graphics (IntegratedGpu, Dx12)\" gpu_blocks=n/a"
        );
    }

    #[test]
    fn device_line_says_so_without_a_device() {
        assert_eq!(
            format_device_memory(None),
            "gpu_adapter=\"n/a\" gpu_blocks=n/a"
        );
    }

    #[test]
    fn host_report_line_has_process_device_atlas_and_instance_buffers() {
        let line = format_host_memory_report(&HostMemorySnapshot {
            working_set: 12 * MIB,
            private_bytes: 508 * MIB,
            device: Some(device(Some(24 * MIB), Some(5 * MIB))),
            atlas: Some((2048, 2048)),
            instance_slots: 3,
            instance_bytes: 3 * 1024 * 1024 + 512,
        });
        assert_eq!(
            line,
            "gpu-tab-host: memory: working_set=12 MiB private=508 MiB \
             gpu_adapter=\"Intel(R) UHD Graphics (IntegratedGpu, Dx12)\" \
             gpu_blocks_reserved=24 MiB gpu_blocks_used=5 MiB atlas=2048x2048 \
             instance_buffers=3 (3072 KiB)"
        );
    }

    #[test]
    fn host_report_line_before_the_first_frame_has_no_atlas() {
        let line = format_host_memory_report(&HostMemorySnapshot::default());
        assert!(line.contains("atlas=n/a"), "{}", line);
        assert!(line.contains("gpu_blocks=n/a"), "{}", line);
        assert!(line.contains("instance_buffers=0 (0 KiB)"), "{}", line);
    }

    #[cfg(windows)]
    #[test]
    fn this_process_reports_nonzero_memory() {
        let (working_set, private_bytes) = process_memory();
        assert!(working_set > 0, "working set {}", working_set);
        assert!(private_bytes > 0, "private bytes {}", private_bytes);
    }
}
