#![cfg(test)]

use crate::cell::TeenyString;
use crate::color::ColorAttribute;
use crate::{
    Cell, CellAttributes, Presentation, UnicodeVersion, grapheme_column_width, is_white_space_char,
    is_white_space_grapheme, unicode_column_width,
};
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use finl_unicode::grapheme_clusters::Graphemes;

#[test]
fn teeny_string() {
    assert!(
        core::mem::size_of::<usize>() <= core::mem::size_of::<u64>(),
        "if a pointer doesn't fit in u64 then we need to change TeenyString"
    );

    let s = TeenyString::from_char('a');
    assert_eq!(s.as_bytes(), b"a");

    let longer = TeenyString::from_str("hellothere", None, None);
    assert_eq!(longer.as_bytes(), b"hellothere");

    assert_eq!(
        TeenyString::from_char(' ').as_bytes(),
        TeenyString::space().as_bytes()
    );
}

#[test]
#[cfg(target_pointer_width = "64")]
fn memory_usage() {
    assert_eq!(core::mem::size_of::<crate::color::RgbColor>(), 4);
    assert_eq!(core::mem::size_of::<ColorAttribute>(), 20);
    assert_eq!(core::mem::size_of::<CellAttributes>(), 16);
    assert_eq!(core::mem::size_of::<Cell>(), 24);
    assert_eq!(core::mem::size_of::<Vec<u8>>(), 24);
    assert_eq!(core::mem::size_of::<char>(), 4);
    assert_eq!(core::mem::size_of::<TeenyString>(), 8);
}

#[test]
fn nerf_special() {
    for c in " \n\r\t".chars() {
        let cell = Cell::new(c, CellAttributes::default());
        assert_eq!(cell.str(), " ");
    }

    for g in &["", " ", "\n", "\r", "\t", "\r\n"] {
        let cell = Cell::new_grapheme(g, CellAttributes::default(), None);
        assert_eq!(cell.str(), " ");
    }
}

#[test]
fn test_width() {
    let foot = "\u{1f9b6}";
    eprintln!("foot chars");
    for c in foot.chars() {
        eprintln!("char: {:?}", c);
    }
    assert_eq!(unicode_column_width(foot, None), 2, "{} should be 2", foot);

    let women_holding_hands_dark_skin_tone_medium_light_skin_tone =
        "\u{1F469}\u{1F3FF}\u{200D}\u{1F91D}\u{200D}\u{1F469}\u{1F3FC}";

    // Ensure that we can hold this longer grapheme sequence in the cell
    // and correctly return its string contents!
    let cell = Cell::new_grapheme(
        women_holding_hands_dark_skin_tone_medium_light_skin_tone,
        CellAttributes::default(),
        None,
    );
    assert_eq!(
        cell.str(),
        women_holding_hands_dark_skin_tone_medium_light_skin_tone
    );
    assert_eq!(
        cell.width(),
        2,
        "width of {} should be 2",
        women_holding_hands_dark_skin_tone_medium_light_skin_tone
    );

    let deaf_man = "\u{1F9CF}\u{200D}\u{2642}\u{FE0F}";
    eprintln!("deaf_man chars");
    for c in deaf_man.chars() {
        eprintln!("char: {:?}", c);
    }
    assert_eq!(unicode_column_width(deaf_man, None), 2);

    let man_dancing = "\u{1F57A}";
    assert_eq!(
        unicode_column_width(man_dancing, Some(&UnicodeVersion::new(9))),
        2
    );
    assert_eq!(
        unicode_column_width(man_dancing, Some(&UnicodeVersion::new(8))),
        2
    );

    let raised_fist = "\u{270a}";
    assert_eq!(
        unicode_column_width(raised_fist, Some(&UnicodeVersion::new(9))),
        2
    );
    assert_eq!(
        unicode_column_width(raised_fist, Some(&UnicodeVersion::new(8))),
        1
    );

    // This is a codepoint in the private use area
    let font_awesome_star = "\u{f005}";
    eprintln!("font_awesome_star {}", font_awesome_star.escape_debug());
    assert_eq!(unicode_column_width(font_awesome_star, None), 1);

    let england_flag = "\u{1f3f4}\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f}";
    assert_eq!(unicode_column_width(england_flag, None), 2);
}

