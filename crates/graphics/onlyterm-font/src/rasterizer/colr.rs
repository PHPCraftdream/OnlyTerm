use crate::rasterizer::paint::Painter;
use onlyterm_color_types::{SrgbaPixel, SrgbaTuple};
use tiny_skia::{
    BlendMode, Color, GradientStop, LinearGradient, Point, RadialGradient, SpreadMode, Transform,
};

/* The gradient related routines in this file were ported from HarfBuzz, which
 * were in turn ported from BlackRenderer by Black Foundry.
 * Used by permission to relicense to HarfBuzz license,
 * which is in turn compatible with onlyterm's license.
 *
 * https://github.com/BlackFoundryCom/black-renderer
 *
 * The linear gradient reduction (`reduce_anchors`) and the general
 * two-radius conical/radial gradient math below were ported from the
 * same lineage; the conical gradient solve mirrors the classic
 * Skia/cairo "two point conical gradient" derivation.
 *
 * `paint_radial_gradient` and `paint_sweep_gradient` are implemented as
 * custom per-pixel shaders driven through `Painter::paint_with`, rather
 * than tiny-skia's built-in gradient shaders: tiny-skia 0.11 has no
 * sweep/conic gradient shader at all, and its `RadialGradient` only
 * supports a zero-radius start circle, whereas COLRv1 fonts may specify
 * a non-zero `r0` (a genuine two-radius conical gradient). Only the
 * linear gradient case is fully covered by a native tiny-skia shader.
 */

#[derive(Clone, Debug)]
pub struct ColorStop {
    pub offset: f64,
    pub color: SrgbaPixel,
}

#[derive(Clone, Debug)]
pub struct ColorLine {
    pub color_stops: Vec<ColorStop>,
    pub extend: SpreadMode,
}

#[derive(Debug, Clone)]
pub enum PaintOp {
    PushTransform(Transform),
    PopTransform,
    PushClip(Vec<DrawOp>),
    PopClip,
    PaintSolid(SrgbaPixel),
    PaintLinearGradient {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color_line: ColorLine,
    },
    PaintRadialGradient {
        x0: f32,
        y0: f32,
        r0: f32,
        x1: f32,
        y1: f32,
        r1: f32,
        color_line: ColorLine,
    },
    PaintSweepGradient {
        x0: f32,
        y0: f32,
        start_angle: f32,
        end_angle: f32,
        color_line: ColorLine,
    },
    PushGroup,
    PopGroup(BlendMode),
}

#[derive(Debug, Clone)]
pub enum DrawOp {
    MoveTo {
        to_x: f32,
        to_y: f32,
    },
    LineTo {
        to_x: f32,
        to_y: f32,
    },
    QuadTo {
        control_x: f32,
        control_y: f32,
        to_x: f32,
        to_y: f32,
    },
    CubicTo {
        control1_x: f32,
        control1_y: f32,
        control2_x: f32,
        control2_y: f32,
        to_x: f32,
        to_y: f32,
    },
    ClosePath,
}

/// Converts an `SrgbaTuple` (straight alpha, f32 channels in 0.0..=1.0)
/// into a `tiny_skia::Color`. Channels are clamped by `Color::from_rgba`
/// semantics (it returns `None` only for NaN/out-of-range values, which
/// shouldn't occur for well-formed font color data; fall back to
/// transparent black rather than panicking on malformed input).
fn srgba_tuple_to_color(t: SrgbaTuple) -> Color {
    let (r, g, b, a) = t.to_tuple_rgba();
    Color::from_rgba(r, g, b, a).unwrap_or(Color::TRANSPARENT)
}

fn color_stop_to_gradient_stop(stop: &ColorStop) -> GradientStop {
    GradientStop::new(stop.offset as f32, srgba_tuple_to_color(stop.color.into()))
}

