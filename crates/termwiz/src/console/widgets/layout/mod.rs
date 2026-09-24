//! This module provides some automatic layout functionality for widgets.
//! The parameters are similar to those that you may have encountered
//! in HTML, but do not fully recreate the layout model.

mod constraints;
mod solver;

pub use constraints::*;
pub use solver::*;

use crate::widgets::{Rect, WidgetId};

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn single_widget_unspec() {
        let mut layout = LayoutState::new();
        let main_id = WidgetId::new();
        layout.add_widget(main_id, &Constraints::default(), &[]);
        let results = layout.compute_constraints(40, 12, main_id).unwrap();

        assert_eq!(
            results,
            vec![LaidOutWidget {
                widget: main_id,
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 12,
                },
            }]
        );
    }

    #[test]
    fn two_children_pct() {
        let root = WidgetId::new();
        let a = WidgetId::new();
        let b = WidgetId::new();

        let mut layout = LayoutState::new();
        layout.add_widget(root, &Constraints::default(), &[a, b]);
        layout.add_widget(a, Constraints::default().set_pct_width(50), &[]);
        layout.add_widget(b, Constraints::default().set_pct_width(50), &[]);

        let results = layout.compute_constraints(100, 100, root).unwrap();

        assert_eq!(
            results,
            vec![
                LaidOutWidget {
                    widget: root,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    },
                },
                LaidOutWidget {
                    widget: a,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 50,
                        height: 100,
                    },
                },
                LaidOutWidget {
                    widget: b,
                    rect: Rect {
                        x: 50,
                        y: 0,
                        width: 50,
                        height: 100,
                    },
                },
            ]
        );
    }

    #[test]
    fn three_children_pct() {
        let root = WidgetId::new();
        let a = WidgetId::new();
        let b = WidgetId::new();
        let c = WidgetId::new();

        let mut layout = LayoutState::new();
        layout.add_widget(root, &Constraints::default(), &[a, b, c]);
        layout.add_widget(a, Constraints::default().set_pct_width(20), &[]);
        layout.add_widget(b, Constraints::default().set_pct_width(20), &[]);
        layout.add_widget(c, Constraints::default().set_pct_width(20), &[]);

        let results = layout.compute_constraints(100, 100, root).unwrap();

        assert_eq!(
            results,
            vec![
                LaidOutWidget {
                    widget: root,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    },
                },
                LaidOutWidget {
                    widget: a,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 20,
                        height: 100,
                    },
                },
                LaidOutWidget {
                    widget: b,
                    rect: Rect {
                        x: 20,
                        y: 0,
                        width: 20,
                        height: 100,
                    },
                },
                LaidOutWidget {
                    widget: c,
                    rect: Rect {
                        x: 40,
                        y: 0,
                        width: 20,
                        height: 100,
                    },
                },
            ]
        );
    }

    #[test]
    fn two_children_a_b() {
        let root = WidgetId::new();
        let a = WidgetId::new();
        let b = WidgetId::new();

        let mut layout = LayoutState::new();
        layout.add_widget(root, &Constraints::default(), &[a, b]);
        layout.add_widget(a, &Constraints::with_fixed_width_height(5, 2), &[]);
        layout.add_widget(b, &Constraints::with_fixed_width_height(3, 2), &[]);

        let results = layout.compute_constraints(100, 100, root).unwrap();

        assert_eq!(
            results,
            vec![
                LaidOutWidget {
                    widget: root,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    },
                },
                LaidOutWidget {
                    widget: a,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 5,
                        height: 2,
                    },
                },
                LaidOutWidget {
                    widget: b,
                    rect: Rect {
                        x: 5,
                        y: 0,
                        width: 3,
                        height: 2,
                    },
                },
            ]
        );
    }

    #[test]
    fn two_children_b_a() {
        let root = WidgetId::new();
        let a = WidgetId::new();
        let b = WidgetId::new();

        let mut layout = LayoutState::new();
        layout.add_widget(root, &Constraints::default(), &[b, a]);
        layout.add_widget(a, &Constraints::with_fixed_width_height(5, 2), &[]);
        layout.add_widget(b, &Constraints::with_fixed_width_height(3, 2), &[]);

        let results = layout.compute_constraints(100, 100, root).unwrap();

        assert_eq!(
            results,
            vec![
                LaidOutWidget {
                    widget: root,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    },
                },
                LaidOutWidget {
                    widget: b,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 3,
                        height: 2,
                    },
                },
                LaidOutWidget {
                    widget: a,
                    rect: Rect {
                        x: 3,
                        y: 0,
                        width: 5,
                        height: 2,
                    },
                },
            ]
        );
    }

    #[test]
    fn two_children_overflow() {
        let root = WidgetId::new();
        let a = WidgetId::new();
        let b = WidgetId::new();

        let mut layout = LayoutState::new();
        layout.add_widget(root, &Constraints::default(), &[a, b]);
        layout.add_widget(a, &Constraints::with_fixed_width_height(5, 2), &[]);
        layout.add_widget(b, &Constraints::with_fixed_width_height(3, 2), &[]);

        let results = layout.compute_constraints(6, 2, root).unwrap();

        assert_eq!(
            results,
            vec![
                LaidOutWidget {
                    widget: root,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 8,
                        height: 2,
                    },
                },
                LaidOutWidget {
                    widget: a,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 5,
                        height: 2,
                    },
                },
                LaidOutWidget {
                    widget: b,
                    rect: Rect {
                        x: 5,
                        y: 0,
                        width: 3,
                        height: 2,
                    },
                },
            ]
        );
    }

    macro_rules! single_constrain {
        ($name:ident, $constraint:expr, $width:expr, $height:expr, $x:expr, $y:expr) => {
            #[test]
            fn $name() {
                let root = WidgetId::new();
                let mut layout = LayoutState::new();
                layout.add_widget(root, &$constraint, &[]);

                let results = layout.compute_constraints(100, 100, root).unwrap();

                assert_eq!(
                    results,
                    vec![LaidOutWidget {
                        widget: root,
                        rect: Rect {
                            x: $x,
                            y: $y,
                            width: $width,
                            height: $height,
                        },
                    }]
                );
            }
        };
    }

    single_constrain!(
        single_constrained_widget_top,
        Constraints::with_fixed_width_height(10, 2),
        10,
        2,
        0,
        0
    );

    single_constrain!(
        single_constrained_widget_bottom,
        Constraints::with_fixed_width_height(10, 2)
            .set_valign(VerticalAlignment::Bottom)
            .clone(),
        10,
        2,
        0,
        98
    );

    single_constrain!(
        single_constrained_widget_middle,
        Constraints::with_fixed_width_height(10, 2)
            .set_valign(VerticalAlignment::Middle)
            .clone(),
        10,
        2,
        0,
        49
    );

    single_constrain!(
        single_constrained_widget_right,
        Constraints::with_fixed_width_height(10, 2)
            .set_halign(HorizontalAlignment::Right)
            .clone(),
        10,
        2,
        90,
        0
    );

    single_constrain!(
        single_constrained_widget_bottom_center,
        Constraints::with_fixed_width_height(10, 2)
            .set_valign(VerticalAlignment::Bottom)
            .set_halign(HorizontalAlignment::Center)
            .clone(),
        10,
        2,
        45,
        98
    );
}
