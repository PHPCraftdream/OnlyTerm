// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Raw FFI bindings to the fontconfig C library, read as a correspondence
//! table against `<fontconfig/fontconfig.h>`. Grouped into thematic
//! modules -- value types, patterns, font sets, char sets, lang sets,
//! config, cache, blanks, atomic file updates, string helpers, unicode
//! conversions, matrix operations, and library init -- rather than one
//! file per binding, since the bindings are not meaningfully self-
//! contained the way ordinary Rust items are.

#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]

mod atomic;
mod blanks;
mod cache;
mod char_set;
mod config;
mod font_set;
mod init;
mod lang_set;
mod matrix_ops;
mod pattern;
mod str_ops;
mod unicode;
mod value_types;

pub use atomic::*;
pub use blanks::*;
pub use cache::*;
pub use char_set::*;
pub use config::*;
pub use font_set::*;
pub use init::*;
pub use lang_set::*;
pub use matrix_ops::*;
pub use pattern::*;
pub use str_ops::*;
pub use unicode::*;
pub use value_types::*;