#[test]
fn issue_1161() {
    let x_ideographic_space_x = "x\u{3000}x";
    assert_eq!(unicode_column_width(x_ideographic_space_x, None), 4);
    assert_eq!(
        Graphemes::new(x_ideographic_space_x).collect::<Vec<_>>(),
        vec!["x".to_string(), "\u{3000}".to_string(), "x".to_string()],
    );

    let c = Cell::new_grapheme("\u{3000}", CellAttributes::blank(), None);
    assert_eq!(c.width(), 2);
}

#[test]
fn issue_997() {
    let victory_hand = "\u{270c}";
    let victory_hand_text_presentation = "\u{270c}\u{fe0e}";

    assert_eq!(
        unicode_column_width(victory_hand_text_presentation, None),
        1
    );
    assert_eq!(unicode_column_width(victory_hand, None), 1);

    assert_eq!(
        Graphemes::new(victory_hand_text_presentation).collect::<Vec<_>>(),
        vec![victory_hand_text_presentation.to_string()]
    );
    assert_eq!(
        Graphemes::new(victory_hand).collect::<Vec<_>>(),
        vec![victory_hand.to_string()]
    );

    let copyright_emoji_presentation = "\u{00A9}\u{FE0F}";
    assert_eq!(
        Graphemes::new(copyright_emoji_presentation).collect::<Vec<_>>(),
        vec![copyright_emoji_presentation.to_string()]
    );
    assert_eq!(unicode_column_width(copyright_emoji_presentation, None), 2);
    assert_eq!(
        unicode_column_width(copyright_emoji_presentation, Some(&UnicodeVersion::new(9))),
        1
    );

    let copyright_text_presentation = "\u{00A9}";
    assert_eq!(
        Graphemes::new(copyright_text_presentation).collect::<Vec<_>>(),
        vec![copyright_text_presentation.to_string()]
    );
    assert_eq!(unicode_column_width(copyright_text_presentation, None), 1);

    let raised_fist = "\u{270a}";
    // Not valid to have explicit Text presentation for raised fist
    let raised_fist_text = "\u{270a}\u{fe0e}";
    assert_eq!(
        Presentation::for_grapheme(raised_fist),
        (Presentation::Emoji, None)
    );
    assert_eq!(unicode_column_width(raised_fist, None), 2);
    assert_eq!(
        Presentation::for_grapheme(raised_fist_text),
        (Presentation::Emoji, None)
    );
    assert_eq!(unicode_column_width(raised_fist_text, None), 2);

    assert_eq!(
        Graphemes::new(raised_fist_text).collect::<Vec<_>>(),
        vec![raised_fist_text.to_string()]
    );
    assert_eq!(
        Graphemes::new(raised_fist).collect::<Vec<_>>(),
        vec![raised_fist.to_string()]
    );
}

#[test]
fn issue_1573() {
    let sequence = "\u{1112}\u{1161}\u{11ab}";
    assert_eq!(unicode_column_width(sequence, None), 2);
    assert_eq!(grapheme_column_width(sequence, None), 2);

    let sequence2 = core::str::from_utf8(b"\xe1\x84\x92\xe1\x85\xa1\xe1\x86\xab").unwrap();
    assert_eq!(unicode_column_width(sequence2, None), 2);
    assert_eq!(grapheme_column_width(sequence2, None), 2);
}

// See <https://github.com/wezterm/wezterm/issues/6637>
// We're not directly "fixing" that issue here in termwiz at this time
// because it isn't clear that this cell module has enough context
// to eg: decide that the width of U+2028 should be returned as 1.
// That decision is made over in wezterm-term when processing
// a sequence of graphemes. This test case is just making assertions
// about the properties of a couple of problematic zero-width
// characters.
#[test]
fn issue_6637() {
    // U+2028 is the unicode line separator. It is Non-printing White_Space.
    let sequence = "\u{2028}";
    // It has zero width
    assert_eq!(unicode_column_width(sequence, None), 0);
    assert_eq!(grapheme_column_width(sequence, None), 0);
    // it is white space
    assert!(is_white_space_grapheme(sequence));

    // Just a couple of sanity checks for the white space function
    assert!(is_white_space_char(' '));
    assert!(!is_white_space_char('x'));

    // U+2068 is a BIDI control character and is relevant here
    // due to <https://github.com/wezterm/wezterm/issues/1422>.
    // It is Non-Printing, non-White_Space
    assert!(!is_white_space_char('\u{2068}'));
}
