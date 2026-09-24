use super::bidi_class;
use super::bidi_class::BidiClass;

pub fn bidi_class_for_char(c: char) -> BidiClass {
    use core::cmp::Ordering;
    if let Ok(idx) = bidi_class::BIDI_CLASS.binary_search_by(|&(lower, upper, _)| {
        if c >= lower && c <= upper {
            Ordering::Equal
        } else if c < lower {
            Ordering::Greater
        } else if c > upper {
            Ordering::Less
        } else {
            unreachable!()
        }
    }) {
        let entry = &bidi_class::BIDI_CLASS[idx];
        if c >= entry.0 && c <= entry.1 {
            return entry.2;
        }
    }
    // extracted/DerivedBidiClass.txt says:
    // All code points not explicitly listed for Bidi_Class
    //  have the value Left_To_Right (L).
    BidiClass::LeftToRight
}

#[cfg(test)]
mod tests {
    use super::bidi_class::BidiClass;
    use super::bidi_class_for_char;
    use alloc::format;
    use k9::assert_equal as assert_eq;

    #[test]
    fn bidi_class_resolve() {
        assert_eq!(bidi_class_for_char('\u{0}'), BidiClass::BoundaryNeutral);
        assert_eq!(bidi_class_for_char('\u{9}'), BidiClass::SegmentSeparator);
        assert_eq!(bidi_class_for_char(' '), BidiClass::WhiteSpace);
        assert_eq!(bidi_class_for_char('a'), BidiClass::LeftToRight);
        assert_eq!(bidi_class_for_char('\u{590}'), BidiClass::RightToLeft);
        assert_eq!(bidi_class_for_char('\u{5d0}'), BidiClass::RightToLeft);
        assert_eq!(bidi_class_for_char('\u{5d1}'), BidiClass::RightToLeft);
    }
}
