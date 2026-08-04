use crate::egl::ffi;
use crate::egl::wrapper::EglWrapper;

#[derive(Debug)]
pub struct GlConnection {
    pub(super) egl: EglWrapper,
    pub(super) display: ffi::types::EGLDisplay,
    pub(super) is_opengl: bool,
    pub(super) extensions: String,
}

impl GlConnection {
    #[allow(dead_code)]
    pub fn has_extension(&self, wanted: &str) -> bool {
        self.extensions.split(' ').any(|ext| ext == wanted)
    }
}

impl std::ops::Deref for GlConnection {
    type Target = ffi::Egl;

    fn deref(&self) -> &ffi::Egl {
        &self.egl.egl
    }
}

impl Drop for GlConnection {
    fn drop(&mut self) {
        // SAFETY: `self.display` is the valid EGL display owned by this connection,
        // obtained earlier via `GetDisplay`/`Initialize`.
        unsafe {
            self.egl.egl.Terminate(self.display);
        }
    }
}