// The eight parameters mirror the OpenType COLR `PaintLinearGradient`
// record (x0,y0,x1,y1,x2,y2,color_line) one-to-one; bundling them into a
// struct would obscure that correspondence with the spec.
#[allow(clippy::too_many_arguments)]
pub fn paint_linear_gradient(
    painter: &mut Painter,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    mut color_line: ColorLine,
) -> anyhow::Result<()> {
    let (min_stop, max_stop) = normalize_color_line(&mut color_line);
    let anchors = reduce_anchors(ReduceAnchorsIn {
        x0,
        y0,
        x1,
        y1,
        x2,
        y2,
    });

    let xxx0 = anchors.xx0 + min_stop * (anchors.xx1 - anchors.xx0);
    let yyy0 = anchors.yy0 + min_stop * (anchors.yy1 - anchors.yy0);
    let xxx1 = anchors.xx0 + max_stop * (anchors.xx1 - anchors.xx0);
    let yyy1 = anchors.yy0 + max_stop * (anchors.yy1 - anchors.yy0);

    let stops: Vec<GradientStop> = color_line
        .color_stops
        .iter()
        .map(color_stop_to_gradient_stop)
        .collect();

    if let Some(shader) = LinearGradient::new(
        Point::from_xy(xxx0 as f32, yyy0 as f32),
        Point::from_xy(xxx1 as f32, yyy1 as f32),
        stops,
        color_line.extend,
        Transform::identity(),
    ) {
        painter.paint_shader(shader);
    }

    Ok(())
}

/// A general two-radius conical/radial gradient, i.e. the interpolation
/// between a start circle (`c0`, `r0`) and an end circle (`c1`, `r1`),
/// exactly as COLRv1's `PaintRadialGradient` and cairo's/CSS's
/// `radial-gradient()` define it. tiny-skia's `RadialGradient` shader
/// only supports `r0 == 0`, so this is implemented as a manual per-pixel
/// shader (see `Painter::paint_with`) using the same quadratic solve
/// that Skia/cairo use internally for the general case.
struct ConicalGradient {
    c0: (f64, f64),
    r0: f64,
    c1: (f64, f64),
    r1: f64,
    color_line: ColorLine,
}

impl ConicalGradient {
    /// Returns the straight-alpha color at device/user-space point
    /// `(px, py)`, or `None` if the point is outside the gradient's
    /// domain (i.e. no valid interpolation parameter `t` produces a
    /// circle passing through the point) - `Extend::Pad`'s edge
    /// clamping is handled by the caller by clamping `t` instead, so a
    /// `None` here only occurs for degenerate gradients.
    fn color_at(&self, px: f64, py: f64) -> Option<Color> {
        let (x0, y0) = self.c0;
        let (x1, y1) = self.c1;
        let dx = x1 - x0;
        let dy = y1 - y0;
        let dr = self.r1 - self.r0;

        // Expanding |P - C(t)|^2 = R(t)^2, where C(t) = c0 + t*(c1-c0) and
        // R(t) = r0 + t*dr, into the standard quadratic form a*t^2+b*t+c=0
        // gives b = -2*(fx*dx + fy*dy + r0*dr) (note the leading minus
        // sign - this used to be dropped here, which silently negated
        // every solved `t` and inverted the whole gradient ramp, e.g.
        // painting the *last* color stop at a radial gradient's center
        // instead of the first: see BUG5 in the migration plan).
        let a = dx * dx + dy * dy - dr * dr;
        let fx = px - x0;
        let fy = py - y0;
        let b = -2.0 * (fx * dx + fy * dy + self.r0 * dr);
        let c = fx * fx + fy * fy - self.r0 * self.r0;

        let t = if a.abs() < 1e-9 {
            // Linear (degenerate quadratic): b*t + c = 0, so t = -c / b.
            if b.abs() < 1e-12 {
                return None;
            }
            -c / b
        } else {
            let disc = b * b - 4.0 * a * c;
            if disc < 0.0 {
                return None;
            }
            let sqrt_disc = disc.sqrt();
            // We want the largest t for which r(t) = r0 + t*dr >= 0,
            // matching Skia's convention of preferring the root that
            // keeps the interpolated radius non-negative.
            let t0 = (-b + sqrt_disc) / (2.0 * a);
            let t1 = (-b - sqrt_disc) / (2.0 * a);
            let (t_hi, t_lo) = if t0 > t1 { (t0, t1) } else { (t1, t0) };
            if self.r0 + t_hi * dr >= 0.0 {
                t_hi
            } else if self.r0 + t_lo * dr >= 0.0 {
                t_lo
            } else {
                return None;
            }
        };

        let t = apply_spread(t, self.color_line.extend);
        Some(color_at_offset(&self.color_line, t))
    }
}

/// Applies `SpreadMode` to a gradient interpolation parameter `t`, which
/// may fall outside `0.0..=1.0`.
fn apply_spread(t: f64, extend: SpreadMode) -> f64 {
    match extend {
        SpreadMode::Pad => t.clamp(0.0, 1.0),
        SpreadMode::Repeat => t.rem_euclid(1.0),
        SpreadMode::Reflect => {
            let period = t.rem_euclid(2.0);
            if period > 1.0 {
                2.0 - period
            } else {
                period
            }
        }
    }
}

