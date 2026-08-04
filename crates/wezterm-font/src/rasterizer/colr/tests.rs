use super::*;

fn stop(offset: f64, r: u8, g: u8, b: u8, a: u8) -> ColorStop {
    ColorStop {
        offset,
        color: SrgbaPixel::rgba(r, g, b, a),
    }
}

#[test]
fn linear_gradient_endpoints_match_stops() {
    let mut p = Painter::new(16, 16).unwrap();
    let color_line = ColorLine {
        color_stops: vec![stop(0.0, 255, 0, 0, 255), stop(1.0, 0, 0, 255, 255)],
        extend: SpreadMode::Pad,
    };
    p.new_path();
    p.move_to(0., 0.);
    p.line_to(16., 0.);
    p.line_to(16., 16.);
    p.line_to(0., 16.);
    p.close_path();
    p.clip();
    paint_linear_gradient(&mut p, 0.0, 0.0, 16.0, 0.0, 0.0, 16.0, color_line).unwrap();

    let pixmap = p.into_pixmap().unwrap();
    let left = pixmap.pixel(0, 8).unwrap();
    let right = pixmap.pixel(15, 8).unwrap();
    // Left edge should be predominantly red, right edge predominantly blue.
    assert!(left.red() > 200, "left.red()={}", left.red());
    assert!(left.blue() < 60, "left.blue()={}", left.blue());
    assert!(right.blue() > 200, "right.blue()={}", right.blue());
    assert!(right.red() < 60, "right.red()={}", right.red());
}

#[test]
fn radial_gradient_center_and_edge_match_stops() {
    let mut p = Painter::new(32, 32).unwrap();
    let color_line = ColorLine {
        color_stops: vec![stop(0.0, 255, 255, 0, 255), stop(1.0, 0, 255, 255, 255)],
        extend: SpreadMode::Pad,
    };
    p.new_path();
    p.move_to(0., 0.);
    p.line_to(32., 0.);
    p.line_to(32., 32.);
    p.line_to(0., 32.);
    p.close_path();
    p.clip();
    paint_radial_gradient(&mut p, 16.0, 16.0, 0.0, 16.0, 16.0, 16.0, color_line).unwrap();

    let pixmap = p.into_pixmap().unwrap();
    let center = pixmap.pixel(16, 16).unwrap();
    let edge = pixmap.pixel(31, 16).unwrap();
    assert!(
        center.red() > 200 && center.green() > 200,
        "center should be ~yellow: {:?}",
        center
    );
    assert!(
        edge.green() > 200 && edge.blue() > 150,
        "edge should be ~cyan: {:?}",
        edge
    );
}

#[test]
fn radial_gradient_general_two_radius_case() {
    // A genuine two-radius conical gradient (r0 != 0), which
    // tiny-skia's native RadialGradient cannot express - exercises
    // the ConicalGradient fallback path directly.
    let mut p = Painter::new(32, 32).unwrap();
    let color_line = ColorLine {
        color_stops: vec![stop(0.0, 255, 0, 0, 255), stop(1.0, 0, 0, 255, 255)],
        extend: SpreadMode::Pad,
    };
    p.new_path();
    p.move_to(0., 0.);
    p.line_to(32., 0.);
    p.line_to(32., 32.);
    p.line_to(0., 32.);
    p.close_path();
    p.clip();
    // start circle centered at (10,16) radius 4, end circle centered
    // at (22,16) radius 10 - definitely r0 != 0.
    paint_radial_gradient(&mut p, 10.0, 16.0, 4.0, 22.0, 16.0, 10.0, color_line).unwrap();

    let pixmap = p.into_pixmap().unwrap();
    // (4, 16) lies within the double-cone traced by the family of
    // circles interpolating between the start and end circle (t
    // solves to ~0.556, i.e. closer to the end/blue stop), so it
    // must be painted with a color, not left transparent.
    let inside_cone = pixmap.pixel(4, 16).unwrap();
    assert!(
        inside_cone.alpha() > 0,
        "point inside the gradient's cone should be painted: {:?}",
        inside_cone
    );
    // Note: unlike a simple single-circle radial gradient, a general
    // two-radius conical gradient has points with *no* valid
    // interpolation parameter at all (e.g. (0, 0) here, where the
    // quadratic's discriminant is negative) - those are correctly
    // left transparent even under `Extend::Pad`, matching
    // cairo/Skia/CSS's `radial-gradient()` behavior; `Pad` only
    // clamps an out-of-range `t`, it doesn't manufacture one where
    // none exists.
}

