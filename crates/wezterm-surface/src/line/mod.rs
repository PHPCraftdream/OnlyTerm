mod cellref;
mod clusterline;
mod doubleclick;
// The inner `line` module holds the `Line` type itself; the directory is named
// `line` to group all line-related code. Renaming it would be a non-cosmetic
// restructure of a core type, so the `module_inception` lint is suppressed.
#[allow(clippy::module_inception)]
mod line;
mod linebits;
mod storage;
mod test;
mod vecstorage;
mod zone;

pub use cellref::CellRef;
pub use doubleclick::DoubleClickRange;
pub use line::Line;
pub use zone::ZoneRange;
