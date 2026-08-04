use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic, Default)]
pub enum FrontEndSelection {
    #[default]
    OpenGL,
    WebGpu,
    Software,
}

/// Corresponds to <https://docs.rs/wgpu/latest/wgpu/struct.AdapterInfo.html>
#[derive(Debug, Clone, FromDynamic, ToDynamic)]
pub struct GpuInfo {
    pub name: String,
    pub device_type: String,
    pub backend: String,
    pub driver: Option<String>,
    pub driver_info: Option<String>,
    pub vendor: Option<u32>,
    pub device: Option<u32>,
}

impl std::fmt::Display for GpuInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut result = format!(
            "name={}, device_type={}, backend={}",
            self.name, self.device_type, self.backend
        );
        if let Some(driver) = &self.driver {
            result.push_str(&format!(", driver={}", driver));
        }
        if let Some(driver_info) = &self.driver_info {
            result.push_str(&format!(", driver_info={}", driver_info));
        }
        if let Some(vendor) = &self.vendor {
            result.push_str(&format!(", vendor={}", vendor));
        }
        if let Some(device) = &self.device {
            result.push_str(&format!(", device={}", device));
        }
        write!(f, "{}", result)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic, Default)]
pub enum WebGpuPowerPreference {
    #[default]
    LowPower,
    HighPerformance,
}
