use crate::egl::connection::GlConnection;
use crate::egl::ffi;
use crate::egl::wrapper::EglWrapper;
use anyhow::{anyhow, bail};
use std::ffi::c_void;
use std::rc::Rc;

#[derive(Debug)]
pub struct GlState {
    connection: Rc<GlConnection>,
    surface: ffi::types::EGLSurface,
    context: ffi::types::EGLContext,
}

impl Drop for GlState {
    fn drop(&mut self) {
        // SAFETY: `display`/`surface`/`context` are the valid EGL objects owned
        // by this `GlState`; releasing the current context and destroying them
        // exactly once on drop.
        unsafe {
            self.connection.MakeCurrent(
                self.connection.display,
                ffi::NO_SURFACE,
                ffi::NO_SURFACE,
                ffi::NO_CONTEXT,
            );
            self.connection
                .DestroySurface(self.connection.display, self.surface);
            self.connection
                .DestroyContext(self.connection.display, self.context);
        }
    }
}

impl GlState {
    #[cfg_attr(any(windows, target_os = "macos"), allow(unused))]
    pub fn get_connection(&self) -> &Rc<GlConnection> {
        &self.connection
    }

    fn with_egl_lib<F: FnMut(EglWrapper) -> anyhow::Result<Self>>(
        mut func: F,
    ) -> anyhow::Result<Self> {
        let mut paths: Vec<std::path::PathBuf> = vec![
            #[cfg(target_os = "windows")]
            "libEGL.dll".into(),
            #[cfg(target_os = "windows")]
            "atioglxx.dll".into(),
            #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
            "libEGL.so.1".into(),
            #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
            "libEGL.so".into(),
        ];

        if cfg!(target_os = "macos") {
            // On macOS, let's look in the application directory to see
            // if we've deployed libEGL.dylib alongside; if so, we want
            // to try loading that.
            paths.push(
                std::env::current_exe()?
                    .parent()
                    .ok_or_else(|| anyhow!("current_exe isn't in a directory!?"))?
                    .join("libEGL.dylib"),
            );

            // And just in case, let's also allow loading via
            // DYLD_LIBRARY_PATH
            paths.push("libEGL.dylib".into());
        }

        let mut errors = vec![];
        let mut prefer_swrast = crate::configuration::prefer_swrast();

        for _ in 0..2 {
            if prefer_swrast {
                // Assuming that we're using Mesa, set an environment
                // variable that should select CPU based rendering.
                std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "true");
            }
            for path in &paths {
                // SAFETY: `path` is a valid filesystem path; `Library::new` loads it
                // and we discard it on failure. The loaded symbols outlive this call
                // only because `EglWrapper` keeps the `Library` alive.
                match unsafe { libloading::Library::new(path) } {
                    Ok(lib) => match EglWrapper::load_egl(lib) {
                        Ok(egl) => match func(egl) {
                            Ok(result) => {
                                return Ok(result);
                            }
                            Err(e) => {
                                errors.push(format!(
                                    "with_egl_lib({}) failed: {}",
                                    path.display(),
                                    e
                                ));
                            }
                        },
                        Err(e) => {
                            errors.push(format!("load_egl {} failed: {}", path.display(), e));
                        }
                    },
                    Err(e) => {
                        errors.push(format!("{}: {}", path.display(), e));
                    }
                }
            }
            // Since we didn't yet succeed, try enabling software rasterization.
            // However, don't do this on Windows; the EGL implementation on
            // Windows isn't MESA so there's no point trying a second pass
            // with the mesa environment set, and if we did, it would just
            // cause us to try software mode instead of the native opengl
            // drivers we'd pick up from the WGL fallback.
            if cfg!(windows) || cfg!(target_os = "macos") {
                break;
            }
            if prefer_swrast {
                break;
            }
            prefer_swrast = true;
        }
        bail!("with_egl_lib failed: {}", errors.join(", "))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[cfg(feature = "wayland")]
    pub fn create_wayland(
        display: Option<ffi::EGLNativeDisplayType>,
        wegl_surface: &wayland_egl::WlEglSurface,
    ) -> anyhow::Result<Self> {
        Self::create(display, wegl_surface.ptr())
    }

