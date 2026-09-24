//! GPU vertex/instance layout types used by the in-process renderer.
//!
//! The plain `QuadInstance` ABI type is shared with the wire protocol crate;
//! this module keeps the WebGPU-specific vertex-layout adapter alongside the
//! renderer.

pub use onlyterm_gpu_protocol::QuadInstance;

/// Each cell is composed of two triangles built from 4 vertices.
/// The buffer is organized row by row.
pub const VERTICES_PER_CELL: usize = 4;
pub const V_TOP_LEFT: usize = 0;
pub const V_TOP_RIGHT: usize = 1;
pub const V_BOT_LEFT: usize = 2;
pub const V_BOT_RIGHT: usize = 3;

pub(crate) fn quad_instance_desc() -> wgpu::VertexBufferLayout<'static> {
    // Locations start at 1, not 0: location 0 belongs to `CornerVertex`,
    // the other vertex buffer bound into this same pipeline, and shader
    // attribute locations must be unique across *all* of a pipeline's
    // vertex buffers, not just within one. Numbering these from 0
    // collides with the corner buffer, which wgpu rejects at
    // `create_render_pipeline` time -- and since wgpu treats validation
    // errors as fatal by default, that surfaces as a panic on the render
    // thread, not a recoverable error: the window's renderer rebuild
    // logic then retries, fails identically, and gives up by closing the
    // window.
    //
    // `vertex_attr_array!` assigns byte offsets sequentially from the
    // formats listed here, so this list's ORDER must match
    // `QuadInstance`'s field declaration order (position, fg_color,
    // alt_color, tex, hsv, has_color, mix_value), and shader.wgsl's
    // `InstanceInput` must bind the same field to the same location.
    // Getting the order wrong doesn't fail validation -- it silently
    // feeds e.g. texture coordinates in as a color.
    const ATTRIBS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
        1 => Float32x4, // position: [left, top, right, bottom]
        2 => Float32x4, // fg_color
        3 => Float32x4, // alt_color
        4 => Float32x4, // tex: [x1, x2, y1, y2]
        5 => Float32x3, // hsv
        6 => Float32,   // has_color
        7 => Float32,   // mix_value
    ];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<QuadInstance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBS,
    }
}

/// Shared corner data: unit coordinates for the 4 corners of a quad.
/// Used with VertexStepMode::Vertex to interpolate instance data.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CornerVertex {
    /// Unit vector in [0,1] range for this corner (e.g., [0.0, 0.0] = top-left)
    pub corner_unit: [f32; 2],
}

impl CornerVertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![
            0 => Float32x2,
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBS,
        }
    }

    /// Create the 4 static corners for quad rendering.
    /// Order must match V_TOP_LEFT, V_TOP_RIGHT, V_BOT_LEFT, V_BOT_RIGHT.
    pub fn static_corners() -> [CornerVertex; 4] {
        [
            CornerVertex {
                corner_unit: [0.0, 0.0],
            }, // V_TOP_LEFT
            CornerVertex {
                corner_unit: [1.0, 0.0],
            }, // V_TOP_RIGHT
            CornerVertex {
                corner_unit: [0.0, 1.0],
            }, // V_BOT_LEFT
            CornerVertex {
                corner_unit: [1.0, 1.0],
            }, // V_BOT_RIGHT
        ]
    }
}

/// Regression coverage for the instanced-rendering vertex layout (task #447).
///
/// The instanced pipeline binds two vertex buffers -- `CornerVertex` (one
/// static set of 4 corners, `step_mode: Vertex`) and `QuadInstance` (one
/// record per quad, `step_mode: Instance`) -- and there is no single place
/// where a mismatch between them shows up as a compile error. Two distinct
/// ways to get it wrong both actually happened while building this, and
/// neither was caught by the build, by clippy, or by launching the app for a
/// few seconds:
///
///  1. **Colliding locations.** Shader attribute locations must be unique
///     across *all* of a pipeline's vertex buffers, not just within one
///     buffer. Numbering `QuadInstance`'s attributes from 0 collided with
///     `CornerVertex`'s location 0. wgpu rejects that at
///     `create_render_pipeline` time, and since it treats validation errors
///     as fatal by default, the failure arrives as a *panic on the render
///     thread* -- which the window's renderer-rebuild logic then retries,
///     fails identically, and finally gives up on by closing the window
///     (taking every tab in it). Nothing reaches stderr, because the GUI
///     binary is `windows_subsystem = "windows"`; nothing reaches Windows
///     Error Reporting either, because closing the window is a *graceful*
///     shutdown, not a crash. The only trace is OnlyTerm's own per-PID log
///     file under `config::RUNTIME_DIR`.
///
///  2. **Right locations, wrong fields.** `vertex_attr_array!` derives each
///     attribute's byte offset from its position in the list, so the list's
///     order has to match `QuadInstance`'s field declaration order, and
///     shader.wgsl has to bind each location to the field that actually
///     lands at that offset. Swapping two same-sized fields (e.g. `tex` and
///     `fg_color`, both `vec4<f32>`) passes validation and simply renders
///     wrong -- texture coordinates fed in as a color.
///
/// These tests pin all three sides (the Rust struct layout, the
/// `VertexBufferLayout` descriptors, and the WGSL declarations) against one
/// shared table, so changing any one of them without the others fails here
/// rather than at runtime on a user's screen.
#[cfg(test)]
mod pipeline_layout {
    use super::*;

