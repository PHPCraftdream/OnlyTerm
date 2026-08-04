use crate::CsiParam;
#[cfg(any(feature = "std", feature = "alloc"))]
use alloc::vec::Vec;

/// `VTActor` is a trait that allows the host application to process
/// the different kinds of sequence as they are parsed from the input
/// stream.
///
/// The functions defined by this trait correspond to the actions defined
/// in the [state machine](https://vt100.net/emu/dec_ansi_parser).
///
/// ## Terminology:
/// An intermediate is a character in the range 0x20-0x2f that
/// occurs before the final character in an escape sequence.
///
/// `ignored_excess_intermediates` is a boolean that is set in the case
/// where there were more than two intermediate characters; no standard
/// defines any codes with more than two.  Intermediates after
/// the second will set this flag and are discarded.
///
/// `params` in most of the functions of this trait are decimal integer parameters in escape
/// sequences.  They are separated by semicolon characters.  An omitted parameter is returned in
/// this interface as a zero, which represents the default value for that parameter.
///
/// Other jargon used here is defined in
/// [ECMA-48](http://www.ecma-international.org/publications/files/ECMA-ST/ECMA-48,%202nd%20Edition,%20August%201979.pdf).
pub trait VTActor {
    /// The current code should be mapped to a glyph according to the character set mappings and
    /// shift states in effect, and that glyph should be displayed.
    ///
    /// If the input was UTF-8 then it will have been mapped to a unicode code point.  Invalid
    /// sequences are represented here using the unicode REPLACEMENT_CHARACTER.
    ///
    /// Otherwise the parameter will be a 7-bit printable value and may be subject to mapping
    /// depending on other state maintained by the embedding application.
    ///
    /// ## Some commentary from the state machine documentation:
    /// GL characters (20 to 7F) are
    /// printed. 20 (SP) and 7F (DEL) are included in this area, although both codes have special
    /// behaviour. If a 94-character set is mapped into GL, 20 will cause a space to be displayed,
    /// and 7F will be ignored. When a 96-character set is mapped into GL, both 20 and 7F may cause
    /// a character to be displayed. Later models of the VT220 included the DEC Multinational
    /// Character Set (MCS), which has 94 characters in its supplemental set (i.e. the characters
    /// supplied in addition to ASCII), so terminals only claiming VT220 compatibility can always
    /// ignore 7F. The VT320 introduced ISO Latin-1, which has 96 characters in its supplemental
    /// set, so emulators with a VT320 compatibility mode need to treat 7F as a printable
    /// character.
    fn print(&mut self, b: char);

    /// The C0 or C1 control function should be executed, which may have any one of a variety of
    /// effects, including changing the cursor position, suspending or resuming communications or
    /// changing the shift states in effect.
    ///
    /// See [ECMA-48](http://www.ecma-international.org/publications/files/ECMA-ST/ECMA-48,%202nd%20Edition,%20August%201979.pdf)
    /// for more information on C0 and C1 control functions.
    fn execute_c0_or_c1(&mut self, control: u8);

    /// invoked when a final character arrives in the first part of a device control string. It
    /// determines the control function from the private marker, intermediate character(s) and
    /// final character, and executes it, passing in the parameter list. It also selects a handler
    /// function for the rest of the characters in the control string.
    ///
    /// See [ECMA-48](http://www.ecma-international.org/publications/files/ECMA-ST/ECMA-48,%202nd%20Edition,%20August%201979.pdf)
    /// for more information on device control strings.
    fn dcs_hook(
        &mut self,
        mode: u8,
        params: &[i64],
        intermediates: &[u8],
        ignored_excess_intermediates: bool,
    );

    /// This action passes characters from the data string part of a device control string to a
    /// handler that has previously been selected by the dcs_hook action. C0 controls are also
    /// passed to the handler.
    ///
    /// See [ECMA-48](http://www.ecma-international.org/publications/files/ECMA-ST/ECMA-48,%202nd%20Edition,%20August%201979.pdf)
    /// for more information on device control strings.
    fn dcs_put(&mut self, byte: u8);

    /// When a device control string is terminated by ST, CAN, SUB or ESC, this action calls the
    /// previously selected handler function with an “end of data” parameter. This allows the
    /// handler to finish neatly.
    ///
    /// See [ECMA-48](http://www.ecma-international.org/publications/files/ECMA-ST/ECMA-48,%202nd%20Edition,%20August%201979.pdf)
    /// for more information on device control strings.
    fn dcs_unhook(&mut self);

    /// The final character of an escape sequence has arrived, so determine the control function
    /// to be executed from the intermediate character(s) and final character, and execute it.
    ///
    /// See [ECMA-48](http://www.ecma-international.org/publications/files/ECMA-ST/ECMA-48,%202nd%20Edition,%20August%201979.pdf)
    /// for more information on escape sequences.
    fn esc_dispatch(
        &mut self,
        params: &[i64],
        intermediates: &[u8],
        ignored_excess_intermediates: bool,
        byte: u8,
    );

    /// A final character of a Control Sequence Initiator has arrived, so determine the control function to be executed from
    /// private marker, intermediate character(s) and final character, and execute it, passing in
    /// the parameter list.
    ///
    /// See [ECMA-48](http://www.ecma-international.org/publications/files/ECMA-ST/ECMA-48,%202nd%20Edition,%20August%201979.pdf)
    /// for more information on control functions.
    fn csi_dispatch(&mut self, params: &[CsiParam], parameters_truncated: bool, byte: u8);

    /// Called when an OSC string is terminated by ST, CAN, SUB or ESC.
    ///
    /// `params` is an array of byte strings (which may also be valid utf-8)
    /// that were passed as semicolon separated parameters to the operating
    /// system command.
    fn osc_dispatch(&mut self, params: &[&[u8]]);

    /// Called when an APC string is terminated by ST
    /// `data` is the data contained within the APC sequence.
    #[cfg(any(feature = "std", feature = "alloc"))]
    fn apc_dispatch(&mut self, data: Vec<u8>);
}