/// Evaluates a (already-normalized, see `normalize_color_line`) color
/// line at interpolation parameter `t` (expected to already be within
/// `0.0..=1.0`, e.g. via `apply_spread`), linearly interpolating
/// between the two bracketing stops.
fn color_at_offset(color_line: &ColorLine, t: f64) -> Color {
    let stops = &color_line.color_stops;
    if stops.is_empty() {
        return Color::TRANSPARENT;
    }
    if stops.len() == 1 || t <= stops[0].offset {
        return srgba_tuple_to_color(stops[0].color.into());
    }
    let last = stops.len() - 1;
    if t >= stops[last].offset {
        return srgba_tuple_to_color(stops[last].color.into());
    }
    for i in 1..stops.len() {
        if t <= stops[i].offset {
            let a = &stops[i - 1];
            let b = &stops[i];
            let span = b.offset - a.offset;
            let k = if span.abs() < 1e-12 {
                0.0
            } else {
                (t - a.offset) / span
            };
            let interpolated: SrgbaTuple = SrgbaTuple::from(a.color).interpolate(b.color.into(), k);
            return srgba_tuple_to_color(interpolated);
        }
    }
    srgba_tuple_to_color(stops[last].color.into())
}

// Parameters mirror the OpenType COLR `PaintRadialGradient` record
// (start circle x0,y0,r0 + end circle x1,y1,r1 + color_line) one-to-one;
// bundling them into a struct would obscure that correspondence with the spec.
#[allow(clippy::too_many_arguments)]
pub fn paint_radial_gradient(
    painter: &mut Painter,
    x0: f64,
    y0: f64,
    r0: f64,
    x1: f64,
    y1: f64,
    r1: f64,
    mut color_line: ColorLine,
) -> anyhow::Result<()> {
    let (min_stop, max_stop) = normalize_color_line(&mut color_line);

    let xx0 = x0 + min_stop * (x1 - x0);
    let yy0 = y0 + min_stop * (y1 - y0);
    let xx1 = x0 + max_stop * (x1 - x0);
    let yy1 = y0 + max_stop * (y1 - y0);
    let rr0 = r0 + min_stop * (r1 - r0);
    let rr1 = r0 + max_stop * (r1 - r0);

    // Fast path: when the start circle has zero radius and the centers
    // coincide with a simple radial gradient, tiny-skia's native
    // `RadialGradient` shader covers it exactly and is much cheaper than
    // the manual per-pixel fallback.
    if rr0 == 0.0 && (xx0, yy0) == (xx1, yy1) && rr1 > 0.0 {
        let stops: Vec<GradientStop> = color_line
            .color_stops
            .iter()
            .map(color_stop_to_gradient_stop)
            .collect();
        if let Some(shader) = RadialGradient::new(
            Point::from_xy(xx0 as f32, yy0 as f32),
            Point::from_xy(xx1 as f32, yy1 as f32),
            rr1 as f32,
            stops,
            color_line.extend,
            Transform::identity(),
        ) {
            painter.paint_shader(shader);
            return Ok(());
        }
    }

    let gradient = ConicalGradient {
        c0: (xx0, yy0),
        r0: rr0,
        c1: (xx1, yy1),
        r1: rr1,
        color_line,
    };

    painter.paint_with(|x, y| {
        gradient
            .color_at(x as f64, y as f64)
            .unwrap_or(Color::TRANSPARENT)
    });

    Ok(())
}

const PI_TIMES_2: f64 = std::f64::consts::PI * 2.;

