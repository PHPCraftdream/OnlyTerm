use super::*;
use crate::alloc::string::ToString;
#[cfg(feature = "use_image")]
use crate::change::{Image, TextureCoordinate};
#[cfg(feature = "use_image")]
use alloc::sync::Arc;
use onlyterm_cell::color::AnsiColor;
#[cfg(feature = "use_image")]
use onlyterm_cell::image::ImageCell;
#[cfg(feature = "use_image")]
use onlyterm_cell::image::ImageData;
use onlyterm_cell::{AttributeChange, Intensity};

// The \x20's look a little awkward, but we can't use a plain
// space in the first chararcter of a multi-line continuation;
// it gets eaten up and ignored.

#[test]
fn basic_print() {
    let mut s = Surface::new(4, 3);
    assert_eq!(
        s.screen_chars_to_string(),
        "\x20\x20\x20\x20\n\
         \x20\x20\x20\x20\n\
         \x20\x20\x20\x20\n"
    );

    s.add_change("w00t");
    assert_eq!(
        s.screen_chars_to_string(),
        "w00t\n\
         \x20\x20\x20\x20\n\
         \x20\x20\x20\x20\n"
    );

    s.add_change("foo");
    assert_eq!(
        s.screen_chars_to_string(),
        "w00t\n\
         foo\x20\n\
         \x20\x20\x20\x20\n"
    );

    s.add_change("baar");
    assert_eq!(
        s.screen_chars_to_string(),
        "w00t\n\
         foob\n\
         aar\x20\n"
    );

    s.add_change("baz");
    assert_eq!(
        s.screen_chars_to_string(),
        "foob\n\
         aarb\n\
         az\x20\x20\n"
    );
}

#[test]
fn newline() {
    let mut s = Surface::new(4, 4);
    s.add_change("bloo\rwat\n hey\r\nho");
    assert_eq!(
        s.screen_chars_to_string(),
        "wato\n\
         \x20\x20\x20\x20\n\
         hey \n\
         ho  \n"
    );
}

#[test]
fn clear_screen() {
    let mut s = Surface::new(2, 2);
    s.add_change("hello");
    assert_eq!(s.xpos, 1);
    assert_eq!(s.ypos, 1);
    s.add_change(Change::ClearScreen(Default::default()));
    assert_eq!(s.xpos, 0);
    assert_eq!(s.ypos, 0);
    assert_eq!(s.screen_chars_to_string(), "  \n  \n");
}

#[test]
fn clear_eol() {
    let mut s = Surface::new(3, 3);
    s.add_change("helwowfoo");
    s.add_change(Change::ClearToEndOfLine(Default::default()));
    assert_eq!(s.screen_chars_to_string(), "hel\nwow\nfoo\n");
    s.add_change(Change::CursorPosition {
        x: Position::Absolute(0),
        y: Position::Absolute(0),
    });
    s.add_change(Change::ClearToEndOfLine(Default::default()));
    assert_eq!(s.screen_chars_to_string(), "   \nwow\nfoo\n");
    s.add_change(Change::CursorPosition {
        x: Position::Absolute(1),
        y: Position::Absolute(1),
    });
    s.add_change(Change::ClearToEndOfLine(Default::default()));
    assert_eq!(s.screen_chars_to_string(), "   \nw\nfoo\n");
}

#[test]
fn clear_eos() {
    let mut s = Surface::new(3, 3);
    s.add_change("helwowfoo");
    s.add_change(Change::ClearToEndOfScreen(Default::default()));
    assert_eq!(s.screen_chars_to_string(), "hel\nwow\nfoo\n");
    s.add_change(Change::CursorPosition {
        x: Position::Absolute(1),
        y: Position::Absolute(1),
    });
    s.add_change(Change::ClearToEndOfScreen(Default::default()));
    assert_eq!(s.screen_chars_to_string(), "hel\nw\n   \n");

    let (_seq, changes) = s.get_changes(0);
    assert_eq!(
        &[
            Change::CursorVisibility(CursorVisibility::Hidden),
            Change::ClearScreen(Default::default()),
            Change::Text("hel".into()),
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(1),
            },
            Change::Text("w".into()),
            Change::CursorPosition {
                x: Position::Absolute(1),
                y: Position::Absolute(1),
            },
            Change::CursorVisibility(CursorVisibility::Visible),
        ],
        &*changes
    );
}

