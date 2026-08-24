use crate::attributes::CellAttributes;
use crate::unicode::{UnicodeVersion, grapheme_column_width};
use alloc::boxed::Box;
use alloc::vec::Vec;
use onlyterm_char_props::emoji::Presentation;
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[cfg(feature = "use_serde")]
fn deserialize_teenystring<'de, D>(deserializer: D) -> Result<TeenyString, D::Error>
where
    D: Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    Ok(TeenyString::from_str(&text, None, None))
}

#[cfg(feature = "use_serde")]
fn serialize_teenystring<S>(value: &TeenyString, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    // The constructor guarantees the storage is valid UTF-8 (input is always a
    // `&str` or a `char`'s UTF-8 encoding); validate rather than trust.
    let s = core::str::from_utf8(value.as_bytes()).expect("TeenyString storage is valid UTF-8");
    s.serialize(serializer)
}

/// TeenyString encodes string storage in a single u64.
/// The scheme is simple but effective: strings that encode into a
/// byte slice that is 1 less byte than the machine word size can
/// be encoded directly into the usize bits stored in the struct.
/// A marker bit (LSB for big endian, MSB for little endian) is
/// set to indicate that the string is stored inline.
/// If the string is longer than this then a `Vec<u8>` is allocated
/// from the heap and the usize holds its raw pointer address.
///
/// When the string is inlined, the next-MSB is used to short-cut
/// calling grapheme_column_width; if it is set, then the TeenyString
/// has length 2, otherwise, it has length 1 (we don't allow zero-length
/// strings).
pub(crate) struct TeenyString(u64);
struct TeenyStringHeap {
    bytes: Vec<u8>,
    width: usize,
}

impl TeenyString {
    const fn marker_mask() -> u64 {
        if cfg!(target_endian = "little") {
            0x80000000_00000000
        } else {
            0x1
        }
    }

    const fn double_wide_mask() -> u64 {
        if cfg!(target_endian = "little") {
            0xc0000000_00000000
        } else {
            0x3
        }
    }

    const fn is_marker_bit_set(word: u64) -> bool {
        let mask = Self::marker_mask();
        word & mask == mask
    }

    const fn is_double_width(word: u64) -> bool {
        let mask = Self::double_wide_mask();
        word & mask == mask
    }

    const fn set_marker_bit(word: u64, width: usize) -> u64 {
        if width > 1 {
            word | Self::double_wide_mask()
        } else {
            word | Self::marker_mask()
        }
    }

    pub fn from_str(
        s: &str,
        width: Option<usize>,
        unicode_version: Option<&UnicodeVersion>,
    ) -> Self {
        // De-fang the input text such that it has no special meaning
        // to a terminal.  All control and movement characters are rewritten
        // as a space.
        let s = if s.is_empty() || s == "\r\n" {
            " "
        } else if s.len() == 1 {
            let b = s.as_bytes()[0];
            if b < 0x20 || b == 0x7f { " " } else { s }
        } else {
            s
        };

        let bytes = s.as_bytes();
        let len = bytes.len();
        let width = width.unwrap_or_else(|| grapheme_column_width(s, unicode_version));

        if len < core::mem::size_of::<u64>() && width < 3 {
            // Pack the inline bytes into a u64 in memory order: place bytes at
            // the low addresses and zero-pad the rest, then reinterpret via the
            // native-endian conversion so the in-memory layout is identical to
            // the previous `copy_nonoverlapping` into `&mut word`.
            let mut arr = [0u8; core::mem::size_of::<u64>()];
            arr[..len].copy_from_slice(bytes);
            let word = if cfg!(target_endian = "little") {
                u64::from_le_bytes(arr)
            } else {
                u64::from_be_bytes(arr)
            };
            let word = Self::set_marker_bit(word, width);
            Self(word)
        } else {
            let vec = Box::new(TeenyStringHeap {
                bytes: bytes.to_vec(),
                width,
            });
            let ptr = Box::into_raw(vec);
            Self(ptr as u64)
        }
    }

    pub const fn space() -> Self {
        Self(if cfg!(target_endian = "little") {
            0x80000000_00000020
        } else {
            0x20000000_00000001
        })
    }

    pub fn from_char(c: char) -> Self {
        let mut bytes = [0u8; 8];
        Self::from_str(c.encode_utf8(&mut bytes), None, None)
    }

    pub fn width(&self) -> usize {
        if Self::is_marker_bit_set(self.0) {
            if Self::is_double_width(self.0) { 2 } else { 1 }
        } else {
            let heap = self.0 as *const u64 as *const TeenyStringHeap;
            // SAFETY: when the marker bit is clear, self.0 holds a valid owned
            // pointer produced by `Box::into_raw` in `from_str` (or cloned from
            // such a value). The heap allocation stays alive for the lifetime of
            // this TeenyString and we only read through the pointer.
            unsafe { (*heap).width }
        }
    }

    pub fn str(&self) -> &str {
        // The constructor guarantees the storage is valid UTF-8 (input is always
        // a `&str` or a `char`'s UTF-8 encoding); validate rather than trust.
        core::str::from_utf8(self.as_bytes()).expect("TeenyString storage is valid UTF-8")
    }

