use super::*;

#[test]
fn bbox_of_simple_rect_path() {
    let mut p = Painter::new_dry_run();
    p.move_to(10., 20.);
    p.line_to(110., 20.);
    p.line_to(110., 70.);
    p.line_to(10., 70.);
    p.close_path();

    let bbox = p.current_path_bbox();
    assert_eq!(bbox.x0, 10.);
    assert_eq!(bbox.y0, 20.);
    assert_eq!(bbox.x1, 110.);
    assert_eq!(bbox.y1, 70.);
    assert_eq!(bbox.width(), 100.);
    assert_eq!(bbox.height(), 50.);
}

#[test]
fn bbox_curve_is_conservative_hull_of_control_points() {
    let mut p = Painter::new_dry_run();
    p.move_to(0., 0.);
    // control points stick out well beyond the actual curve
    p.curve_to(50., -100., 150., -100., 100., 0.);
    let bbox = p.current_path_bbox();
    // hull must contain all 4 points: (0,0) (50,-100) (150,-100) (100,0)
    assert_eq!(bbox.x0, 0.);
    assert_eq!(bbox.y0, -100.);
    assert_eq!(bbox.x1, 150.);
    assert_eq!(bbox.y1, 0.);
}

#[test]
fn dry_run_paint_solid_extends_bbox_to_clip_extent() {
    let mut p = Painter::new_dry_run();
    p.width = 64;
    p.height = 64;
    p.save();
    p.move_to(5., 5.);
    p.line_to(20., 5.);
    p.line_to(20., 20.);
    p.line_to(5., 20.);
    p.close_path();
    p.clip();
    p.paint_solid(Color::from_rgba8(255, 0, 0, 255));
    p.restore();

    let bbox = p.bbox();
    assert_eq!(bbox.x0, 5.);
    assert_eq!(bbox.y0, 5.);
    assert_eq!(bbox.x1, 20.);
    assert_eq!(bbox.y1, 20.);
}

#[test]
fn save_restore_preserves_transform() {
    let mut p = Painter::new(8, 8).unwrap();
    p.translate(3., 4.);
    p.save();
    p.scale(2., 2.);
    p.translate(1., 1.);
    let t_inside = p.current_transform();
    p.restore();
    let t_after = p.current_transform();

    assert_eq!(t_after.tx, 3.);
    assert_eq!(t_after.ty, 4.);
    assert_eq!(t_after.sx, 1.);
    assert_eq!(t_after.sy, 1.);

    // sanity: the transform inside the save/restore pair really was
    // different, i.e. restore did something.
    assert_ne!(t_inside, t_after);
}

#[test]
fn nested_save_restore_stack_order() {
    let mut p = Painter::new(8, 8).unwrap();
    p.translate(1., 0.);
    p.save();
    p.translate(2., 0.);
    p.save();
    p.translate(4., 0.);
    assert_eq!(p.current_transform().tx, 7.);
    p.restore();
    assert_eq!(p.current_transform().tx, 3.);
    p.restore();
    assert_eq!(p.current_transform().tx, 1.);
}

fn fill_whole_pixmap(painter: &mut Painter, color: Color) {
    painter.save();
    painter.new_path();
    painter.move_to(0., 0.);
    painter.line_to(painter.width as f32, 0.);
    painter.line_to(painter.width as f32, painter.height as f32);
    painter.line_to(0., painter.height as f32);
    painter.close_path();
    painter.clip();
    painter.paint_solid(color);
    painter.restore();
}

#[test]
fn group_composite_over_blend_mode() {
    // Blue background, then a group containing a red fill composited
    // with BlendMode::SourceOver should just show red (SourceOver
    // fully replaces where source alpha is 1).
    let mut p = Painter::new(4, 4).unwrap();
    fill_whole_pixmap(&mut p, Color::from_rgba8(0, 0, 255, 255));

    p.push_group();
    fill_whole_pixmap(&mut p, Color::from_rgba8(255, 0, 0, 255));
    p.pop_group(BlendMode::SourceOver);

    let pixmap = p.into_pixmap().unwrap();
    let pixel = pixmap.pixel(0, 0).unwrap();
    assert_eq!((pixel.red(), pixel.green(), pixel.blue()), (255, 0, 0));
}

#[test]
fn group_composite_multiply_blend_mode() {
    // Red (255,0,0) background; group with (0,255,0) green painted
    // using BlendMode::Multiply should produce black (each channel
    // multiplies to 0 except where both are non-zero).
    let mut p = Painter::new(4, 4).unwrap();
    fill_whole_pixmap(&mut p, Color::from_rgba8(255, 0, 0, 255));

    p.push_group();
    fill_whole_pixmap(&mut p, Color::from_rgba8(0, 255, 0, 255));
    p.pop_group(BlendMode::Multiply);

    let pixmap = p.into_pixmap().unwrap();
    let pixel = pixmap.pixel(0, 0).unwrap();
    assert_eq!((pixel.red(), pixel.green(), pixel.blue()), (0, 0, 0));
}

#[test]
fn premultiplied_output_matches_rasterized_glyph_expectations() {
    // RasterizedGlyph::data is documented as premultiplied RGBA.
    // tiny-skia's Pixmap always stores premultiplied color; verify a
    // half-transparent red fill comes back premultiplied (R <= A).
    let mut p = Painter::new(2, 2).unwrap();
    fill_whole_pixmap(&mut p, Color::from_rgba8(255, 0, 0, 128));
    let pixmap = p.into_pixmap().unwrap();
    let pixel = pixmap.pixel(0, 0).unwrap();
    assert_eq!(pixel.alpha(), 128);
    // premultiplied red at alpha=128 must be roughly 128, definitely
    // not 255 (which would indicate un-premultiplied storage).
    assert!(pixel.red() <= 129);
    assert!(pixel.red() > 0);
}