#[test]
fn clear_eos_back_color() {
    let mut s = Surface::new(3, 3);
    s.add_change(Change::ClearScreen(AnsiColor::Red.into()));
    s.add_change("helwowfoo");
    assert_eq!(s.screen_chars_to_string(), "hel\nwow\nfoo\n");
    s.add_change(Change::CursorPosition {
        x: Position::Absolute(1),
        y: Position::Absolute(1),
    });
    s.add_change(Change::ClearToEndOfScreen(AnsiColor::Red.into()));
    assert_eq!(s.screen_chars_to_string(), "hel\nw  \n   \n");

    let (_seq, changes) = s.get_changes(0);
    assert_eq!(
        &[
            Change::CursorVisibility(CursorVisibility::Hidden),
            Change::ClearScreen(Default::default()),
            Change::AllAttributes(
                CellAttributes::default()
                    .set_background(AnsiColor::Red)
                    .clone()
            ),
            Change::Text("hel".into()),
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(1),
            },
            Change::Text("w".into()),
            Change::ClearToEndOfScreen(AnsiColor::Red.into()),
            Change::CursorPosition {
                x: Position::Absolute(1),
                y: Position::Absolute(1),
            },
            Change::CursorVisibility(CursorVisibility::Visible),
        ],
        &*changes
    );
}

#[test]
fn clear_eol_opt() {
    let mut s = Surface::new(3, 3);
    s.add_change(Change::Attribute(AttributeChange::Background(
        AnsiColor::Red.into(),
    )));
    s.add_change("111   333");
    let (_seq, changes) = s.get_changes(0);
    assert_eq!(
        &[
            Change::CursorVisibility(CursorVisibility::Hidden),
            Change::ClearScreen(Default::default()),
            Change::AllAttributes(
                CellAttributes::default()
                    .set_background(AnsiColor::Red)
                    .clone()
            ),
            Change::Text("111".into()),
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(1),
            },
            Change::ClearToEndOfLine(AnsiColor::Red.into()),
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(1),
            },
            Change::Text("333".into()),
            Change::CursorPosition {
                x: Position::Absolute(3),
                y: Position::Absolute(2),
            },
            Change::CursorVisibility(CursorVisibility::Visible),
        ],
        &*changes
    );
}

#[test]
fn clear_and_move_cursor() {
    let mut s = Surface::new(4, 3);
    s.add_change(Change::CursorPosition {
        x: Position::Absolute(3),
        y: Position::Absolute(2),
    });
    let (_seq, changes) = s.get_changes(0);
    assert_eq!(
        &[
            Change::CursorVisibility(CursorVisibility::Hidden),
            Change::ClearScreen(Default::default()),
            Change::CursorPosition {
                x: Position::Absolute(3),
                y: Position::Absolute(2),
            },
            Change::CursorVisibility(CursorVisibility::Visible),
        ],
        &*changes
    );
}

#[test]
fn cursor_movement() {
    let mut s = Surface::new(4, 3);
    s.add_change(Change::CursorPosition {
        x: Position::Absolute(3),
        y: Position::Absolute(2),
    });
    s.add_change("X");
    assert_eq!(
        s.screen_chars_to_string(),
        "\x20\x20\x20\x20\n\
         \x20\x20\x20\x20\n\
         \x20\x20\x20X\n"
    );

    s.add_change(Change::CursorPosition {
        x: Position::Relative(-2),
        y: Position::Relative(-1),
    });
    s.add_change("-");
    assert_eq!(
        s.screen_chars_to_string(),
        "\x20\x20\x20\x20\n\
         \x20\x20-\x20\n\
         \x20\x20\x20X\n"
    );

    s.add_change(Change::CursorPosition {
        x: Position::Relative(1),
        y: Position::Relative(-1),
    });
    s.add_change("-");
    assert_eq!(
        s.screen_chars_to_string(),
        "\x20\x20\x20-\n\
         \x20\x20-\x20\n\
         \x20\x20\x20X\n"
    );
}