    /// One per-instance attribute, as it must appear in all three places.
    struct AttrSpec {
        /// `@location(N)` in shader.wgsl, and `shader_location` in the
        /// `VertexAttribute`.
        location: u32,
        /// Byte offset of the backing field within `QuadInstance`, taken
        /// from the struct itself rather than hand-counted.
        offset: u64,
        format: wgpu::VertexFormat,
        /// Field name as declared in shader.wgsl.
        wgsl_name: &'static str,
        wgsl_type: &'static str,
    }

    /// The single source of truth: which `QuadInstance` field feeds which
    /// shader location, in the order `vertex_attr_array!` lays them out.
    fn instance_spec() -> Vec<AttrSpec> {
        use std::mem::offset_of;
        vec![
            AttrSpec {
                location: 1,
                offset: offset_of!(QuadInstance, position) as u64,
                format: wgpu::VertexFormat::Float32x4,
                wgsl_name: "position",
                wgsl_type: "vec4<f32>",
            },
            AttrSpec {
                location: 2,
                offset: offset_of!(QuadInstance, fg_color) as u64,
                format: wgpu::VertexFormat::Float32x4,
                wgsl_name: "fg_color",
                wgsl_type: "vec4<f32>",
            },
            AttrSpec {
                location: 3,
                offset: offset_of!(QuadInstance, alt_color) as u64,
                format: wgpu::VertexFormat::Float32x4,
                wgsl_name: "alt_color",
                wgsl_type: "vec4<f32>",
            },
            AttrSpec {
                location: 4,
                offset: offset_of!(QuadInstance, tex) as u64,
                format: wgpu::VertexFormat::Float32x4,
                wgsl_name: "tex",
                wgsl_type: "vec4<f32>",
            },
            AttrSpec {
                location: 5,
                offset: offset_of!(QuadInstance, hsv) as u64,
                format: wgpu::VertexFormat::Float32x3,
                wgsl_name: "hsv",
                wgsl_type: "vec3<f32>",
            },
            AttrSpec {
                location: 6,
                offset: offset_of!(QuadInstance, has_color) as u64,
                format: wgpu::VertexFormat::Float32,
                wgsl_name: "has_color",
                wgsl_type: "f32",
            },
            AttrSpec {
                location: 7,
                offset: offset_of!(QuadInstance, mix_value) as u64,
                format: wgpu::VertexFormat::Float32,
                wgsl_name: "mix_value",
                wgsl_type: "f32",
            },
        ]
    }

    const SHADER_SRC: &str = include_str!("shader.wgsl");

    /// Extract `(location, field_name, wgsl_type)` for every `@location(..)`
    /// member of a WGSL `struct <name> { .. }` block, in declaration order.
    fn parse_wgsl_struct(src: &str, struct_name: &str) -> Vec<(u32, String, String)> {
        let header = format!("struct {struct_name} {{");
        let start = src
            .find(&header)
            .unwrap_or_else(|| panic!("shader.wgsl no longer declares `{}`", header));
        let body_start = start + header.len();
        let body_len = src[body_start..]
            .find('}')
            .unwrap_or_else(|| panic!("unterminated `struct {}` in shader.wgsl", struct_name));
        let body = &src[body_start..body_start + body_len];

        let mut out = vec![];
        for line in body.lines() {
            // Drop any trailing `// ...` comment before parsing; a
            // comment-only line then trims to empty and is skipped.
            let line = line.split("//").next().unwrap_or("").trim();
            let Some(rest) = line.strip_prefix("@location(") else {
                continue;
            };
            let close = rest
                .find(')')
                .unwrap_or_else(|| panic!("unterminated `@location(` in: {}", line));
            let location: u32 = rest[..close]
                .trim()
                .parse()
                .unwrap_or_else(|e| panic!("non-numeric location in `{}`: {}", line, e));
            let decl = rest[close + 1..].trim().trim_end_matches(',');
            let (name, ty) = decl
                .split_once(':')
                .unwrap_or_else(|| panic!("expected `name: type` in: {}", line));
            out.push((
                location,
                name.trim().to_string(),
                ty.trim().trim_end_matches(',').trim().to_string(),
            ));
        }
        out
    }

