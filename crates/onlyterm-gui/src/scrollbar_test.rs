use super::normalize_viewport;

#[test]
fn wheel_at_empty_scrollback_keeps_following_bottom() {
    assert_eq!(normalize_viewport(Some(-3), 0, 0), None);
    assert_eq!(normalize_viewport(Some(997), 1000, 1000), None);
    let after_alternate_screen_wheel = normalize_viewport(Some(-3), 0, 0);
    assert_eq!(after_alternate_screen_wheel.unwrap_or(8000), 8000);
}

#[test]
fn scrollback_boundaries_preserve_explicit_history_positions() {
    assert_eq!(normalize_viewport(Some(997), 100, 1000), Some(997));
    assert_eq!(normalize_viewport(Some(97), 100, 1000), Some(100));
    assert_eq!(normalize_viewport(Some(1000), 100, 1000), None);
    assert_eq!(normalize_viewport(Some(isize::MIN), 100, 1000), Some(100));
    assert_eq!(normalize_viewport(None, 100, 1000), None);
}
