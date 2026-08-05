use core::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub enum DoubleClickRange {
    Range(Range<usize>),
    RangeWithWrap(Range<usize>),
}
