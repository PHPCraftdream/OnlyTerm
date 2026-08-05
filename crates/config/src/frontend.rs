use wezterm_dynamic::{FromDynamic, FromDynamicOptions, ToDynamic, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ToDynamic, Default)]
pub enum FrontEndSelection {
    #[default]
    WebGpu,
}

impl FromDynamic for FrontEndSelection {
    /// `OpenGL` and `Software` used to be selectable `front_end` backends,
    /// implemented on top of an OpenGL/Mesa renderer. That renderer has been
    /// removed; `WebGpu` is now the only backend. Rather than hard-failing
    /// config load for anyone with `front_end = "OpenGL"` or `front_end =
    /// "Software"` left over in their config (which would turn a working
    /// setup into a crash on upgrade), we map both of those legacy values
    /// onto `WebGpu` and emit a warning explaining what happened.
    fn from_dynamic(
        value: &Value,
        options: FromDynamicOptions,
    ) -> Result<Self, wezterm_dynamic::Error> {
        let s = String::from_dynamic(value, options)?;
        match s.as_str() {
            "WebGpu" => Ok(Self::WebGpu),
            "OpenGL" | "Software" => {
                let message = format!(
                    "front_end = \"{s}\" is no longer supported: the OpenGL \
                     renderer has been removed from OnlyTerm. Falling back to \
                     front_end = \"WebGpu\". Please update your config to \
                     remove this setting (or set it to \"WebGpu\") to \
                     silence this warning."
                );
                log::warn!("{message}");
                crate::show_error(&message);
                Ok(Self::WebGpu)
            }
            _ => Err(wezterm_dynamic::Error::Message(format!(
                "`{s}` is not a valid FrontEndSelection variant, use one of `WebGpu`"
            ))),
        }
    }
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

#[cfg(test)]
mod front_end_migration_test {
    use super::*;

    /// The current, still-supported value must continue to parse as itself.
    #[test]
    fn web_gpu_parses_as_web_gpu() {
        let value = Value::String("WebGpu".into());
        assert_eq!(
            FrontEndSelection::from_dynamic(&value, FromDynamicOptions::default()).unwrap(),
            FrontEndSelection::WebGpu
        );
    }

    /// Old configs may still say `front_end = "OpenGL"` (the historical
    /// default) or `front_end = "Software"` (the Mesa/SWRAST mode). Both
    /// backends have been removed; loading such a config must not error out
    /// (that would turn a previously-working setup into a hard failure on
    /// upgrade) and must instead transparently map onto `WebGpu`.
    #[test]
    fn legacy_open_gl_maps_to_web_gpu() {
        let value = Value::String("OpenGL".into());
        assert_eq!(
            FrontEndSelection::from_dynamic(&value, FromDynamicOptions::default()).unwrap(),
            FrontEndSelection::WebGpu
        );
    }

    /// Same migration path for the other removed GL-backed variant.
    #[test]
    fn legacy_software_maps_to_web_gpu() {
        let value = Value::String("Software".into());
        assert_eq!(
            FrontEndSelection::from_dynamic(&value, FromDynamicOptions::default()).unwrap(),
            FrontEndSelection::WebGpu
        );
    }

    /// Anything else is still a real error: we only want to special-case the
    /// two backends that actually used to exist, not silently accept
    /// arbitrary garbage.
    #[test]
    fn unknown_value_is_still_an_error() {
        let value = Value::String("NotARealBackend".into());
        assert!(FrontEndSelection::from_dynamic(&value, FromDynamicOptions::default()).is_err());
    }
}
