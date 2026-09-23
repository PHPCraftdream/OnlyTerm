//! Rules X1 through X9: explicit embedding levels and the removal of
//! explicit formatting characters.

use super::level_stack::{LevelStack, Override};
use super::paragraph::paragraph_level;
use super::BidiContext;
use crate::{BidiClass, Direction, Level, NO_LEVEL};
use log::trace;

impl BidiContext {
    /// Rules X1 through X8
    // The X7 (PopDirectionalFormat) handler uses explicit nested `if`s to
    // mirror the spec; keep them rather than collapsing with `&&`.
    #[allow(clippy::collapsible_if)]
    pub(crate) fn explicit_embedding_levels(&mut self) {
        // X1: initialize stack and other variables
        let mut stack = LevelStack::new();
        stack.push(self.base_level, Override::Neutral, false);

        let len = self.char_types.len();
        self.levels.resize(len, Level::default());

        let mut overflow_isolate = 0;
        let mut overflow_embedding = 0;
        let mut valid_isolate = 0;

        // X2..X8: process each character, setting embedding levels
        // and override status
        for idx in 0..len {
            let bc = self.char_types[idx];
            trace!("Considering idx={} {:?}", idx, bc);
            match bc {
                // X2
                BidiClass::RightToLeftEmbedding => {
                    if let Some(level) = stack.embedding_level().least_greater_odd() {
                        if overflow_isolate == 0 && overflow_embedding == 0 {
                            stack.push(level, Override::Neutral, false);
                            continue;
                        }
                    }
                    if overflow_isolate == 0 {
                        overflow_embedding += 1;
                    }
                }
                // X3
                BidiClass::LeftToRightEmbedding => {
                    if let Some(level) = stack.embedding_level().least_greater_even() {
                        if overflow_isolate == 0 && overflow_embedding == 0 {
                            stack.push(level, Override::Neutral, false);
                            continue;
                        }
                    }
                    if overflow_isolate == 0 {
                        overflow_embedding += 1;
                    }
                }
                // X4
                BidiClass::RightToLeftOverride => {
                    if let Some(level) = stack.embedding_level().least_greater_odd() {
                        if overflow_isolate == 0 && overflow_embedding == 0 {
                            stack.push(level, Override::RTL, false);
                            continue;
                        }
                    }
                    if overflow_isolate == 0 {
                        overflow_embedding += 1;
                    }
                }
                // X5
                BidiClass::LeftToRightOverride => {
                    if let Some(level) = stack.embedding_level().least_greater_even() {
                        if overflow_isolate == 0 && overflow_embedding == 0 {
                            stack.push(level, Override::LTR, false);
                            continue;
                        }
                    }
                    if overflow_isolate == 0 {
                        overflow_embedding += 1;
                    }
                }
                // X5a
                BidiClass::RightToLeftIsolate => {
                    self.levels[idx] = stack.embedding_level();
                    stack.apply_override(&mut self.char_types[idx]);
                    if let Some(level) = stack.embedding_level().least_greater_odd() {
                        if overflow_isolate == 0 && overflow_embedding == 0 {
                            valid_isolate += 1;
                            stack.push(level, Override::Neutral, true);
                            continue;
                        }
                    }
                    overflow_isolate += 1;
                }
                // X5b
                BidiClass::LeftToRightIsolate => {
                    self.levels[idx] = stack.embedding_level();
                    stack.apply_override(&mut self.char_types[idx]);
                    if let Some(level) = stack.embedding_level().least_greater_even() {
                        if overflow_isolate == 0 && overflow_embedding == 0 {
                            valid_isolate += 1;
                            stack.push(level, Override::Neutral, true);
                            continue;
                        }
                    }
                    overflow_isolate += 1;
                }
                // X5c
                BidiClass::FirstStrongIsolate => {
                    let level =
                        paragraph_level(&self.char_types[idx + 1..], true, Direction::LeftToRight);
                    self.levels[idx] = stack.embedding_level();
                    stack.apply_override(&mut self.char_types[idx]);
                    let level = if level.0 == 1 {
                        stack.embedding_level().least_greater_odd()
                    } else {
                        stack.embedding_level().least_greater_even()
                    };
                    trace!(
                        "picked {:?} based on current stack level {:?}",
                        level,
                        stack.embedding_level()
                    );

                    if let Some(level) = level {
                        if overflow_isolate == 0 && overflow_embedding == 0 {
                            valid_isolate += 1;
                            stack.push(level, Override::Neutral, true);
                            continue;
                        }
                    }
                    overflow_isolate += 1;
                }
                // X6a
                BidiClass::PopDirectionalIsolate => {
                    if overflow_isolate > 0 {
                        overflow_isolate -= 1;
                    } else if valid_isolate == 0 {
                        // Do nothing
                    } else {
                        overflow_embedding = 0;
                        loop {
                            if stack.isolate_status() {
                                break;
                            }
                            stack.pop();
                        }
                        stack.pop();
                        valid_isolate -= 1;
                    }

                    self.levels[idx] = stack.embedding_level();
                    stack.apply_override(&mut self.char_types[idx]);
                }
                // X7
                BidiClass::PopDirectionalFormat => {
                    if overflow_isolate > 0 {
                        // Do nothing
                    } else if overflow_embedding > 0 {
                        overflow_embedding -= 1;
                    } else {
                        if !stack.isolate_status() {
                            if stack.depth() >= 2 {
                                stack.pop();
                            }
                        }
                    }
                }
                BidiClass::BoundaryNeutral => {}
                // X8
                BidiClass::ParagraphSeparator => {
                    // Terminates all embedding contexts.
                    // Should only ever be the last character in
                    // a paragraph if present at all.
                    self.levels[idx] = self.base_level;
                }
                // X6
                _ => {
                    self.levels[idx] = stack.embedding_level();
                    stack.apply_override(&mut self.char_types[idx]);
                }
            }
        }
    }

    /// X9
    pub(crate) fn delete_format_characters(&mut self) {
        for (bc, level) in self.char_types.iter().zip(&mut self.levels) {
            match bc {
                BidiClass::RightToLeftEmbedding
                | BidiClass::LeftToRightEmbedding
                | BidiClass::RightToLeftOverride
                | BidiClass::LeftToRightOverride
                | BidiClass::PopDirectionalFormat
                | BidiClass::BoundaryNeutral => {
                    *level = Level(NO_LEVEL);
                }
                _ => {}
            }
        }
    }
}