    /// `QuadInstance::desc()` must describe exactly the fields of
    /// `QuadInstance`, at the offsets those fields actually occupy.
    #[test]
    fn quad_instance_desc_matches_struct_layout() {
        let desc = quad_instance_desc();
        let spec = instance_spec();

        assert_eq!(
            desc.step_mode,
            wgpu::VertexStepMode::Instance,
            "QuadInstance is the per-quad buffer; stepping it per-vertex would \
             replay one quad's data across the 4 shared corners"
        );
        assert_eq!(
            desc.array_stride,
            std::mem::size_of::<QuadInstance>() as wgpu::BufferAddress,
            "array_stride must be the full struct size or every instance after \
             the first reads misaligned data"
        );
        assert_eq!(
            desc.attributes.len(),
            spec.len(),
            "every QuadInstance field must be described exactly once"
        );

        for (attr, want) in desc.attributes.iter().zip(spec.iter()) {
            assert_eq!(
                attr.shader_location, want.location,
                "attribute for `{}` is at location {}, expected {}",
                want.wgsl_name, attr.shader_location, want.location
            );
            assert_eq!(
                attr.offset, want.offset,
                "attribute at location {} must read from `{}` (offset {}), but \
                 vertex_attr_array! placed it at offset {} -- the ATTRIBS list \
                 order no longer matches the struct's field order",
                want.location, want.wgsl_name, want.offset, attr.offset
            );
            assert_eq!(
                attr.format, want.format,
                "attribute at location {} (`{}`) has the wrong format",
                want.location, want.wgsl_name
            );
        }
    }

    /// The corner buffer is a fixed 4-entry, single-attribute buffer.
    #[test]
    fn corner_vertex_desc_matches_struct_layout() {
        let desc = CornerVertex::desc();
        assert_eq!(desc.step_mode, wgpu::VertexStepMode::Vertex);
        assert_eq!(
            desc.array_stride,
            std::mem::size_of::<CornerVertex>() as wgpu::BufferAddress
        );
        assert_eq!(desc.attributes.len(), 1);
        assert_eq!(desc.attributes[0].shader_location, 0);
        assert_eq!(desc.attributes[0].offset, 0);
        assert_eq!(desc.attributes[0].format, wgpu::VertexFormat::Float32x2);
    }

