use super::*;

/// When the uppercase form is used, the delete: field is set to true
/// which means that the underlying data is also released.  Otherwise,
/// the data is available to be placed again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KittyImageDelete {
    /// d='a' or d='A'.
    /// Delete all placements on visible screen
    All { delete: bool },
    /// d='i' or d='I'
    /// Delete all images with specified image_id.
    /// If placement_id is specified, then both image_id
    /// and placement_id must match
    ByImageId {
        image_id: u32,
        placement_id: Option<u32>,
        delete: bool,
    },
    /// d='n' or d='N'
    /// Delete newest image with specified image number.
    /// If placement_id is specified, then placement_id
    /// must also match.
    ByImageNumber {
        image_number: u32,
        placement_id: Option<u32>,
        delete: bool,
    },

    /// d='c' or d='C'
    /// Delete all placements that intersect with the current
    /// cursor position.
    AtCursorPosition { delete: bool },

    /// d='f' or d='F'
    /// Delete animation frames
    AnimationFrames { delete: bool },

    /// d='p' or d='P'
    /// Delete all placements that intersect the specified
    /// cell x and y coordinates
    DeleteAt { x: u32, y: u32, delete: bool },

    /// d='q' or d='Q'
    /// Delete all placements that intersect the specified
    /// cell x and y coordinates, with the specified z-index
    DeleteAtZ {
        x: u32,
        y: u32,
        z: i32,
        delete: bool,
    },

    /// d='x' or d='X'
    /// Delete all placements that intersect the specified column.
    DeleteColumn { x: u32, delete: bool },

    /// d='y' or d='Y'
    /// Delete all placements that intersect the specified row.
    DeleteRow { y: u32, delete: bool },

    /// d='z' or d='Z'
    /// Delete all placements that have the specified z-index.
    DeleteZ { z: i32, delete: bool },
}

impl KittyImageDelete {
    pub(super) fn from_keys(keys: &BTreeMap<&str, &str>) -> Option<Self> {
        let d = get(keys, "d").unwrap_or("a");
        if d.len() != 1 {
            return None;
        }
        let d = d.chars().next()?;
        let delete = d.is_ascii_uppercase();
        match d {
            'a' | 'A' => Some(Self::All { delete }),
            'i' | 'I' => Some(Self::ByImageId {
                image_id: geti(keys, "i")?,
                placement_id: geti(keys, "p"),
                delete,
            }),
            'n' | 'N' => Some(Self::ByImageNumber {
                image_number: geti(keys, "I")?,
                placement_id: geti(keys, "p"),
                delete,
            }),
            'c' | 'C' => Some(Self::AtCursorPosition { delete }),
            'f' | 'F' => Some(Self::AnimationFrames { delete }),
            'p' | 'P' => Some(Self::DeleteAt {
                x: geti(keys, "x")?,
                y: geti(keys, "y")?,
                delete,
            }),
            'q' | 'Q' => Some(Self::DeleteAtZ {
                x: geti(keys, "x")?,
                y: geti(keys, "y")?,
                z: geti(keys, "z")?,
                delete,
            }),
            'x' | 'X' => Some(Self::DeleteColumn {
                x: geti(keys, "x")?,
                delete,
            }),
            'y' | 'Y' => Some(Self::DeleteRow {
                y: geti(keys, "y")?,
                delete,
            }),
            'z' | 'Z' => Some(Self::DeleteZ {
                z: geti(keys, "z")?,
                delete,
            }),
            _ => None,
        }
    }

    pub(super) fn to_keys(&self, keys: &mut BTreeMap<&'static str, String>) {
        fn d(c: char, delete: &bool) -> String {
            if *delete { c.to_ascii_uppercase() } else { c }.to_string()
        }

        match self {
            Self::All { delete } => {
                keys.insert("d", d('a', delete));
            }
            Self::ByImageId {
                image_id,
                placement_id,
                delete,
            } => {
                keys.insert("d", d('i', delete));
                if let Some(p) = placement_id {
                    keys.insert("p", p.to_string());
                }
                keys.insert("i", image_id.to_string());
            }
            Self::ByImageNumber {
                image_number,
                placement_id,
                delete,
            } => {
                keys.insert("d", d('n', delete));
                if let Some(p) = placement_id {
                    keys.insert("p", p.to_string());
                }
                keys.insert("I", image_number.to_string());
            }
            Self::AtCursorPosition { delete } => {
                keys.insert("d", d('c', delete));
            }
            Self::AnimationFrames { delete } => {
                keys.insert("d", d('f', delete));
            }
            Self::DeleteAt { x, y, delete } => {
                keys.insert("d", d('p', delete));
                keys.insert("x", x.to_string());
                keys.insert("y", y.to_string());
            }
            Self::DeleteAtZ { x, y, z, delete } => {
                keys.insert("d", d('p', delete));
                keys.insert("x", x.to_string());
                keys.insert("y", y.to_string());
                keys.insert("z", z.to_string());
            }
            Self::DeleteColumn { x, delete } => {
                keys.insert("d", d('x', delete));
                keys.insert("x", x.to_string());
            }
            Self::DeleteRow { y, delete } => {
                keys.insert("d", d('y', delete));
                keys.insert("y", y.to_string());
            }
            Self::DeleteZ { z, delete } => {
                keys.insert("d", d('z', delete));
                keys.insert("z", z.to_string());
            }
        }
    }
}