    pub fn create(
        display: Option<ffi::EGLNativeDisplayType>,
        window: ffi::EGLNativeWindowType,
    ) -> anyhow::Result<Self> {
        Self::with_egl_lib(|egl| {
            let egl_display = egl.get_display(display)?;

            let (major, minor) = egl.initialize_and_get_version(egl_display)?;
            log::trace!("initialized EGL version {}.{}", major, minor);

            let is_opengl =
                // SAFETY: `BindAPI` takes a valid API enum constant.
                unsafe {
                    if egl.egl.BindAPI(ffi::OPENGL_API) != 0 {
                    log::trace!("using OpenGL");
                    true
                } else if egl.egl.BindAPI(ffi::OPENGL_ES_API) != 0 {
                    log::trace!("using GLES");
                    false
                } else {
                    anyhow::bail!("Unable to bind to OpenGL or GL ES!?");
                }
            };

            // SAFETY: `egl_display` is valid and `EXTENSIONS` is a valid query name.
            let extensions = unsafe { egl.egl.QueryString(egl_display, ffi::EXTENSIONS as _) };
            let extensions = if extensions.is_null() {
                String::new()
            } else {
                // SAFETY: a non-null `QueryString(EXTENSIONS)` result is a NUL-terminated C string.
                let cstr = unsafe { std::ffi::CStr::from_ptr(extensions) };
                String::from_utf8_lossy(cstr.to_bytes()).to_string()
            };
            log::trace!("EGL extensions: {}", extensions);

            let connection = Rc::new(GlConnection {
                display: egl_display,
                egl,
                is_opengl,
                extensions,
            });

            Self::create_with_existing_connection(&connection, window)
        })
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[cfg(feature = "wayland")]
    pub fn create_wayland_with_existing_connection(
        connection: &Rc<GlConnection>,
        wegl_surface: &wayland_egl::WlEglSurface,
    ) -> anyhow::Result<Self> {
        Self::create_with_existing_connection(connection, wegl_surface.ptr())
    }

    pub fn create_with_existing_connection(
        connection: &Rc<GlConnection>,
        window: ffi::EGLNativeWindowType,
    ) -> anyhow::Result<GlState> {
        let configs = connection.egl.choose_config(
            connection.display,
            &[
                // We're explicitly asking for any alpha size; this is
                // the default behavior but we're making it explicit here
                // for the sake of documenting our intent.
                // In general we want 32bpp with 8bpc, but for displays
                // that are natively 10bpc we should be fine with relaxing
                // this to 0 alpha bits, so by asking for 0 here we effectively
                // indicate that we don't care.
                // In our implementation of choose_config we will return
                // only entries with 8bpc for red/green/blue so we should
                // end up with either 24bpp/8bpc with no alpha, or 32bpp/8bpc
                // with 8bpc alpha.
                ffi::ALPHA_SIZE,
                0,
                // Request at least 8bpc, 24bpp.  The implementation may
                // return a context capable of more than this.
                ffi::RED_SIZE,
                8,
                ffi::GREEN_SIZE,
                8,
                ffi::BLUE_SIZE,
                8,
                ffi::DEPTH_SIZE,
                24,
                ffi::CONFORMANT,
                if connection.is_opengl {
                    ffi::OPENGL_BIT
                } else {
                    ffi::OPENGL_ES3_BIT
                },
                ffi::RENDERABLE_TYPE,
                if connection.is_opengl {
                    ffi::OPENGL_BIT
                } else {
                    ffi::OPENGL_ES3_BIT
                },
                // Wayland EGL doesn't give us a working context if we request
                // PBUFFER|PIXMAP.  We don't appear to require these for X11,
                // so we're just asking for a WINDOW capable context
                ffi::SURFACE_TYPE,
                ffi::WINDOW_BIT, //| ffi::PBUFFER_BIT | ffi::PIXMAP_BIT,
                ffi::NONE,
            ],
        )?;

        if configs.is_empty() {
            anyhow::bail!("no compatible EGL configuration was found");
        }
        let mut errors = String::new();

        for config in configs {
            let surface =
                match connection
                    .egl
                    .create_window_surface(connection.display, config, window)
                {
                    Ok(s) => s,
                    Err(e) => {
                        errors.push_str(&format!("{:#} {:x?}\n", e, config));
                        continue;
                    }
                };

            let mut attributes = vec![ffi::CONTEXT_MAJOR_VERSION, 3];
            if cfg!(windows) {
                // On Windows, where drivers may be dynamically unloaded,
                // let's make an effort to try to survive that event.
                for &a in &[
                    ffi::CONTEXT_OPENGL_ROBUST_ACCESS_EXT,
                    ffi::TRUE,
                    ffi::CONTEXT_OPENGL_RESET_NOTIFICATION_STRATEGY_EXT,
                    ffi::LOSE_CONTEXT_ON_RESET_EXT,
                ] {
                    attributes.push(a);
                }
            }
            attributes.push(ffi::NONE);

            let context = match connection.egl.create_context(
                connection.display,
                config,
                std::ptr::null(),
                &attributes,
            ) {
                Ok(c) => c,
                Err(e) => {
                    errors.push_str(&format!("{:#} {:x?}\n", e, config));
                    continue;
                }
            };

            log::trace!("Successfully created a surface using this configuration");
            connection.egl.log_config_info(connection.display, config);

            // Request non-blocking buffer swaps; we'll manage throttling
            // frames at the application level.
            // SAFETY: `connection.display` is a valid EGL display; 0 requests
            // non-blocking swaps.
            unsafe {
                connection.egl.egl.SwapInterval(connection.display, 0);
            }

            return Ok(Self {
                connection: Rc::clone(connection),
                context,
                surface,
            });
        }

        Err(anyhow!(errors))
    }
}

// SAFETY: `GlState` owns a valid EGL display/surface/context triple and
// faithfully implements the glium `Backend` contract on the thread that owns
// the context.
unsafe impl glium::backend::Backend for GlState {
    fn resize(&self, _: (u32, u32)) {
        todo!()
    }