    /// The bug that actually shipped: shader locations are a single
    /// namespace shared by every vertex buffer in the pipeline, so the
    /// corner buffer's location 0 and the instance buffer's locations must
    /// not overlap. wgpu rejects a pipeline that violates this, and it does
    /// so by panicking on the render thread.
    #[test]
    fn pipeline_vertex_locations_do_not_collide() {
        let mut seen: Vec<(u32, &'static str)> = vec![];
        for attr in CornerVertex::desc().attributes {
            seen.push((attr.shader_location, "CornerVertex"));
        }
        for attr in quad_instance_desc().attributes {
            seen.push((attr.shader_location, "QuadInstance"));
        }

        for i in 0..seen.len() {
            for j in (i + 1)..seen.len() {
                assert_ne!(
                    seen[i].0, seen[j].0,
                    "shader location {} is claimed by both {} and {}; locations \
                     must be unique across every vertex buffer bound to one \
                     pipeline, or create_render_pipeline fails validation (and \
                     wgpu turns that into a render-thread panic, which ends up \
                     closing the window)",
                    seen[i].0, seen[i].1, seen[j].1
                );
            }
        }
    }

    /// shader.wgsl's `InstanceInput` must bind each location to the same
    /// field the Rust side feeds into it. Two same-sized fields swapped here
    /// still passes GPU validation and just renders wrong.
    #[test]
    fn shader_instance_input_matches_rust_layout() {
        let parsed = parse_wgsl_struct(SHADER_SRC, "InstanceInput");
        let spec = instance_spec();

        assert_eq!(
            parsed.len(),
            spec.len(),
            "shader.wgsl's InstanceInput declares {} attributes, Rust supplies {}",
            parsed.len(),
            spec.len()
        );

        for (want, (location, name, ty)) in spec.iter().zip(parsed.iter()) {
            assert_eq!(
                *location, want.location,
                "shader.wgsl's InstanceInput is out of order: found location {} \
                 (`{}`) where the Rust layout supplies location {} (`{}`)",
                location, name, want.location, want.wgsl_name
            );
            assert_eq!(
                name, want.wgsl_name,
                "location {} is fed from QuadInstance::{} (offset {}), but \
                 shader.wgsl binds it to `{}`",
                want.location, want.wgsl_name, want.offset, name
            );
            assert_eq!(
                ty, want.wgsl_type,
                "location {} (`{}`) is declared `{}` in shader.wgsl but supplied \
                 as {:?}",
                want.location, want.wgsl_name, ty, want.format
            );
        }
    }

    #[test]
    fn shader_corner_input_matches_rust_layout() {
        let parsed = parse_wgsl_struct(SHADER_SRC, "CornerInput");
        assert_eq!(
            parsed.len(),
            1,
            "CornerInput should have exactly one member"
        );
        assert_eq!(parsed[0].0, 0, "corner attribute must be at location 0");
        assert_eq!(parsed[0].1, "corner_unit");
        assert_eq!(parsed[0].2, "vec2<f32>");
    }

    /// The 4 shared corners must reproduce, via
    /// `mix(pos_min, pos_max, corner_unit)` in the vertex shader, exactly
    /// the same (position, tex) pairs the pre-instancing code baked into 4
    /// full vertices -- see `Quad::set_position`/`set_texture_discrete`.
    /// A swapped pair here renders every glyph mirrored or degenerate
    /// without failing anything else.
    #[test]
    fn static_corners_match_the_pre_instancing_vertex_convention() {
        let corners = CornerVertex::static_corners();
        assert_eq!(corners.len(), VERTICES_PER_CELL);

        // Same rect the old 4-vertex path would have been handed.
        let (left, top, right, bottom) = (10.0f32, 20.0f32, 30.0f32, 40.0f32);
        let (x1, x2, y1, y2) = (0.1f32, 0.2f32, 0.3f32, 0.4f32);

        // What the old code baked into each vertex, by index.
        let expected_position = [
            [left, top],     // V_TOP_LEFT
            [right, top],    // V_TOP_RIGHT
            [left, bottom],  // V_BOT_LEFT
            [right, bottom], // V_BOT_RIGHT
        ];
        let expected_tex = [
            [x1, y1], // V_TOP_LEFT
            [x2, y1], // V_TOP_RIGHT
            [x1, y2], // V_BOT_LEFT
            [x2, y2], // V_BOT_RIGHT
        ];

        // The vertex shader's `mix(min, max, corner_unit)`, in Rust.
        let mix = |min: f32, max: f32, t: f32| min + (max - min) * t;

        for (idx, corner) in corners.iter().enumerate() {
            let [u, v] = corner.corner_unit;
            assert!(
                (u == 0.0 || u == 1.0) && (v == 0.0 || v == 1.0),
                "corner {idx} unit vector must select an edge, got {:?}",
                corner.corner_unit
            );

            let position = [mix(left, right, u), mix(top, bottom, v)];
            assert_eq!(
                position, expected_position[idx],
                "corner {idx} produces position {:?}, but the pre-instancing \
                 vertex at that index was {:?}",
                position, expected_position[idx]
            );

            // tex is packed as [x1, x2, y1, y2]: u picks between x1/x2, v
            // between y1/y2 (see vs_main's tex_min/tex_max).
            let tex = [mix(x1, x2, u), mix(y1, y2, v)];
            assert_eq!(
                tex, expected_tex[idx],
                "corner {idx} produces tex {:?}, but the pre-instancing vertex \
                 at that index was {:?}",
                tex, expected_tex[idx]
            );
        }

        // Sanity-check the named indices themselves still mean what the
        // table above assumes.
        assert_eq!(corners[V_TOP_LEFT].corner_unit, [0.0, 0.0]);
        assert_eq!(corners[V_TOP_RIGHT].corner_unit, [1.0, 0.0]);
        assert_eq!(corners[V_BOT_LEFT].corner_unit, [0.0, 1.0]);
        assert_eq!(corners[V_BOT_RIGHT].corner_unit, [1.0, 1.0]);
    }
}