#[test]
fn attribute_setting() {
    use onlyterm_cell::Intensity;

    let mut s = Surface::new(3, 1);
    s.add_change("n");
    s.add_change(AttributeChange::Intensity(Intensity::Bold));
    s.add_change("b");

    let mut bold = CellAttributes::default();
    bold.set_intensity(Intensity::Bold);

    assert_eq!(
        s.screen_cells(),
        [[
            Cell::new('n', CellAttributes::default()),
            Cell::new('b', bold),
            Cell::default(),
        ]]
    );
}

#[test]
fn empty_changes() {
    let s = Surface::new(4, 3);

    let empty = &[
        Change::CursorVisibility(CursorVisibility::Hidden),
        Change::ClearScreen(Default::default()),
        Change::CursorVisibility(CursorVisibility::Visible),
    ];

    let (seq, changes) = s.get_changes(0);
    assert_eq!(seq, 0);
    assert_eq!(empty, &*changes);

    // Using an invalid sequence number should get us the full
    // repaint also.
    let (seq, changes) = s.get_changes(1);
    assert_eq!(seq, 0);
    assert_eq!(empty, &*changes);
}

#[test]
fn add_changes_empty() {
    let mut s = Surface::new(2, 2);
    let last_seq = s.add_change("foo");
    assert_eq!(0, last_seq);
    assert_eq!(last_seq, s.add_changes(vec![]));
    assert_eq!(last_seq + 1, s.add_changes(vec![Change::Text("a".into())]));
}

#[test]
fn resize_delta_flush() {
    let mut s = Surface::new(4, 3);
    s.add_change("a");
    let (seq, _) = s.get_changes(0);
    s.resize(2, 2);

    let full = &[
        Change::CursorVisibility(CursorVisibility::Hidden),
        Change::ClearScreen(Default::default()),
        Change::Text("a".to_string()),
        Change::CursorPosition {
            x: Position::Absolute(1),
            y: Position::Absolute(0),
        },
        Change::CursorVisibility(CursorVisibility::Visible),
    ];

    let (_seq, changes) = s.get_changes(seq);
    // The resize causes get_changes to return a full repaint
    assert_eq!(full, &*changes);
}

#[test]
fn dont_lose_first_char_on_attr_change() {
    let mut s = Surface::new(2, 2);
    s.add_change(Change::Attribute(AttributeChange::Foreground(
        AnsiColor::Maroon.into(),
    )));
    s.add_change("ab");
    let (_seq, changes) = s.get_changes(0);
    assert_eq!(
        &[
            Change::CursorVisibility(CursorVisibility::Hidden),
            Change::ClearScreen(Default::default()),
            Change::AllAttributes(
                CellAttributes::default()
                    .set_foreground(AnsiColor::Maroon)
                    .clone()
            ),
            Change::Text("ab".into()),
            Change::CursorPosition {
                x: Position::Absolute(2),
                y: Position::Absolute(0),
            },
            Change::CursorVisibility(CursorVisibility::Visible),
        ],
        &*changes
    );
}

#[test]
fn resize_cursor_position() {
    let mut s = Surface::new(4, 4);

    s.add_change(" a");
    s.add_change(Change::CursorPosition {
        x: Position::Absolute(3),
        y: Position::Absolute(3),
    });

    assert_eq!(s.xpos, 3);
    assert_eq!(s.ypos, 3);
    s.resize(2, 2);
    assert_eq!(s.xpos, 1);
    assert_eq!(s.ypos, 1);

    let full = &[
        Change::CursorVisibility(CursorVisibility::Hidden),
        Change::ClearScreen(Default::default()),
        Change::Text(" a".to_string()),
        Change::CursorPosition {
            x: Position::Absolute(1),
            y: Position::Absolute(1),
        },
        Change::CursorVisibility(CursorVisibility::Visible),
    ];

    let (_seq, changes) = s.get_changes(0);
    assert_eq!(full, &*changes);
}