/// A sweep (conic) gradient around `center`, spanning `start_angle` to
/// `end_angle` (radians, `atan2`-style: 0 along +x, increasing towards
/// +y), evaluated exactly (no tessellation) for every pixel via
/// `Painter::paint_with`. Replaces the cairo-era mesh-patch
/// tessellation (`Patch`/`add_sweep_gradient_patches`/
/// `apply_sweep_gradient_patches`), which existed only because cairo had
/// no native conic gradient primitive to fall back on; tiny-skia doesn't
/// have one either (as of 0.11), but since we already need a manual
/// per-pixel shader for the general radial case above, sweep gradients
/// can use the exact same mechanism instead of approximating the arc
/// with Bezier patches.
pub fn paint_sweep_gradient(
    painter: &mut Painter,
    x0: f64,
    y0: f64,
    start_angle: f64,
    end_angle: f64,
    color_line: ColorLine,
) -> anyhow::Result<()> {
    painter.paint_with(|x, y| {
        let dx = x as f64 - x0;
        let dy = y as f64 - y0;
        let mut angle = dy.atan2(dx);
        if angle < 0.0 {
            angle += PI_TIMES_2;
        }

        // Degenerate case: a single angle. Mirrors the cairo/HarfBuzz
        // convention (see the removed `apply_sweep_gradient_patches`):
        // with Pad extend, everything is the nearest stop's color;
        // otherwise, transparent.
        if start_angle == end_angle {
            return if color_line.extend == SpreadMode::Pad {
                let c = if angle <= start_angle {
                    color_line.color_stops[0].color
                } else {
                    color_line.color_stops.last().unwrap().color
                };
                srgba_tuple_to_color(c.into())
            } else {
                Color::TRANSPARENT
            };
        }

        // Map `angle` (always in 0..2*PI from atan2) into the same
        // winding domain as `[start_angle, end_angle]`, which may be
        // wider than one turn or wound the other way.
        let span = end_angle - start_angle;
        let mut t = (angle - start_angle) / span;
        // atan2's principal range means `angle` (and hence the raw `t`
        // above) can be off by a full turn relative to the intended
        // sweep; fold it back so Pad clamps against the *nearest*
        // occurrence of the sweep, matching how the mesh code treated
        // angles as periodic in 2*PI.
        if color_line.extend == SpreadMode::Pad {
            let period = PI_TIMES_2 / span.abs();
            if t < 0.0 {
                t += period * (-t / period).ceil().max(1.0);
            } else if t > 1.0 {
                t -= period * ((t - 1.0) / period).ceil().max(1.0);
            }
            t = t.clamp(0.0, 1.0);
        } else {
            t = apply_spread(t, color_line.extend);
        }

        color_at_offset(&color_line, t)
    });

    Ok(())
}

fn normalize_color_line(color_line: &mut ColorLine) -> (f64, f64) {
    let mut smallest = color_line.color_stops[0].offset;
    let mut largest = smallest;

    color_line
        .color_stops
        .sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap());

    for stop in &color_line.color_stops[1..] {
        smallest = smallest.min(stop.offset);
        largest = largest.max(stop.offset);
    }

    if smallest != largest {
        for stop in &mut color_line.color_stops {
            stop.offset = (stop.offset - smallest) / (largest - smallest);
        }
    }

    (smallest, largest)
}

struct ReduceAnchorsIn {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

struct ReduceAnchorsOut {
    xx0: f64,
    yy0: f64,
    xx1: f64,
    yy1: f64,
}

fn reduce_anchors(
    ReduceAnchorsIn {
        x0,
        y0,
        x1,
        y1,
        x2,
        y2,
    }: ReduceAnchorsIn,
) -> ReduceAnchorsOut {
    let q2x = x2 - x0;
    let q2y = y2 - y0;
    let q1x = x1 - x0;
    let q1y = y1 - y0;

    let s = q2x * q2x + q2y * q2y;
    if s < 0.000001 {
        return ReduceAnchorsOut {
            xx0: x0,
            yy0: y0,
            xx1: x1,
            yy1: y1,
        };
    }

    let k = (q2x * q1x + q2y * q1y) / s;
    ReduceAnchorsOut {
        xx0: x0,
        yy0: y0,
        xx1: x1 - k * q2x,
        yy1: y1 - k * q2y,
    }
}

pub fn apply_draw_ops_to_context(ops: &[DrawOp], painter: &mut Painter) -> anyhow::Result<()> {
    painter.new_path();
    for op in ops {
        match op {
            DrawOp::MoveTo { to_x, to_y } => {
                painter.move_to(*to_x, *to_y);
            }
            DrawOp::LineTo { to_x, to_y } => {
                painter.line_to(*to_x, *to_y);
            }
            DrawOp::QuadTo {
                control_x,
                control_y,
                to_x,
                to_y,
            } => {
                // tiny-skia supports quadratic curves natively, unlike
                // cairo (which required expressing them as cubics - see
                // the removed quad-to-cubic conversion that used to
                // live here).
                painter.quad_to(*control_x, *control_y, *to_x, *to_y);
            }
            DrawOp::CubicTo {
                control1_x,
                control1_y,
                control2_x,
                control2_y,
                to_x,
                to_y,
            } => {
                painter.curve_to(
                    *control1_x,
                    *control1_y,
                    *control2_x,
                    *control2_y,
                    *to_x,
                    *to_y,
                );
            }
            DrawOp::ClosePath => {
                painter.close_path();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
