use crate::{bidi_class, BidiClass};

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