#[test]
fn delta_change() {
    let mut s = Surface::new(4, 3);
    // flushing nothing should be a NOP
    s.flush_changes_older_than(0);

    // check that using an invalid index doesn't panic
    s.flush_changes_older_than(1);

    let initial = &[
        Change::CursorVisibility(CursorVisibility::Hidden),
        Change::ClearScreen(Default::default()),
        Change::Text("a".to_string()),
        Change::CursorPosition {
            x: Position::Absolute(1),
            y: Position::Absolute(0),
        },
        Change::CursorVisibility(CursorVisibility::Visible),
    ];

    let seq_pos = {
        let next_seq = s.add_change("a");
        let (seq, changes) = s.get_changes(0);
        assert_eq!(seq, next_seq + 1);
        assert_eq!(initial, &*changes);
        seq
    };

    let seq_pos = {
        let next_seq = s.add_change("b");
        let (seq, changes) = s.get_changes(seq_pos);
        assert_eq!(seq, next_seq + 1);
        assert_eq!(&[Change::Text("b".to_string())], &*changes);
        seq
    };

    // prep some deltas for the loop to test below
    {
        s.add_change(Change::Attribute(AttributeChange::Intensity(
            Intensity::Bold,
        )));
        s.add_change("c");
        s.add_change(Change::Attribute(AttributeChange::Intensity(
            Intensity::Normal,
        )));
        s.add_change("d");
    }

    // Do this three times to ennsure that the behavior is consistent
    // across multiple flush calls
    for _ in 0..3 {
        {
            let (_seq, changes) = s.get_changes(seq_pos);

            assert_eq!(
                &[
                    Change::Attribute(AttributeChange::Intensity(Intensity::Bold)),
                    Change::Text("c".to_string()),
                    Change::Attribute(AttributeChange::Intensity(Intensity::Normal)),
                    Change::Text("d".to_string()),
                ],
                &*changes
            );
        }

        // Flush the changes so that the next iteration is run on a pruned
        // set of changes.  It should not change the outcome of the body
        // of the loop.
        s.flush_changes_older_than(seq_pos);
    }
}

#[test]
fn diff_screens() {
    let mut s = Surface::new(4, 3);
    s.add_change("w00t");
    s.add_change("foo");
    s.add_change("baar");
    s.add_change("baz");
    assert_eq!(
        s.screen_chars_to_string(),
        "foob\n\
         aarb\n\
         az  \n"
    );

    let s2 = Surface::new(2, 2);

    {
        // We want to sample the top left corner
        let changes = s2.diff_region(0, 0, 2, 2, &s, 0, 0);
        assert_eq!(
            vec![
                Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(0),
                },
                Change::AllAttributes(CellAttributes::default()),
                Change::Text("fo".into()),
                Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(1),
                },
                Change::Text("aa".into()),
            ],
            changes
        );
    }

    // Throw in some attribute changes too
    s.add_change(Change::CursorPosition {
        x: Position::Absolute(1),
        y: Position::Absolute(1),
    });
    s.add_change(Change::Attribute(AttributeChange::Intensity(
        Intensity::Bold,
    )));
    s.add_change("XO");

    {
        let changes = s2.diff_region(0, 0, 2, 2, &s, 1, 1);
        assert_eq!(
            vec![
                Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(0),
                },
                Change::AllAttributes(
                    CellAttributes::default()
                        .set_intensity(Intensity::Bold)
                        .clone(),
                ),
                Change::Text("XO".into()),
                Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(1),
                },
                Change::AllAttributes(CellAttributes::default()),
                Change::Text("z".into()),
                /* There's no change for the final character
                 * position because it is a space in both regions. */
            ],
            changes
        );
    }
}

#[test]
fn draw_screens() {
    let mut s = Surface::new(4, 4);

    let mut s1 = Surface::new(2, 2);
    s1.add_change("1234");

    let mut s2 = Surface::new(2, 2);
    s2.add_change("XYZA");

    s.draw_from_screen(&s1, 0, 0);
    s.draw_from_screen(&s2, 2, 2);

    assert_eq!(
        s.screen_chars_to_string(),
        "12  \n\
         34  \n\
         \x20\x20XY\n\
         \x20\x20ZA\n"
    );
}

