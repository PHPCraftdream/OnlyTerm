use crate::egl::ffi;
use anyhow::{anyhow, ensure, Error};

pub(super) struct EglWrapper {
    _lib: libloading::Library,
    pub(super) egl: ffi::Egl,
}

impl std::fmt::Debug for EglWrapper {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.debug_struct("EglWrapper").finish()
    }
}

type GetProcAddressFunc =
    unsafe extern "C" fn(*const std::os::raw::c_char) -> *const std::os::raw::c_void;

impl EglWrapper {
    pub(super) fn load_egl(lib: libloading::Library) -> anyhow::Result<Self> {
        let get_proc_address: libloading::Symbol<GetProcAddressFunc> =
            // SAFETY: `b"eglGetProcAddress\0"` is a valid null-terminated symbol name.
            unsafe { lib.get(b"eglGetProcAddress\0")? };
        let egl = ffi::Egl::load_with(|s: &'static str| {
            let sym_name = std::ffi::CString::new(s).expect("symbol to be cstring compatible");
            // SAFETY: `sym_name` is a live null-terminated C string; `_lib` outlives
            // the returned function pointers (kept alive by `EglWrapper`).
            if let Ok(sym) = unsafe { lib.get(sym_name.as_bytes_with_nul()) } {
                return *sym;
            }
            // SAFETY: valid null-terminated name for the loaded `eglGetProcAddress`.
            unsafe { get_proc_address(sym_name.as_ptr()) }
        });
        log::trace!("load_egl: {:?}", lib);
        Ok(Self { _lib: lib, egl })
    }

    pub(super) fn get_display(
        &self,
        display: Option<ffi::EGLNativeDisplayType>,
    ) -> anyhow::Result<ffi::types::EGLDisplay> {
        // SAFETY: `DEFAULT_DISPLAY` (or the provided display) is a valid EGL
        // native display argument; the call returns a display handle or null.
        let display = unsafe { self.egl.GetDisplay(display.unwrap_or(ffi::DEFAULT_DISPLAY)) };
        if display.is_null() {
            Err(self.error("egl GetDisplay"))
        } else {
            Ok(display)
        }
    }

    pub(super) fn error(&self, context: &str) -> Error {
        let label =
            // SAFETY: `GetError` takes no arguments and is always safe to call.
            match unsafe { self.egl.GetError() } as u32 {
            ffi::NOT_INITIALIZED => "NOT_INITIALIZED".into(),
            ffi::BAD_ACCESS => "BAD_ACCESS".into(),
            ffi::BAD_ALLOC => "BAD_ALLOC".into(),
            ffi::BAD_ATTRIBUTE => "BAD_ATTRIBUTE".into(),
            ffi::BAD_CONTEXT => "BAD_CONTEXT".into(),
            ffi::BAD_CURRENT_SURFACE => "BAD_CURRENT_SURFACE".into(),
            ffi::BAD_DISPLAY => "BAD_DISPLAY".into(),
            ffi::BAD_SURFACE => "BAD_SURFACE".into(),
            ffi::BAD_MATCH => "BAD_MATCH".into(),
            ffi::BAD_PARAMETER => "BAD_PARAMETER".into(),
            ffi::BAD_NATIVE_PIXMAP => "BAD_NATIVE_PIXMAP".into(),
            ffi::BAD_NATIVE_WINDOW => "BAD_NATIVE_WINDOW".into(),
            ffi::CONTEXT_LOST => "CONTEXT_LOST".into(),
            ffi::SUCCESS => "Failed but with error code: SUCCESS".into(),
            err => format!("EGL Error code: {}", err),
        };
        anyhow!("{}: {}", context, label)
    }

    pub(super) fn initialize_and_get_version(
        &self,
        display: ffi::types::EGLDisplay,
    ) -> anyhow::Result<(ffi::EGLint, ffi::EGLint)> {
        let mut major = 0;
        let mut minor = 0;
        // SAFETY: `display` is a valid EGL display; `major`/`minor` are valid
        // out-pointers the call writes to.
        unsafe {
            if self.egl.Initialize(display, &mut major, &mut minor) != 0 {
                Ok((major, minor))
            } else {
                Err(self.error("egl Initialize"))
            }
        }
    }

    fn config_attrib(
        &self,
        display: ffi::types::EGLDisplay,
        config: ffi::types::EGLConfig,
        attribute: u32,
    ) -> Option<ffi::EGLint> {
        let mut value = 0;
        // SAFETY: `display`/`config` are valid and `value` is a valid out-pointer.
        let res = unsafe {
            self.egl
                .GetConfigAttrib(display, config, attribute as ffi::EGLint, &mut value)
        };
        if res == 1 {
            Some(value)
        } else {
            None
        }
    }

    pub(super) fn log_config_info(
        &self,
        display: ffi::types::EGLDisplay,
        config: ffi::types::EGLConfig,
    ) {
        #[derive(Debug)]
        #[allow(dead_code)]
        struct ConfigInfo {
            config: ffi::types::EGLConfig,
            alpha_size: Option<ffi::EGLint>,
            red_size: Option<ffi::EGLint>,
            green_size: Option<ffi::EGLint>,
            blue_size: Option<ffi::EGLint>,
            depth_size: Option<ffi::EGLint>,
            conformant: Option<String>,
            renderable_type: Option<String>,
            native_visual_id: Option<ffi::EGLint>,
            surface_type: Option<String>,
        }

        fn conformant_bits(bits: ffi::EGLint) -> String {
            let bits = bits as ffi::types::EGLenum;
            let mut s = String::new();
            if bits & ffi::OPENGL_BIT != 0 {
                s.push_str("OPENGL ");
            }
            if bits & ffi::OPENGL_ES2_BIT != 0 {
                s.push_str("OPENGL_ES2 ");
            }
            if bits & ffi::OPENGL_ES3_BIT != 0 {
                s.push_str("OPENGL_ES3 ");
            }
            if bits & ffi::OPENVG_BIT != 0 {
                s.push_str("OPENVG_BIT ");
            }
            s
        }

        fn surface_bits(bits: ffi::EGLint) -> String {
            let bits = bits as ffi::types::EGLenum;
            let mut s = String::new();
            if bits & ffi::PBUFFER_BIT != 0 {
                s.push_str("PBUFFER ");
            }
            if bits & ffi::PIXMAP_BIT != 0 {
                s.push_str("PIXMAP ");
            }
            if bits & ffi::WINDOW_BIT != 0 {
                s.push_str("WINDOW ");
            }
            s
        }

        let info = ConfigInfo {
            config,
            alpha_size: self.config_attrib(display, config, ffi::ALPHA_SIZE),
            red_size: self.config_attrib(display, config, ffi::RED_SIZE),
            green_size: self.config_attrib(display, config, ffi::GREEN_SIZE),
            blue_size: self.config_attrib(display, config, ffi::BLUE_SIZE),
            depth_size: self.config_attrib(display, config, ffi::DEPTH_SIZE),
            conformant: self
                .config_attrib(display, config, ffi::CONFORMANT)
                .map(conformant_bits),
            renderable_type: self
                .config_attrib(display, config, ffi::RENDERABLE_TYPE)
                .map(conformant_bits),
            native_visual_id: self.config_attrib(display, config, ffi::NATIVE_VISUAL_ID),
            surface_type: self
                .config_attrib(display, config, ffi::SURFACE_TYPE)
                .map(surface_bits),
        };

        log::trace!("{:x?}", info);
    }

    pub(super) fn choose_config(
        &self,
        display: ffi::types::EGLDisplay,
        attributes: &[u32],
    ) -> anyhow::Result<Vec<ffi::types::EGLConfig>> {
        ensure!(
            !attributes.is_empty() && attributes[attributes.len() - 1] == ffi::NONE,
            "attributes list must be terminated with ffi::NONE"
        );

        let mut num_configs = 0;
        // SAFETY: `display` is valid; a null buffer with capacity 0 queries the
        // config count into `num_configs` without writing.
        if unsafe {
            self.egl
                .GetConfigs(display, std::ptr::null_mut(), 0, &mut num_configs)
        } != 1
        {
            return Err(self.error("egl GetConfigs to count possible number of configurations"));
        }

        let mut configs = vec![std::ptr::null(); num_configs as usize];

        // SAFETY: `configs` holds `num_configs` slots and the call writes at most
        // that many into it.
        if unsafe {
            self.egl
                .GetConfigs(display, configs.as_mut_ptr(), num_configs, &mut num_configs)
        } != 1
        {
            return Err(self.error("egl GetConfigs to enumerate configurations"));
        }

        log::trace!("Available Configuration(s):");
        for c in &configs {
            self.log_config_info(display, *c);
        }

        // SAFETY: `display` is valid, `attributes` is NONE-terminated (checked
        // above), and `configs`/`num_configs` are valid out-params.
        if unsafe {
            self.egl.ChooseConfig(
                display,
                attributes.as_ptr() as *const ffi::EGLint,
                configs.as_mut_ptr(),
                configs.len() as ffi::EGLint,
                &mut num_configs,
            )
        } != 1
        {
            return Err(self.error("egl ChooseConfig to select configurations"));
        }

        configs.resize(num_configs as usize, std::ptr::null());

        log::trace!("Matching Configuration(s):");
        for c in &configs {
            self.log_config_info(display, *c);
        }

        // If we're running on a system with 30bpp color depth then ChooseConfig
        // will bias towards putting 10bpc matches first, but we want 8-bit.
        // Let's filter out matches that are too deep
        configs.retain(|config| {
            self.config_attrib(display, *config, ffi::RED_SIZE) == Some(8)
                && self.config_attrib(display, *config, ffi::GREEN_SIZE) == Some(8)
                && self.config_attrib(display, *config, ffi::BLUE_SIZE) == Some(8)
        });

        // Sort by descending alpha size, otherwise we can end up selecting
        // alpha size 0 under XWayland, even though a superior config with
        // 32bpp 8bpc is available.  For whatever reason (probably a Wayland/mutter
        // weirdness) that renders with a transparent background on my pixelbook.
        configs.sort_by(|a, b| {
            self.config_attrib(display, *a, ffi::ALPHA_SIZE)
                .cmp(&self.config_attrib(display, *b, ffi::ALPHA_SIZE))
                .reverse()
        });

        log::trace!("Filtered down to these configuration(s):");
        for c in &configs {
            self.log_config_info(display, *c);
        }

        Ok(configs)
    }

    pub(super) fn create_window_surface(
        &self,
        display: ffi::types::EGLDisplay,
        config: ffi::types::EGLConfig,
        window: ffi::EGLNativeWindowType,
    ) -> anyhow::Result<ffi::types::EGLSurface> {
        // SAFETY: `display`/`config` are valid and `window` is a valid native
        // window handle; the colorspace attribute array is NONE-terminated.
        let surface = unsafe {
            self.egl.CreateWindowSurface(
                display,
                config,
                window,
                [
                    ffi::GL_COLORSPACE as i32,
                    ffi::GL_COLORSPACE_SRGB as i32,
                    ffi::NONE as i32,
                ]
                .as_ptr(),
            )
        };
        if surface.is_null() {
            Err(self.error("EGL CreateWindowSurface"))
        } else {
            Ok(surface)
        }
    }

    pub(super) fn create_context(
        &self,
        display: ffi::types::EGLDisplay,
        config: ffi::types::EGLConfig,
        share_context: ffi::types::EGLContext,
        attributes: &[u32],
    ) -> anyhow::Result<ffi::types::EGLConfig> {
        ensure!(
            !attributes.is_empty() && attributes[attributes.len() - 1] == ffi::NONE,
            "attributes list must be terminated with ffi::NONE"
        );
        // SAFETY: `display`/`config`/`share_context` are valid and `attributes`
        // is NONE-terminated (checked above).
        let context = unsafe {
            self.egl.CreateContext(
                display,
                config,
                share_context,
                attributes.as_ptr() as *const i32,
            )
        };
        if context.is_null() {
            Err(self.error("EGL CreateContext"))
        } else {
            Ok(context)
        }
    }
}
