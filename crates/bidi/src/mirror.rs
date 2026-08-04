use crate::bidi_mirroring;

/// Returns the mirrored counterpart of `c` per the Unicode
/// `Bidi_Mirroring_Glyph` property (UAX #9 rule L4), if one exists. A
/// renderer must draw this glyph instead of `c` wherever `c`'s resolved
/// embedding level is odd (right-to-left) -- eg: `(` drawn as `)` and
/// vice versa, so parens/brackets/angle-quotes open towards the start of
/// the RTL run rather than always opening towards the right as if the
/// surrounding text were still left-to-right. Rule L4 is explicitly out
/// of scope for the bidi algorithm itself (a rendering concern), so this
/// is not applied anywhere inside `resolve_paragraph`/`reordered_runs`;
/// callers that draw characters must apply it themselves once they know
/// each character's resolved direction.
pub fn mirror_char(c: char) -> Option<char> {
    use bidi_mirroring::BIDI_MIRRORING;
    if let Ok(idx) = BIDI_MIRRORING.binary_search_by_key(&c, |&(from, _)| from) {
        return Some(BIDI_MIRRORING[idx].1);
    }
    None
}