#[test]
fn draw_colored_region() {
    let mut dest = Surface::new(4, 4);
    dest.add_change("A");
    let mut src = Surface::new(2, 2);
    src.add_change(Change::ClearScreen(AnsiColor::Blue.into()));
    dest.draw_from_screen(&src, 2, 2);

    assert_eq!(
        dest.screen_chars_to_string(),
        "A   \n\
         \x20   \n\
         \x20   \n\
         \x20   \n"
    );

    let blue_space = Cell::new(
        ' ',
        CellAttributes::default()
            .set_background(AnsiColor::Blue)
            .clone(),
    );

    assert_eq!(
        dest.screen_cells(),
        [
            [
                Cell::new('A', CellAttributes::default()),
                Cell::default(),
                Cell::default(),
                Cell::default(),
            ],
            [
                Cell::default(),
                Cell::default(),
                Cell::default(),
                Cell::default(),
            ],
            [
                Cell::default(),
                Cell::default(),
                blue_space.clone(),
                blue_space.clone(),
            ],
            [
                Cell::default(),
                Cell::default(),
                blue_space.clone(),
                blue_space.clone(),
            ]
        ]
    );

    assert_eq!(dest.xpos, 1);
    assert_eq!(dest.ypos, 0);
    assert_eq!(dest.attributes, Default::default());
    dest.add_change("B");

    assert_eq!(
        dest.screen_chars_to_string(),
        "AB  \n\
         \x20   \n\
         \x20   \n\
         \x20   \n"
    );
}

#[test]
fn copy_region() {
    let mut s = Surface::new(4, 3);
    s.add_change("w00t");
    s.add_change("foo");
    s.add_change("baar");
    s.add_change("baz");
    assert_eq!(
        s.screen_chars_to_string(),
        "foob\n\
         aarb\n\
         az  \n"
    );

    // Copy top left to bottom left
    s.copy_region(0, 0, 2, 2, 2, 1);
    assert_eq!(
        s.screen_chars_to_string(),
        "foob\n\
         aafo\n\
         azaa\n"
    );
}

#[test]
fn double_width() {
    let mut s = Surface::new(4, 1);
    s.add_change("🤷12");
    assert_eq!(s.screen_chars_to_string(), "🤷12\n");
    s.add_change(Change::CursorPosition {
        x: Position::Absolute(1),
        y: Position::Absolute(0),
    });
    s.add_change("a🤷");
    assert_eq!(s.screen_chars_to_string(), " a🤷\n");
    s.add_change(Change::CursorPosition {
        x: Position::Absolute(2),
        y: Position::Absolute(0),
    });
    s.add_change("x");
    assert_eq!(s.screen_chars_to_string(), " ax \n");
}

#[test]
fn draw_double_width() {
    let mut s = Surface::new(4, 1);
    s.add_change("か a");
    assert_eq!(s.screen_chars_to_string(), "か a\n");

    let mut s2 = Surface::new(4, 1);
    s2.draw_from_screen(&s, 0, 0);
    // Verify no issue when the second visible cells on both sides
    // are identical (' 's) but they are at different cell indices.
    assert_eq!(s2.screen_chars_to_string(), "か a\n");

    let s3 = Surface::new(4, 1);
    s2.draw_from_screen(&s3, 0, 0);
    // Verify same but in other direction
    assert_eq!(s2.screen_chars_to_string(), "    \n");

    let mut s4 = Surface::new(4, 1);
    s4.add_change("abcd");
    s.draw_from_screen(&s4, 0, 0);
    // Verify that all overlapping cells are updated when cell widths
    // differ on each side.
    assert_eq!(s.screen_chars_to_string(), "abcd\n");
}

#[test]
fn diff_cursor_double_width() {
    let mut s = Surface::new(3, 1);
    s.add_change("かa");

    let s2 = Surface::new(3, 1);
    let changes = s2.diff_region(0, 0, 3, 1, &s, 0, 0);

    assert_eq!(
        changes
            .iter()
            .filter(|change| matches!(change, Change::CursorPosition { .. }))
            .count(),
        1
    );
}

#[test]
fn zero_width() {
    let mut s = Surface::new(4, 1);
    // https://en.wikipedia.org/wiki/Zero-width_space
    s.add_change("A\u{200b}B");
    assert_eq!(s.screen_chars_to_string(), "A\u{200b}B \n");
}

