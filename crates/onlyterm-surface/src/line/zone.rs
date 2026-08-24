use core::ops::Range;
use onlyterm_cell::SemanticType;
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneRange {
    pub semantic_type: SemanticType,
    pub range: Range<u16>,
}
