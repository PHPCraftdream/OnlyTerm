//! Rules P2 and P3: determine the paragraph embedding level from the
//! first strong character in the paragraph.

use crate::{BidiClass, Direction, Level};

/// 3.3.1 Paragraph level.
/// We've been fed a single paragraph, which takes care of rule P1.
/// This function implements rules P2 and P3.
pub(crate) fn paragraph_level(
    types: &[BidiClass],
    respect_pdi: bool,
    fallback: Direction,
) -> Level {
    let mut isolate_count = 0;
    for &t in types {
        match t {
            BidiClass::RightToLeftIsolate
            | BidiClass::LeftToRightIsolate
            | BidiClass::FirstStrongIsolate => isolate_count += 1,
            BidiClass::PopDirectionalIsolate => {
                if isolate_count > 0 {
                    isolate_count -= 1;
                } else if respect_pdi {
                    break;
                }
            }
            BidiClass::LeftToRight if isolate_count == 0 => return Level(0),
            BidiClass::RightToLeft | BidiClass::ArabicLetter if isolate_count == 0 => {
                return Level(1)
            }
            _ => {}
        }
    }
    if fallback == Direction::LeftToRight {
        Level(0)
    } else {
        Level(1)
    }
}