#[cfg(feature = "use_image")]
#[test]
fn images() {
    // a dummy image blob with nonsense content
    let data = Arc::new(ImageData::with_raw_data(vec![]));
    let mut s = Surface::new(2, 2);
    s.add_change(Change::Image(Image {
        top_left: TextureCoordinate::new_f32(0.0, 0.0),
        bottom_right: TextureCoordinate::new_f32(1.0, 1.0),
        image: data.clone(),
        width: 4,
        height: 2,
    }));

    // We're checking that we slice the image up and assign the correct
    // texture coordinates for each cell.  The width and height are
    // different from each other to help ensure that the right terms
    // are used by add_image() function.
    assert_eq!(
        s.screen_cells(),
        [
            [
                Cell::new(
                    ' ',
                    CellAttributes::default()
                        .set_image(Box::new(ImageCell::new(
                            TextureCoordinate::new_f32(0.0, 0.0),
                            TextureCoordinate::new_f32(0.25, 0.5),
                            data.clone()
                        )))
                        .clone()
                ),
                Cell::new(
                    ' ',
                    CellAttributes::default()
                        .set_image(Box::new(ImageCell::new(
                            TextureCoordinate::new_f32(0.25, 0.0),
                            TextureCoordinate::new_f32(0.5, 0.5),
                            data.clone()
                        )))
                        .clone()
                ),
                Cell::new(
                    ' ',
                    CellAttributes::default()
                        .set_image(Box::new(ImageCell::new(
                            TextureCoordinate::new_f32(0.5, 0.0),
                            TextureCoordinate::new_f32(0.75, 0.5),
                            data.clone()
                        )))
                        .clone()
                ),
                Cell::new(
                    ' ',
                    CellAttributes::default()
                        .set_image(Box::new(ImageCell::new(
                            TextureCoordinate::new_f32(0.75, 0.0),
                            TextureCoordinate::new_f32(1.0, 0.5),
                            data.clone()
                        )))
                        .clone()
                ),
            ],
            [
                Cell::new(
                    ' ',
                    CellAttributes::default()
                        .set_image(Box::new(ImageCell::new(
                            TextureCoordinate::new_f32(0.0, 0.5),
                            TextureCoordinate::new_f32(0.25, 1.0),
                            data.clone()
                        )))
                        .clone()
                ),
                Cell::new(
                    ' ',
                    CellAttributes::default()
                        .set_image(Box::new(ImageCell::new(
                            TextureCoordinate::new_f32(0.25, 0.5),
                            TextureCoordinate::new_f32(0.5, 1.0),
                            data.clone()
                        )))
                        .clone()
                ),
                Cell::new(
                    ' ',
                    CellAttributes::default()
                        .set_image(Box::new(ImageCell::new(
                            TextureCoordinate::new_f32(0.5, 0.5),
                            TextureCoordinate::new_f32(0.75, 1.0),
                            data.clone()
                        )))
                        .clone()
                ),
                Cell::new(
                    ' ',
                    CellAttributes::default()
                        .set_image(Box::new(ImageCell::new(
                            TextureCoordinate::new_f32(0.75, 0.5),
                            TextureCoordinate::new_f32(1.0, 1.0),
                            data.clone()
                        )))
                        .clone()
                ),
            ],
        ]
    );

    // Check that starting at not the texture origin coordinates
    // gives reasonable values in the resultant cell
    let mut other = Surface::new(1, 1);
    other.add_change(Change::Image(Image {
        top_left: TextureCoordinate::new_f32(0.25, 0.3),
        bottom_right: TextureCoordinate::new_f32(0.75, 0.8),
        image: data.clone(),
        width: 1,
        height: 1,
    }));
    assert_eq!(
        other.screen_cells(),
        [[Cell::new(
            ' ',
            CellAttributes::default()
                .set_image(Box::new(ImageCell::new(
                    TextureCoordinate::new_f32(0.25, 0.3),
                    TextureCoordinate::new_f32(0.75, 0.8),
                    data.clone()
                )))
                .clone()
        ),]]
    );
}