#[test]
fn radial_gradient_concentric_two_radius_ramps_in_correct_direction() {
    // Regression test for BUG5 (see the migration plan / TaskList): a
    // concentric two-radius conical gradient (same center, r0 != 0,
    // as COLRv1 fonts commonly use for a soft radial highlight, e.g.
    // Noto Color Emoji's U+1F600 face) used to have the sign of `b`
    // flipped in `ConicalGradient::color_at`'s quadratic solve, which
    // silently negated the solved `t` and inverted the whole ramp:
    // the *last* color stop got painted at the center (and near it)
    // instead of the *first* one. This does not hit tiny-skia's
    // native `RadialGradient` fast path (which requires `rr0 == 0`),
    // so it must go through `ConicalGradient`.
    let mut p = Painter::new(64, 64).unwrap();
    let color_line = ColorLine {
        color_stops: vec![stop(0.0, 255, 255, 0, 255), stop(1.0, 0, 0, 255, 255)],
        extend: SpreadMode::Pad,
    };
    p.new_path();
    p.move_to(0., 0.);
    p.line_to(64., 0.);
    p.line_to(64., 64.);
    p.line_to(0., 64.);
    p.close_path();
    p.clip();
    // Same center (32,32) for both circles, r0=16 (first/yellow
    // stop) and r1=32 (second/blue stop) - r0 != 0, so this must go
    // through the ConicalGradient fallback, not the native shader.
    paint_radial_gradient(&mut p, 32.0, 32.0, 16.0, 32.0, 32.0, 32.0, color_line).unwrap();

    let pixmap = p.into_pixmap().unwrap();
    // The center (well inside r0) must clamp (Pad) to t=0, i.e. the
    // *first* (yellow) stop - this is exactly the pixel the sign bug
    // got wrong (it painted the *last*/blue stop there instead).
    let center = pixmap.pixel(32, 32).unwrap();
    assert!(
        center.red() > 200 && center.green() > 200 && center.blue() < 60,
        "center (inside r0) should clamp to the first (yellow) stop: {:?}",
        center
    );
    // A point beyond r1 (Pad) must clamp to t=1, i.e. the *last*
    // (blue) stop.
    let outside = pixmap.pixel(63, 32).unwrap();
    assert!(
        outside.blue() > 200 && outside.red() < 60,
        "point beyond r1 should clamp to the last (blue) stop: {:?}",
        outside
    );
    // Halfway between r0 and r1 (radius 24 from center, e.g. straight
    // right of center) should be roughly an even blend, i.e. neither
    // pure yellow nor pure blue.
    let mid = pixmap.pixel(56, 32).unwrap();
    assert!(
        mid.red() > 40 && mid.blue() > 40,
        "halfway between r0 and r1 should be a blend of both stops: {:?}",
        mid
    );
}

#[test]
fn sweep_gradient_start_and_end_angles_match_stops() {
    let mut p = Painter::new(64, 64).unwrap();
    let color_line = ColorLine {
        color_stops: vec![stop(0.0, 255, 0, 0, 255), stop(1.0, 0, 255, 0, 255)],
        extend: SpreadMode::Pad,
    };
    p.new_path();
    p.move_to(0., 0.);
    p.line_to(64., 0.);
    p.line_to(64., 64.);
    p.line_to(0., 64.);
    p.close_path();
    p.clip();

    let cx = 32.0;
    let cy = 32.0;
    paint_sweep_gradient(&mut p, cx, cy, 0.0, std::f64::consts::PI / 2.0, color_line).unwrap();

    let pixmap = p.into_pixmap().unwrap();
    // angle 0 (start_angle): point directly to the +x of center.
    let start_pt = pixmap.pixel(63, 32).unwrap();
    // angle PI/2 (end_angle): point directly to the +y (down, since
    // our angle convention matches atan2(dy, dx) with y growing
    // downward in pixel space) of center.
    let end_pt = pixmap.pixel(32, 63).unwrap();

    assert!(
        start_pt.red() > 200,
        "start angle should be ~red: {:?}",
        start_pt
    );
    assert!(
        end_pt.green() > 200,
        "end angle should be ~green: {:?}",
        end_pt
    );
}

#[test]
fn sweep_gradient_full_circle_wraps() {
    let mut p = Painter::new(64, 64).unwrap();
    let color_line = ColorLine {
        color_stops: vec![stop(0.0, 255, 0, 0, 255), stop(1.0, 255, 0, 0, 255)],
        extend: SpreadMode::Pad,
    };
    p.new_path();
    p.move_to(0., 0.);
    p.line_to(64., 0.);
    p.line_to(64., 64.);
    p.line_to(0., 64.);
    p.close_path();
    p.clip();
    paint_sweep_gradient(&mut p, 32.0, 32.0, 0.0, PI_TIMES_2, color_line).unwrap();

    let pixmap = p.into_pixmap().unwrap();
    // Every angle should land on ~red since both stops are red.
    for (x, y) in [(63, 32), (32, 63), (0, 32), (32, 0)] {
        let px = pixmap.pixel(x, y).unwrap();
        assert!(px.red() > 200, "({x},{y}) should be ~red: {:?}", px);
    }
}

#[test]
fn draw_ops_produce_nonempty_bbox() {
    let mut p = Painter::new_dry_run();
    let ops = vec![
        DrawOp::MoveTo { to_x: 5., to_y: 5. },
        DrawOp::LineTo {
            to_x: 25.,
            to_y: 5.,
        },
        DrawOp::LineTo {
            to_x: 25.,
            to_y: 25.,
        },
        DrawOp::QuadTo {
            control_x: 15.,
            control_y: 40.,
            to_x: 5.,
            to_y: 25.,
        },
        DrawOp::ClosePath,
    ];
    apply_draw_ops_to_context(&ops, &mut p).unwrap();
    let bbox = p.current_path_bbox();
    assert!(
        !bbox.is_empty(),
        "bbox should not be empty for a nonempty path"
    );
    assert_eq!(bbox.x0, 5.);
    assert_eq!(bbox.y0, 5.);
    assert_eq!(bbox.x1, 25.);
    // y1 comes from the quad_to control point (40), which is a
    // conservative (but correct) superset of the true curve extent.
    assert_eq!(bbox.y1, 40.);
}

#[test]
fn draw_ops_empty_produces_empty_bbox() {
    let mut p = Painter::new_dry_run();
    apply_draw_ops_to_context(&[], &mut p).unwrap();
    assert!(p.current_path_bbox().is_empty());
}
