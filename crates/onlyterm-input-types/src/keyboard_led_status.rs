use alloc::string::ToString;
use bitflags::*;
use core::fmt::Write;
use onlyterm_dynamic::{FromDynamic, ToDynamic};

bitflags! {
    #[derive(Default, FromDynamic, ToDynamic)]
    pub struct KeyboardLedStatus: u8 {
        const CAPS_LOCK = 1<<1;
        const NUM_LOCK = 1<<2;
    }
}

impl core::fmt::Display for KeyboardLedStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let mut wrote_any = false;
        if self.contains(Self::CAPS_LOCK) {
            f.write_str("CAPS_LOCK")?;
            wrote_any = true;
        }
        if self.contains(Self::NUM_LOCK) {
            if wrote_any {
                f.write_char('|')?;
            }
            f.write_str("NUM_LOCK")?;
        }
        Ok(())
    }
}
