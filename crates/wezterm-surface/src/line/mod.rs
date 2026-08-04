mod cellref;
mod clusterline;
// The inner `line` module holds the `Line` type itself; the directory is named
// `line` to group all line-related code. Renaming it would be a non-cosmetic
// restructure of a core type, so the `module_inception` lint is suppressed.
#[allow(clippy::module_inception)]
mod line;
mod linebits;
mod storage;
mod test;
mod vecstorage;

pub use cellref::CellRef;
pub use line::{DoubleClickRange, Line};
