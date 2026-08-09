// Vertex shader

struct CornerInput {
    @location(0) corner_unit: vec2<f32>,
}

struct InstanceInput {
    @location(1) position: vec4<f32>,  // [left, top, right, bottom]
    @location(2) tex: vec4<f32>,       // [x1, x2, y1, y2]
    @location(3) fg_color: vec4<f32>,
    @location(4) alt_color: vec4<f32>,
    @location(5) hsv: vec3<f32>,
    @location(6) has_color: f32,
    @location(7) mix_value: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex: vec2<f32>,
    @location(1) fg_color: vec4<f32>,
    @location(2) hsv: vec3<f32>,
    @location(3) has_color: f32,
};

// a regular monochrome text glyph
const IS_GLYPH: f32 = 0.0;

// a color emoji glyph
const IS_COLOR_EMOJI: f32 = 1.0;

// a full color texture attached as the
// background image of the window
const IS_BG_IMAGE: f32 = 2.0;

// like 2.0, except that instead of an
// image, we use the solid bg color
const IS_SOLID_COLOR: f32 = 3.0;

// Grayscale poly quad for non-aa text render layers
const IS_GRAY_SCALE: f32 = 4.0;

struct ShaderUniform {
  foreground_text_hsb: vec3<f32>,
  milliseconds: u32,
  projection: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> uniforms: ShaderUniform;

@group(1) @binding(0) var atlas_linear_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_linear_sampler: sampler;

@group(2) @binding(0) var atlas_nearest_tex: texture_2d<f32>;
@group(2) @binding(1) var atlas_nearest_sampler: sampler;

fn rgb2hsv(c: vec3<f32>) -> vec3<f32>
{
    let K = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4<f32>(c.bg, K.wz), vec4<f32>(c.gb, K.xy), step(c.b, c.g));
    let q = mix(vec4<f32>(p.xyw, c.r), vec4<f32>(c.r, p.yzx), step(p.x, c.r));

    let d = q.x - min(q.w, q.y);
    let e = 1.0e-10;
    return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

fn hsv2rgb(c: vec3<f32>) -> vec3<f32>
{
    let K = vec4<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    let p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, vec3(0.0), vec3(1.0)), c.y);
}

fn apply_hsv(c: vec4<f32>, transform: vec3<f32>) -> vec4<f32>
{
  let hsv = rgb2hsv(c.rgb) * transform;
  return vec4<f32>(hsv2rgb(hsv).rgb, c.a);
}

// The render target we write into is a non-sRGB view of an sRGB-capable
// surface (see WebGpuState::submit_frame), so blending happens directly on
// these gamma-encoded bytes rather than being auto-linearized by the GPU
// (matching the gamma-space blending of the OpenGL renderer this backend
// replaced -- removed in task #414). Without this, alpha-blended glyph edges
// are composited in linear light instead of gamma space, which reads as
// visibly thinner strokes for the same coverage data.
fn to_srgb(linear_rgb: vec4<f32>) -> vec4<f32>
{
  let cutoff = linear_rgb.rgb < vec3<f32>(0.0031308);
  let higher = vec3<f32>(1.055) * pow(linear_rgb.rgb, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
  let lower = linear_rgb.rgb * vec3<f32>(12.92);
  return vec4<f32>(select(higher, lower, cutoff), linear_rgb.a);
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    corner: CornerInput,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;

    // corner_unit is in [0,1] range: (0,0) = top-left, (1,0) = top-right, (0,1) = bot-left, (1,1) = bot-right
    // Interpolate position: mix(top-left, bottom-right, corner_unit)
    let pos_min = vec2<f32>(instance.position.x, instance.position.y);
    let pos_max = vec2<f32>(instance.position.z, instance.position.w);
    out.clip_position = vec4<f32>(mix(pos_min, pos_max, corner.corner_unit), 0.0, 1.0);
    out.clip_position = uniforms.projection * out.clip_position;

    // Interpolate texture coordinates
    let tex_min = vec2<f32>(instance.tex.x, instance.tex.z);
    let tex_max = vec2<f32>(instance.tex.y, instance.tex.w);
    out.tex = mix(tex_min, tex_max, corner.corner_unit);

    out.hsv = instance.hsv;
    out.has_color = instance.has_color;
    out.fg_color = mix(instance.fg_color, instance.alt_color, instance.mix_value);

    return out;
}

// Fragment shader

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
  var color: vec4<f32>;
  var linear_tex: vec4<f32> = textureSample(atlas_linear_tex, atlas_linear_sampler, in.tex);
  var nearest_tex: vec4<f32> = textureSample(atlas_nearest_tex, atlas_nearest_sampler, in.tex);

  var hsv = in.hsv;

  if in.has_color == IS_SOLID_COLOR {
    // Solid color block
    color = in.fg_color;
  } else if in.has_color == IS_BG_IMAGE {
    // Window background attachment
    // Apply window_background_image_opacity to the background image
    color = linear_tex;
    color.a *= in.fg_color.a;
  } else if in.has_color == IS_COLOR_EMOJI {
    // the texture is full color info (eg: color emoji glyph)
    color = nearest_tex;
  } else if in.has_color == IS_GRAY_SCALE {
    // Grayscale poly quad for non-aa text render layers
    color = in.fg_color;
    color.a *= nearest_tex.a;
  } else if in.has_color == IS_GLYPH {
    // the texture is the alpha channel/color mask
    // and we need to tint with the fg_color
    color = in.fg_color;
    color.a = nearest_tex.a;
    hsv *= uniforms.foreground_text_hsb;
  }

  color = apply_hsv(color, hsv);

  return to_srgb(color);
}