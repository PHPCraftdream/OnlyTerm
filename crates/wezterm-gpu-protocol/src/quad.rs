//! Plain GPU instance data shared across process boundaries.

/// GPU instance data for a single quad.
///
/// This contains all per-quad-unique data; the four corners are shared across
/// all quads. The field order is part of the renderer/wire ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct QuadInstance {
    /// Position rect as [left, top, right, bottom].
    pub position: [f32; 4],
    /// Foreground color as [r, g, b, a].
    pub fg_color: [f32; 4],
    /// Alternate color as [r, g, b, a].
    pub alt_color: [f32; 4],
    /// Texture rect as [x1, x2, y1, y2].
    pub tex: [f32; 4],
    /// HSV transform as [hue, saturation, brightness].
    pub hsv: [f32; 3],
    /// Quad type flag (IS_GLYPH, IS_COLOR_EMOJI, etc.).
    pub has_color: f32,
    /// Mix value for foreground/alternate-color blending.
    pub mix_value: f32,
}

#[cfg(test)]
mod tests {
    use super::QuadInstance;

    #[test]
    fn quad_instance_has_stable_wire_size() {
        assert_eq!(std::mem::size_of::<QuadInstance>(), 84);
        assert_eq!(std::mem::align_of::<QuadInstance>(), 4);
    }
}
