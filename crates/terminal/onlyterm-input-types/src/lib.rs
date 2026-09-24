#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[path = "core/geometry.rs"]
mod geometry;
pub use geometry::*;

#[path = "core/keyboard_led_status.rs"]
mod keyboard_led_status;
pub use keyboard_led_status::*;

#[path = "core/modifiers.rs"]
mod modifiers;
pub use modifiers::*;

#[path = "core/handled.rs"]
mod handled;
pub use handled::*;

#[path = "core/mouse.rs"]
mod mouse;
pub use mouse::*;

#[path = "keys/key_code.rs"]
mod key_code;
pub use key_code::*;

#[path = "keys/phys_key_code.rs"]
mod phys_key_code;
pub use phys_key_code::*;

#[path = "keys/ctrl_mapping.rs"]
mod ctrl_mapping;
pub use ctrl_mapping::*;

#[path = "keys/raw_key_event.rs"]
mod raw_key_event;
pub use raw_key_event::*;

#[path = "keys/kitty_keyboard_flags.rs"]
mod kitty_keyboard_flags;
pub use kitty_keyboard_flags::*;

#[path = "keys/is_ascii_control.rs"]
mod is_ascii_control;
pub use is_ascii_control::*;

#[path = "keys/key_event.rs"]
mod key_event;
pub use key_event::*;

#[path = "ui/ui_key_cap_rendering.rs"]
mod ui_key_cap_rendering;
pub use ui_key_cap_rendering::*;

#[path = "ui/window_decorations.rs"]
mod window_decorations;
pub use window_decorations::*;

#[path = "ui/integrated_title_button.rs"]
mod integrated_title_button;
pub use integrated_title_button::*;

#[path = "ui/integrated_title_button_alignment.rs"]
mod integrated_title_button_alignment;
pub use integrated_title_button_alignment::*;

#[path = "ui/integrated_title_button_style.rs"]
mod integrated_title_button_style;
pub use integrated_title_button_style::*;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod test;
