//! Rules I1 and I2: resolve the implicit levels.

use super::super::BidiContext;
use crate::{BidiClass, Direction};

impl BidiContext {
    /// This function runs Rules I1 and I2 together.
    pub(crate) fn resolve_implicit_levels(&mut self) {
        for (idx, level) in self.levels.iter_mut().enumerate() {
            if level.removed_by_x9() {
                continue;
            }

            match level.direction() {
                Direction::LeftToRight => {
                    // I1
                    match self.char_types[idx] {
                        BidiClass::RightToLeft => {
                            level.0 += 1;
                        }
                        BidiClass::ArabicNumber | BidiClass::EuropeanNumber => {
                            level.0 += 2;
                        }
                        _ => {}
                    }
                }
                Direction::RightToLeft => {
                    // I2
                    match self.char_types[idx] {
                        BidiClass::LeftToRight
                        | BidiClass::ArabicNumber
                        | BidiClass::EuropeanNumber => {
                            level.0 += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