    fn swap_buffers(&self) -> Result<(), glium::SwapBuffersError> {
        // SAFETY: `display`/`surface` are the valid EGL objects owned by this state.
        let res = unsafe {
            self.connection
                .SwapBuffers(self.connection.display, self.surface)
        };
        if res != 1 {
            // SAFETY: `GetError` takes no arguments and is always safe to call.
            Err(match unsafe { self.connection.GetError() } as u32 {
                ffi::CONTEXT_LOST => glium::SwapBuffersError::ContextLost,
                _ => glium::SwapBuffersError::AlreadySwapped,
            })
        } else {
            Ok(())
        }
    }

    unsafe fn get_proc_address(&self, symbol: &str) -> *const c_void {
        let sym_name = std::ffi::CString::new(symbol).expect("symbol to be cstring compatible");
        self.connection.GetProcAddress(sym_name.as_ptr()) as *const c_void
    }

    fn get_framebuffer_dimensions(&self) -> (u32, u32) {
        let mut width = 0;
        let mut height = 0;

        // SAFETY: `display`/`surface` are valid and `width` is a valid out-pointer.
        unsafe {
            self.connection.QuerySurface(
                self.connection.display,
                self.surface,
                ffi::WIDTH as i32,
                &mut width,
            );
        }
        // SAFETY: same as the WIDTH query above.
        unsafe {
            self.connection.QuerySurface(
                self.connection.display,
                self.surface,
                ffi::HEIGHT as i32,
                &mut height,
            );
        }
        (width as u32, height as u32)
    }

    fn is_current(&self) -> bool {
        // SAFETY: `GetCurrentContext` takes no arguments; `self.context` is valid.
        unsafe { self.connection.GetCurrentContext() == self.context }
    }

    unsafe fn make_current(&self) {
        if self.connection.MakeCurrent(
            self.connection.display,
            self.surface,
            self.surface,
            self.context,
        ) == 0
        {
            let err = self.connection.egl.error("MakeCurrent");
            log::error!("make_current failed {:?} {:?}", self, err);
        }
    }
}