    pub fn as_bytes(&self) -> &[u8] {
        if Self::is_marker_bit_set(self.0) {
            let bytes = &self.0 as *const u64 as *const u8;
            // SAFETY: `bytes` points at the inline representation of `self.0`,
            // which outlives the returned borrow. We read exactly 7 bytes
            // (size_of::<u64>() - 1), all within the u64, so the slice is in
            // bounds and properly aligned. A safe wrapper is impossible here
            // because we must return a borrow into the interior bytes of the
            // packed u64 field, which is what keeps TeenyString 8 bytes.
            let bytes =
                unsafe { core::slice::from_raw_parts(bytes, core::mem::size_of::<u64>() - 1) };
            let len = bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(core::mem::size_of::<u64>() - 1);

            &bytes[0..len]
        } else {
            let heap = self.0 as *const u64 as *const TeenyStringHeap;
            // SAFETY: marker bit clear means self.0 is a valid owned pointer to
            // a TeenyStringHeap (see `from_str`/`Drop`); it outlives this borrow
            // and we only read its `bytes` Vec.
            unsafe { (*heap).bytes.as_slice() }
        }
    }
}

impl Drop for TeenyString {
    fn drop(&mut self) {
        if !Self::is_marker_bit_set(self.0) {
            // SAFETY: marker bit clear means self.0 is the exact raw pointer
            // produced by `Box::into_raw` in `from_str` (Clone rebuilds via
            // from_str rather than copying the pointer, so ownership is unique).
            // We own exactly one reference, so reconstructing and dropping the
            // Box exactly once frees the heap allocation without double-free.
            let vec = unsafe { Box::from_raw(self.0 as *mut usize as *mut TeenyStringHeap) };
            drop(vec);
        }
    }
}

impl core::clone::Clone for TeenyString {
    fn clone(&self) -> Self {
        if Self::is_marker_bit_set(self.0) {
            Self(self.0)
        } else {
            Self::from_str(self.str(), None, None)
        }
    }
}

impl core::cmp::PartialEq for TeenyString {
    fn eq(&self, rhs: &Self) -> bool {
        self.as_bytes().eq(rhs.as_bytes())
    }
}
impl core::cmp::Eq for TeenyString {}

/// Models the contents of a cell on the terminal display
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Clone, Eq, PartialEq)]
pub struct Cell {
    #[cfg_attr(
        feature = "use_serde",
        serde(
            deserialize_with = "deserialize_teenystring",
            serialize_with = "serialize_teenystring"
        )
    )]
    text: TeenyString,
    attrs: CellAttributes,
}

impl core::fmt::Debug for Cell {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        fmt.debug_struct("Cell")
            .field("text", &self.str())
            .field("width", &self.width())
            .field("attrs", &self.attrs)
            .finish()
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::blank()
    }
}

impl Cell {
    /// Create a new cell holding the specified character and with the
    /// specified cell attributes.
    /// All control and movement characters are rewritten as a space.
    pub fn new(text: char, attrs: CellAttributes) -> Self {
        let storage = TeenyString::from_char(text);
        Self {
            text: storage,
            attrs,
        }
    }

    pub const fn blank() -> Self {
        Self {
            text: TeenyString::space(),
            attrs: CellAttributes::blank(),
        }
    }

    pub const fn blank_with_attrs(attrs: CellAttributes) -> Self {
        Self {
            text: TeenyString::space(),
            attrs,
        }
    }

    /// Indicates whether this cell has text or emoji presentation.
    /// The width already reflects that choice; this information
    /// is also useful when selecting an appropriate font.
    pub fn presentation(&self) -> Presentation {
        match Presentation::for_grapheme(self.str()) {
            (_, Some(variation)) => variation,
            (presentation, None) => presentation,
        }
    }

    /// Create a new cell holding the specified grapheme.
    /// The grapheme is passed as a string slice and is intended to hold
    /// double-width characters, or combining unicode sequences, that need
    /// to be treated as a single logical "character" that can be cursored
    /// over.  This function technically allows for an arbitrary string to
    /// be passed but it should not be used to hold strings other than
    /// graphemes.
    pub fn new_grapheme(
        text: &str,
        attrs: CellAttributes,
        unicode_version: Option<&UnicodeVersion>,
    ) -> Self {
        let storage = TeenyString::from_str(text, None, unicode_version);

        Self {
            text: storage,
            attrs,
        }
    }

    pub fn new_grapheme_with_width(text: &str, width: usize, attrs: CellAttributes) -> Self {
        let storage = TeenyString::from_str(text, Some(width), None);
        Self {
            text: storage,
            attrs,
        }
    }

    /// Returns the textual content of the cell
    pub fn str(&self) -> &str {
        self.text.str()
    }

    /// Returns the number of cells visually occupied by this grapheme
    pub fn width(&self) -> usize {
        self.text.width()
    }

    /// Returns the attributes of the cell
    pub fn attrs(&self) -> &CellAttributes {
        &self.attrs
    }

    pub fn attrs_mut(&mut self) -> &mut CellAttributes {
        &mut self.attrs
    }
}
