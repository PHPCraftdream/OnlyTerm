use super::*;

// Fields order matches the format string in ListAllPanes::get_command:
// session_id window_id pane_id pane_index cursor_x cursor_y
// pane_width pane_height pane_left pane_top pane_active
// mouse_any_flag mouse_button_flag
#[test]
fn parse_pane_line_any_mouse() {
    let line = "$1 @2 %3 0 4 5 80 24 0 0 1 1 0";
    let pane = parse_pane_line(line).expect("parse ok");
    assert_eq!(pane.session_id, 1);
    assert_eq!(pane.window_id, 2);
    assert_eq!(pane.pane_id, 3);
    assert_eq!(pane.cursor_x, 4);
    assert_eq!(pane.cursor_y, 5);
    assert_eq!(pane.pane_width, 80);
    assert_eq!(pane.pane_height, 24);
    assert!(pane.pane_active);
    assert!(pane.pane_mouse_any);
    assert!(!pane.pane_mouse_buttton);
}

#[test]
fn parse_pane_line_no_mouse() {
    let line = "$1 @2 %3 0 0 0 80 24 0 0 0 0 0";
    let pane = parse_pane_line(line).expect("parse ok");
    assert!(!pane.pane_active);
    assert!(!pane.pane_mouse_any);
    assert!(!pane.pane_mouse_buttton);
}

#[test]
fn parse_pane_line_button_mouse() {
    let line = "$10 @20 %30 2 7 8 100 40 5 6 1 0 1";
    let pane = parse_pane_line(line).expect("parse ok");
    assert_eq!(pane.pane_left, 5);
    assert_eq!(pane.pane_top, 6);
    assert!(pane.pane_active);
    assert!(!pane.pane_mouse_any);
    assert!(pane.pane_mouse_buttton);
}

#[test]
fn parse_pane_line_missing_mouse_flag_errors() {
    // Trailing mouse flags omitted -> parse must fail rather than silently
    // defaulting to false.
    let line = "$1 @2 %3 0 0 0 80 24 0 0 1";
    assert!(parse_pane_line(line).is_err());
}
