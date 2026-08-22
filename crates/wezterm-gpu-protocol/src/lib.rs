//! Shared, dependency-light GPU wire and ABI types.
//!
//! This crate intentionally contains no WebGPU or window-system dependencies.
//! It is used by both the renderer and the host-process transport so that the
//! protocol can be compiled and tested independently of backend initialization.

mod quad;

pub use quad::QuadInstance;

/// Uniform data shared by the renderer and its GPU shaders.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShaderUniform {
    pub foreground_text_hsb: [f32; 3],
    pub milliseconds: u32,
    pub projection: [[f32; 4]; 4],
}

pub mod wire;
