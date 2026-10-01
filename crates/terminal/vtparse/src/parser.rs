use crate::enums::*;
use crate::transitions::{ENTRY, EXIT, TRANSITIONS};
use crate::{CsiParam, VTActor};
use utf8parse::Parser as Utf8Parser;

#[cfg(any(feature = "std", feature = "alloc"))]
use alloc::vec::Vec;
#[cfg(all(not(feature = "std"), not(feature = "alloc")))]
use heapless::Vec;

#[inline(always)]
fn lookup(state: State, b: u8) -> (Action, State) {
    // `state` is one of the 15 table-indexed states (0..14) and `b` is a u8, so
    // both indices are always in range; the bounds checks are elided by LLVM.
    let v = TRANSITIONS[state as usize][b as usize];
    (Action::from_u16(v >> 8), State::from_u16(v & 0xff))
}

#[inline(always)]
#[cfg(not(test))]
fn lookup_entry(state: State) -> Action {
    ENTRY[state as usize]
}

#[inline(always)]
#[cfg(test)]
fn lookup_entry(state: State) -> Action {
    *ENTRY
        .get(state as usize)
        .unwrap_or_else(|| panic!("State {:?} has no entry in ENTRY", state))
}

#[inline(always)]
#[cfg(test)]
fn lookup_exit(state: State) -> Action {
    *EXIT
        .get(state as usize)
        .unwrap_or_else(|| panic!("State {:?} has no entry in EXIT", state))
}

#[inline(always)]
#[cfg(not(test))]
fn lookup_exit(state: State) -> Action {
    EXIT[state as usize]
}

const MAX_INTERMEDIATES: usize = 2;
const MAX_OSC: usize = 64;
const MAX_PARAMS: usize = 256;

/// Threshold above which we proactively release excess capacity from the
/// OSC/APC scratch buffers once they've been consumed.
///
/// Below this size we deliberately *keep* the existing allocation around:
/// `Action::Clear` fires on every escape/CSI/DCS entry (i.e. very
/// frequently), and `Action::OscStart`/`Action::ApcStart` fire at the start
/// of every OSC/APC string.  Real-world sessions commonly emit many
/// similarly-sized OSC sequences in a row (window title updates, shell
/// integration markers, hyperlinks, etc.), so unconditionally calling
/// `shrink_to_fit()` after every single one just forces the allocator to
/// free and immediately re-grow the same buffer over and over, which shows
/// up as allocator churn inside `OscState::put`/`VTParser::action`.
///
/// Only buffers that grew unusually large (e.g. a big embedded image
/// payload) are shrunk back down, so we still avoid holding on to a large
/// allocation indefinitely after a one-off outlier sequence.
#[cfg(any(feature = "std", feature = "alloc"))]
const SHRINK_THRESHOLD: usize = 64 * 1024;

/// Release a scratch buffer's excess capacity, but only if it has grown
/// past `SHRINK_THRESHOLD`.  See its documentation for the rationale.
#[cfg(any(feature = "std", feature = "alloc"))]
#[inline]
fn shrink_if_oversized(buf: &mut Vec<u8>) {
    if buf.capacity() > SHRINK_THRESHOLD {
        buf.shrink_to_fit();
    }
}

struct OscState {
    #[cfg(any(feature = "std", feature = "alloc"))]
    buffer: Vec<u8>,
    #[cfg(not(any(feature = "std", feature = "alloc")))]
    buffer: heapless::Vec<u8, { MAX_OSC * 16 }>,
    param_indices: [usize; MAX_OSC],
    num_params: usize,
    full: bool,
}

impl OscState {
    fn put(&mut self, param: char) {
        if param == ';' {
            match self.num_params {
                MAX_OSC => {
                    self.full = true;
                }
                num => {
                    self.param_indices[num.saturating_sub(1)] = self.buffer.len();
                    self.num_params += 1;
                }
            }
        } else if !self.full {
            let mut buf = [0u8; 8];
            let bytes = param.encode_utf8(&mut buf).as_bytes();

            #[cfg(any(feature = "std", feature = "alloc"))]
            self.buffer.extend_from_slice(bytes);

            #[cfg(not(any(feature = "std", feature = "alloc")))]
            if self.buffer.extend_from_slice(bytes).is_err() {
                self.full = true;
                return;
            }

            if self.num_params == 0 {
                self.num_params = 1;
            }
        }
    }
}

/// The virtual terminal parser.  It works together with an implementation of `VTActor`.
pub struct VTParser {
    state: State,

    intermediates: [u8; MAX_INTERMEDIATES],
    num_intermediates: usize,
    ignored_excess_intermediates: bool,

    osc: OscState,

    params: [CsiParam; MAX_PARAMS],
    num_params: usize,
    current_param: Option<CsiParam>,
    params_full: bool,
    #[cfg(any(feature = "std", feature = "alloc"))]
    apc_data: Vec<u8>,

    utf8_parser: Utf8Parser,
    utf8_return_state: State,
}

impl VTParser {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let param_indices = [0usize; MAX_OSC];

        Self {
            state: State::Ground,
            utf8_return_state: State::Ground,

            intermediates: [0, 0],
            num_intermediates: 0,
            ignored_excess_intermediates: false,

            osc: OscState {
                buffer: Vec::new(),
                param_indices,
                num_params: 0,
                full: false,
            },

            params: [CsiParam::default(); MAX_PARAMS],
            num_params: 0,
            params_full: false,
            current_param: None,

            utf8_parser: Utf8Parser::new(),
            #[cfg(any(feature = "std", feature = "alloc"))]
            apc_data: Vec::new(),
        }
    }

    /// Returns if the state machine is in the ground state,
    /// i.e. there is no pending state held by the state machine.
    pub fn is_ground(&self) -> bool {
        self.state == State::Ground
    }

    fn as_integer_params(&self) -> [i64; MAX_PARAMS] {
        let mut res = [0i64; MAX_PARAMS];
        let mut i = 0;
        for src in &self.params[0..self.num_params] {
            if let CsiParam::Integer(value) = src {
                res[i] = *value;
            } else if let CsiParam::P(b';') = src {
                i += 1;
            }
        }
        res
    }

    fn finish_param(&mut self) {
        if let Some(val) = self.current_param.take() {
            if self.num_params < MAX_PARAMS {
                self.params[self.num_params] = val;
                self.num_params += 1;
            }
        }
    }

    /// Promote early intermediates to parameters.
    /// This is handle sequences such as DECSET that use `?`
    /// prior to other numeric parameters.
    /// `?` is technically in the intermediate range and shouldn't
    /// appear in the parameter position according to ECMA 48
    fn promote_intermediates_to_params(&mut self) {
        if self.num_intermediates > 0 {
            for &p in &self.intermediates[..self.num_intermediates] {
                if self.num_params >= MAX_PARAMS {
                    self.ignored_excess_intermediates = true;
                    break;
                }
                self.params[self.num_params] = CsiParam::P(p);
                self.num_params += 1;
            }
            self.num_intermediates = 0;
        }
    }

    fn action(&mut self, action: Action, param: u8, actor: &mut dyn VTActor) {
        match action {
            Action::None | Action::Ignore => {}
            Action::Print => actor.print(param as char),
            Action::Execute => actor.execute_c0_or_c1(param),
            Action::Clear => {
                self.num_intermediates = 0;
                self.ignored_excess_intermediates = false;
                self.osc.num_params = 0;
                self.osc.full = false;
                self.num_params = 0;
                self.params_full = false;
                self.current_param.take();
                #[cfg(any(feature = "std", feature = "alloc"))]
                {
                    self.apc_data.clear();
                    shrink_if_oversized(&mut self.apc_data);
                    self.osc.buffer.clear();
                    shrink_if_oversized(&mut self.osc.buffer);
                }
            }
            Action::Collect => {
                if self.num_intermediates < MAX_INTERMEDIATES {
                    self.intermediates[self.num_intermediates] = param;
                    self.num_intermediates += 1;
                } else {
                    self.ignored_excess_intermediates = true;
                }
            }
            Action::Param => {
                if self.params_full {
                    return;
                }

                self.promote_intermediates_to_params();

                match param {
                    b'0'..=b'9' => match self.current_param.take() {
                        Some(CsiParam::Integer(i)) => {
                            self.current_param.replace(CsiParam::Integer(
                                i.saturating_mul(10).saturating_add((param - b'0') as i64),
                            ));
                        }
                        Some(_) => unreachable!(),
                        None => {
                            self.current_param
                                .replace(CsiParam::Integer((param - b'0') as i64));
                        }
                    },
                    p => {
                        self.finish_param();

                        if self.num_params + 1 > MAX_PARAMS {
                            self.params_full = true;
                        } else {
                            self.params[self.num_params] = CsiParam::P(p);
                            self.num_params += 1;
                        }
                    }
                }
            }
            Action::Hook => {
                self.finish_param();
                actor.dcs_hook(
                    param,
                    &self.as_integer_params()[0..self.num_params],
                    &self.intermediates[0..self.num_intermediates],
                    self.ignored_excess_intermediates,
                );
            }
            Action::Put => actor.dcs_put(param),
            Action::EscDispatch => {
                self.finish_param();
                actor.esc_dispatch(
                    &self.as_integer_params()[0..self.num_params],
                    &self.intermediates[0..self.num_intermediates],
                    self.ignored_excess_intermediates,
                    param,
                );
            }
            Action::CsiDispatch => {
                self.finish_param();
                self.promote_intermediates_to_params();
                actor.csi_dispatch(
                    &self.params[0..self.num_params],
                    self.ignored_excess_intermediates,
                    param,
                );
            }
            Action::Unhook => actor.dcs_unhook(),
            Action::OscStart => {
                self.osc.buffer.clear();
                #[cfg(any(feature = "std", feature = "alloc"))]
                shrink_if_oversized(&mut self.osc.buffer);
                self.osc.num_params = 0;
                self.osc.full = false;
            }
            Action::OscPut => self.osc.put(param as char),

            Action::OscEnd => {
                if self.osc.num_params == 0 {
                    actor.osc_dispatch(&[]);
                } else {
                    let mut params: [&[u8]; MAX_OSC] = [b""; MAX_OSC];
                    let mut offset = 0usize;
                    let mut slice = self.osc.buffer.as_slice();
                    let limit = self.osc.num_params.min(MAX_OSC);
                    #[allow(clippy::needless_range_loop)]
                    for i in 0..limit - 1 {
                        let (a, b) = slice.split_at(self.osc.param_indices[i] - offset);
                        params[i] = a;
                        slice = b;
                        offset = self.osc.param_indices[i];
                    }
                    params[limit - 1] = slice;
                    actor.osc_dispatch(&params[0..limit]);
                }
            }

            Action::ApcStart => {
                #[cfg(any(feature = "std", feature = "alloc"))]
                {
                    self.apc_data.clear();
                    shrink_if_oversized(&mut self.apc_data);
                }
            }
            Action::ApcPut => {
                #[cfg(any(feature = "std", feature = "alloc"))]
                self.apc_data.push(param);
            }
            Action::ApcEnd => {
                #[cfg(any(feature = "std", feature = "alloc"))]
                actor.apc_dispatch(core::mem::take(&mut self.apc_data));
            }

            Action::Utf8 => self.next_utf8(actor, param),
        }
    }

    // Process a utf-8 multi-byte sequence.
    // The state tables emit Action::Utf8 to initiate a multi-byte
    // sequence, and once we're in the utf-8 state we'll defer to
    // this method for each byte until the Decode struct is signalled
    // that we're done.
    // We use the REPLACEMENT_CHARACTER for invalid sequences.
    // We return to the ground state after each codepoint, successful
    // or otherwise.
    fn next_utf8(&mut self, actor: &mut dyn VTActor, byte: u8) {
        struct Decoder {
            codepoint: Option<char>,
        }

        impl utf8parse::Receiver for Decoder {
            fn codepoint(&mut self, c: char) {
                self.codepoint.replace(c);
            }

            fn invalid_sequence(&mut self) {
                self.codepoint(char::REPLACEMENT_CHARACTER);
            }
        }

        let mut decoder = Decoder { codepoint: None };

        self.utf8_parser.advance(&mut decoder, byte);
        if let Some(c) = decoder.codepoint {
            // Slightly gross special cases C1 controls that were
            // encoded as UTF-8 rather than emitted as raw 8-bit.
            // If the decoded value is in the byte range, and that
            // value would cause a state transition, then we process
            // that state transition rather than performing the default
            // string accumulation.
            if c as u32 <= 0xff {
                let byte = ((c as u32) & 0xff) as u8;

                let (action, state) = lookup(self.utf8_return_state, byte);
                if action == Action::Execute
                    || (state != self.utf8_return_state && state != State::Utf8Sequence)
                {
                    self.action(lookup_exit(self.utf8_return_state), 0, actor);
                    self.action(action, byte, actor);
                    self.action(lookup_entry(state), 0, actor);
                    self.utf8_return_state = self.state;
                    self.state = state;
                    return;
                }
            }

            match self.utf8_return_state {
                State::Ground => actor.print(c),
                State::OscString => self.osc.put(c),
                state => panic!("unreachable state {:?}", state),
            };
            self.state = self.utf8_return_state;
        }
    }

    /// Parse a single byte.  This may result in a call to one of the
    /// methods on the provided `actor`.
    #[inline(always)]
    pub fn parse_byte(&mut self, byte: u8, actor: &mut dyn VTActor) {
        // While in utf-8 parsing mode, co-opt the vt state
        // table and instead use the utf-8 state table from the
        // parser.  It will drop us back into the Ground state
        // after each recognized (or invalid) codepoint.
        if self.state == State::Utf8Sequence {
            self.next_utf8(actor, byte);
            return;
        }

        let (action, state) = lookup(self.state, byte);

        if state != self.state {
            if state != State::Utf8Sequence {
                self.action(lookup_exit(self.state), 0, actor);
            }
            self.action(action, byte, actor);
            self.action(lookup_entry(state), byte, actor);
            self.utf8_return_state = self.state;
            self.state = state;
        } else {
            self.action(action, byte, actor);
        }
    }

    /// Parse a sequence of bytes.  The sequence need not be complete.
    /// This may result in some number of calls to the methods on the
    /// provided `actor`.
    ///
    /// While the state machine is in the Ground state, runs of printable
    /// ASCII bytes (0x20..=0x7f) are collected and handed to the actor in
    /// a single `print_run` call rather than one `print` per byte; see
    /// `VTActor::print_run`.  Everything else is parsed byte by byte, so
    /// the chunking of the input cannot change the resulting events: a run
    /// that is cut short by the end of a chunk simply ends there, and the
    /// remainder starts a new run in the next chunk.
    pub fn parse(&mut self, bytes: &[u8], actor: &mut dyn VTActor) {
        let mut pos = 0;
        while pos < bytes.len() {
            if self.state == State::Ground {
                let start = pos;
                while pos < bytes.len() && (0x20..=0x7f).contains(&bytes[pos]) {
                    pos += 1;
                }
                if pos > start {
                    // In Ground, every byte in 0x20..=0x7f maps to
                    // (Print, Ground): no state change, no entry/exit
                    // action, no parser state is consulted.  Bulk printing
                    // the run is therefore equivalent to running
                    // `parse_byte` over each of its bytes.
                    actor.print_run(&bytes[start..pos]);
                    continue;
                }
            }
            let byte = bytes[pos];
            self.parse_byte(byte, actor);
            pos += 1;
        }
    }

    /// Parse a sequence of bytes strictly one byte at a time via
    /// `parse_byte`, which is the algorithm `parse` implemented before the
    /// Ground-state printable-ASCII fast path was added.
    ///
    /// This exists purely as a differential reference for the tests: it
    /// must always produce the same sequence of actor calls as `parse`.
    #[cfg(test)]
    pub fn parse_reference(&mut self, bytes: &[u8], actor: &mut dyn VTActor) {
        for &b in bytes {
            self.parse_byte(b, actor);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{CollectingVTActor, VTAction};
    use k9::assert_equal as assert_eq;

    /// A `VTActor` that records the size of every `print_run` call while
    /// forwarding everything to a `CollectingVTActor`, so that tests can
    /// check both the resulting actions and how the printable runs were
    /// grouped.
    #[derive(Default)]
    struct RunRecordingActor {
        inner: CollectingVTActor,
        run_lens: Vec<usize>,
    }

    impl VTActor for RunRecordingActor {
        fn print(&mut self, b: char) {
            self.inner.print(b);
        }

        fn print_run(&mut self, bytes: &[u8]) {
            self.run_lens.push(bytes.len());
            self.inner.print_run(bytes);
        }

        fn execute_c0_or_c1(&mut self, control: u8) {
            self.inner.execute_c0_or_c1(control);
        }

        fn dcs_hook(
            &mut self,
            byte: u8,
            params: &[i64],
            intermediates: &[u8],
            ignored_excess_intermediates: bool,
        ) {
            self.inner
                .dcs_hook(byte, params, intermediates, ignored_excess_intermediates);
        }

        fn dcs_put(&mut self, byte: u8) {
            self.inner.dcs_put(byte);
        }

        fn dcs_unhook(&mut self) {
            self.inner.dcs_unhook();
        }

        fn esc_dispatch(
            &mut self,
            params: &[i64],
            intermediates: &[u8],
            ignored_excess_intermediates: bool,
            byte: u8,
        ) {
            self.inner
                .esc_dispatch(params, intermediates, ignored_excess_intermediates, byte);
        }

        fn csi_dispatch(&mut self, params: &[CsiParam], parameters_truncated: bool, byte: u8) {
            self.inner.csi_dispatch(params, parameters_truncated, byte);
        }

        fn osc_dispatch(&mut self, params: &[&[u8]]) {
            self.inner.osc_dispatch(params);
        }

        fn apc_dispatch(&mut self, data: Vec<u8>) {
            self.inner.apc_dispatch(data);
        }
    }

    fn parse_as_vec(bytes: &[u8]) -> Vec<VTAction> {
        let mut parser = VTParser::new();
        let mut actor = CollectingVTActor::default();
        parser.parse(bytes, &mut actor);
        actor.into_vec()
    }

    #[test]
    fn test_mixed() {
        assert_eq!(
            parse_as_vec(b"yo\x07\x1b[32mwoot\x1b[0mdone"),
            vec![
                VTAction::Print('y'),
                VTAction::Print('o'),
                VTAction::ExecuteC0orC1(0x07,),
                VTAction::CsiDispatch {
                    params: vec![CsiParam::Integer(32)],
                    parameters_truncated: false,
                    byte: b'm',
                },
                VTAction::Print('w',),
                VTAction::Print('o',),
                VTAction::Print('o',),
                VTAction::Print('t',),
                VTAction::CsiDispatch {
                    params: vec![CsiParam::Integer(0)],
                    parameters_truncated: false,
                    byte: b'm',
                },
                VTAction::Print('d',),
                VTAction::Print('o',),
                VTAction::Print('n',),
                VTAction::Print('e',),
            ]
        );
    }

    #[test]
    fn test_print() {
        assert_eq!(
            parse_as_vec(b"yo"),
            vec![VTAction::Print('y'), VTAction::Print('o')]
        );
    }

    #[test]
    fn test_osc_with_c1_st() {
        assert_eq!(
            parse_as_vec(b"\x1b]0;there\x9c"),
            vec![VTAction::OscDispatch(vec![
                b"0".to_vec(),
                b"there".to_vec()
            ])]
        );
    }

    #[test]
    fn test_osc_with_bel_st() {
        assert_eq!(
            parse_as_vec(b"\x1b]0;hello\x07"),
            vec![VTAction::OscDispatch(vec![
                b"0".to_vec(),
                b"hello".to_vec()
            ])]
        );
    }

    #[test]
    fn test_decset() {
        assert_eq!(
            parse_as_vec(b"\x1b[?1l"),
            vec![VTAction::CsiDispatch {
                params: vec![CsiParam::P(b'?'), CsiParam::Integer(1)],
                parameters_truncated: false,
                byte: b'l',
            },]
        );
    }

    #[test]
    fn test_osc_too_many_params() {
        let fields = (0..MAX_OSC + 2)
            .into_iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>();
        let input = format!("\x1b]{}\x07", fields.join(";"));
        let actions = parse_as_vec(input.as_bytes());
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            VTAction::OscDispatch(parsed_fields) => {
                let fields: Vec<_> = fields.into_iter().map(|s| s.as_bytes().to_vec()).collect();
                assert_eq!(parsed_fields.as_slice(), &fields[0..MAX_OSC]);
            }
            other => panic!("Expected OscDispatch but got {:?}", other),
        }
    }

    #[test]
    fn test_osc_with_no_params() {
        assert_eq!(
            parse_as_vec(b"\x1b]\x07"),
            vec![VTAction::OscDispatch(vec![])]
        );
    }

    #[test]
    fn test_osc_with_esc_sequence_st() {
        // This case isn't the same as the other OSC cases; even though
        // `ESC \` is the long form escape sequence for ST, the ESC on its
        // own breaks out of the OSC state and jumps into the ESC state,
        // and that leaves the `\` character to be dispatched there in
        // the calling application.
        assert_eq!(
            parse_as_vec(b"\x1b]woot\x1b\\"),
            vec![
                VTAction::OscDispatch(vec![b"woot".to_vec()]),
                VTAction::EscDispatch {
                    params: vec![],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                    byte: b'\\'
                }
            ]
        );
    }

    #[test]
    fn test_fancy_underline() {
        assert_eq!(
            parse_as_vec(b"\x1b[4m"),
            vec![VTAction::CsiDispatch {
                params: vec![CsiParam::Integer(4)],
                parameters_truncated: false,
                byte: b'm'
            }]
        );

        assert_eq!(
            // This is the kitty curly underline sequence.
            parse_as_vec(b"\x1b[4:3m"),
            vec![VTAction::CsiDispatch {
                params: vec![
                    CsiParam::Integer(4),
                    CsiParam::P(b':'),
                    CsiParam::Integer(3)
                ],
                parameters_truncated: false,
                byte: b'm'
            }]
        );
    }

    #[test]
    fn test_colon_rgb() {
        assert_eq!(
            parse_as_vec(b"\x1b[38:2::128:64:192m"),
            vec![VTAction::CsiDispatch {
                params: vec![
                    CsiParam::Integer(38),
                    CsiParam::P(b':'),
                    CsiParam::Integer(2),
                    CsiParam::P(b':'),
                    CsiParam::P(b':'),
                    CsiParam::Integer(128),
                    CsiParam::P(b':'),
                    CsiParam::Integer(64),
                    CsiParam::P(b':'),
                    CsiParam::Integer(192),
                ],
                parameters_truncated: false,
                byte: b'm'
            }]
        );
    }

    #[test]
    fn test_csi_omitted_param() {
        assert_eq!(
            parse_as_vec(b"\x1b[;1m"),
            vec![VTAction::CsiDispatch {
                params: vec![CsiParam::P(b';'), CsiParam::Integer(1)],
                parameters_truncated: false,
                byte: b'm'
            }]
        );
    }

    #[test]
    fn test_csi_too_many_params() {
        // Due to the much higher CSI element limit,
        // we must construct this test differently.
        let mut input = "\x1b[0".to_string();
        let mut params = vec![CsiParam::default()];

        for n in 1..=127 {
            input.push_str(&format!(";{n}"));
            params.push(CsiParam::P(b';'));
            params.push(CsiParam::Integer(n));
        }
        input.push_str(";128");

        input.push('p');
        params.push(CsiParam::P(b';'));

        assert_eq!(
            parse_as_vec(input.as_bytes()),
            vec![VTAction::CsiDispatch {
                params,
                parameters_truncated: false,
                byte: b'p'
            }]
        );
    }

    #[test]
    fn test_csi_intermediates() {
        assert_eq!(
            parse_as_vec(b"\x1b[1 p"),
            vec![VTAction::CsiDispatch {
                params: vec![CsiParam::Integer(1), CsiParam::P(b' ')],
                parameters_truncated: false,
                byte: b'p'
            }]
        );
        assert_eq!(
            parse_as_vec(b"\x1b[1 !p"),
            vec![VTAction::CsiDispatch {
                params: vec![CsiParam::Integer(1), CsiParam::P(b' '), CsiParam::P(b'!')],
                parameters_truncated: false,
                byte: b'p'
            }]
        );
        assert_eq!(
            parse_as_vec(b"\x1b[1 !#p"),
            vec![VTAction::CsiDispatch {
                // Note that the `#` was discarded
                params: vec![CsiParam::Integer(1), CsiParam::P(b' '), CsiParam::P(b'!')],
                parameters_truncated: true,
                byte: b'p'
            }]
        );
    }

    #[test]
    fn osc_utf8() {
        assert_eq!(
            parse_as_vec("\x1b]\u{af}\x07".as_bytes()),
            vec![VTAction::OscDispatch(vec!["\u{af}".as_bytes().to_vec()])]
        );
    }

    #[test]
    fn osc_fedora_vte() {
        assert_eq!(
            parse_as_vec("\u{9d}777;preexec\u{9c}".as_bytes()),
            vec![VTAction::OscDispatch(vec![
                b"777".to_vec(),
                b"preexec".to_vec(),
            ])]
        );
    }

    #[test]
    fn print_utf8() {
        assert_eq!(
            parse_as_vec("\u{af}".as_bytes()),
            vec![VTAction::Print('\u{af}')]
        );
    }

    #[test]
    fn utf8_control() {
        assert_eq!(
            parse_as_vec("\u{8d}".as_bytes()),
            vec![VTAction::ExecuteC0orC1(0x8d)]
        );
    }

    #[test]
    fn tmux_control() {
        assert_eq!(
            parse_as_vec("\x1bP1000phello\x1b\\".as_bytes()),
            vec![
                VTAction::DcsHook {
                    byte: b'p',
                    params: vec![1000],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                },
                VTAction::DcsPut(b'h'),
                VTAction::DcsPut(b'e'),
                VTAction::DcsPut(b'l'),
                VTAction::DcsPut(b'l'),
                VTAction::DcsPut(b'o'),
                VTAction::DcsUnhook,
                VTAction::EscDispatch {
                    params: vec![],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                    byte: b'\\',
                }
            ]
        );
    }

    #[test]
    fn tmux_passthru() {
        // I'm not convinced that we *should* represent this tmux sequence
        // in this way, but it is how it currently maps.
        // It's worth noting that we see this as final byte `t` here, which
        // collides with decVT105G in https://vt100.net/emu/dcsseq_dec.html
        assert_eq!(
            parse_as_vec("\x1bPtmux;data\x1b\\".as_bytes()),
            vec![
                VTAction::DcsHook {
                    byte: b't',
                    params: vec![],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                },
                VTAction::DcsPut(b'm'),
                VTAction::DcsPut(b'u'),
                VTAction::DcsPut(b'x'),
                VTAction::DcsPut(b';'),
                VTAction::DcsPut(b'd'),
                VTAction::DcsPut(b'a'),
                VTAction::DcsPut(b't'),
                VTAction::DcsPut(b'a'),
                VTAction::DcsUnhook,
                VTAction::EscDispatch {
                    params: vec![],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                    byte: b'\\',
                }
            ]
        );
    }

    #[test]
    fn kitty_img() {
        assert_eq!(
            parse_as_vec("\x1b_Gf=24,s=10,v=20;payload\x1b\\".as_bytes()),
            vec![
                VTAction::ApcDispatch(b"Gf=24,s=10,v=20;payload".to_vec()),
                VTAction::EscDispatch {
                    params: vec![],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                    byte: b'\\',
                }
            ]
        );
    }

    #[test]
    fn sixel() {
        assert_eq!(
            parse_as_vec("\x1bPqhello\x1b\\".as_bytes()),
            vec![
                VTAction::DcsHook {
                    byte: b'q',
                    params: vec![],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                },
                VTAction::DcsPut(b'h'),
                VTAction::DcsPut(b'e'),
                VTAction::DcsPut(b'l'),
                VTAction::DcsPut(b'l'),
                VTAction::DcsPut(b'o'),
                VTAction::DcsUnhook,
                VTAction::EscDispatch {
                    params: vec![],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                    byte: b'\\',
                }
            ]
        );
    }

    #[test]
    fn test_ommitted_dcs_param() {
        assert_eq!(
            parse_as_vec("\x1bP;1q\x1b\\".as_bytes()),
            vec![
                VTAction::DcsHook {
                    byte: b'q',
                    params: vec![0, 1],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                },
                VTAction::DcsUnhook,
                VTAction::EscDispatch {
                    params: vec![],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                    byte: b'\\',
                }
            ]
        );
    }

    /// Repeated small/medium OSC sequences (eg. window title updates) are a
    /// common real-world pattern.  We should not be freeing and
    /// immediately re-growing the OSC scratch buffer on every single one
    /// of them; the allocation should be reused across sequences as long
    /// as it doesn't grow past `SHRINK_THRESHOLD`.
    #[test]
    fn osc_buffer_capacity_is_reused_for_small_sequences() {
        let mut parser = VTParser::new();
        let mut actor = CollectingVTActor::default();

        parser.parse(b"\x1b]0;first title\x07", &mut actor);
        let cap_after_first = parser.osc.buffer.capacity();
        assert!(cap_after_first > 0);

        // A subsequent CSI sequence (Action::Clear on CsiEntry) must not
        // discard the OSC buffer's capacity.
        parser.parse(b"\x1b[1;32m", &mut actor);
        assert_eq!(
            parser.osc.buffer.capacity(),
            cap_after_first,
            "CSI entry should not shrink an OSC buffer within the reuse threshold"
        );

        // Starting a new OSC sequence of similar size should reuse the
        // existing allocation rather than reallocating from scratch.
        parser.parse(b"\x1b]0;second title\x07", &mut actor);
        assert_eq!(
            parser.osc.buffer.capacity(),
            cap_after_first,
            "starting a new small OSC sequence should reuse the existing buffer capacity"
        );
    }

    /// An unusually large OSC/APC payload (eg. a big embedded image) should
    /// still have its buffer capacity released afterwards, so that we don't
    /// hold on to a large allocation for the remaining lifetime of the
    /// parser.
    #[test]
    fn oversized_osc_buffer_is_shrunk_after_use() {
        let mut parser = VTParser::new();
        let mut actor = CollectingVTActor::default();

        let huge_payload = "a".repeat(SHRINK_THRESHOLD * 2);
        let sequence = format!("\x1b]0;{}\x07", huge_payload);
        parser.parse(sequence.as_bytes(), &mut actor);
        assert!(parser.osc.buffer.capacity() > SHRINK_THRESHOLD);

        // Starting the next OSC sequence should trigger the shrink since
        // the buffer is well over the threshold.
        parser.parse(b"\x1b]0;small\x07", &mut actor);
        assert!(
            parser.osc.buffer.capacity() <= SHRINK_THRESHOLD,
            "oversized OSC buffer should be released once it exceeds the threshold, got capacity {}",
            parser.osc.buffer.capacity()
        );
    }

    /// The APC scratch buffer is handed off via `mem::take` in
    /// `Action::ApcEnd` (its contents are passed by value to
    /// `VTActor::apc_dispatch`), so unlike the OSC buffer it never actually
    /// carries capacity into the next sequence. This just confirms that
    /// repeated APC sequences interleaved with CSI sequences keep working
    /// correctly now that the scratch-buffer shrink is conditional.
    #[test]
    fn apc_sequences_still_dispatch_correctly_after_shrink_change() {
        assert_eq!(
            parse_as_vec(b"\x1b_Gf=24,s=10,v=20;payload\x1b\\\x1b[1;32m\x1b_Ga=1;more\x1b\\"),
            vec![
                VTAction::ApcDispatch(b"Gf=24,s=10,v=20;payload".to_vec()),
                VTAction::EscDispatch {
                    params: vec![],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                    byte: b'\\',
                },
                VTAction::CsiDispatch {
                    params: vec![
                        CsiParam::Integer(1),
                        CsiParam::P(b';'),
                        CsiParam::Integer(32)
                    ],
                    parameters_truncated: false,
                    byte: b'm',
                },
                VTAction::ApcDispatch(b"Ga=1;more".to_vec()),
                VTAction::EscDispatch {
                    params: vec![],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                    byte: b'\\',
                },
            ]
        );
    }

    /// Deterministic PRNG (splitmix64) so that the differential tests are
    /// reproducible without pulling in an external `rand` dependency.
    struct Rng(u64);

    impl Rng {
        fn new(seed: u64) -> Self {
            Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
        }

        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn below(&mut self, bound: usize) -> usize {
            if bound == 0 {
                0
            } else {
                (self.next_u64() % bound as u64) as usize
            }
        }

        fn byte(&mut self) -> u8 {
            (self.next_u64() & 0xff) as u8
        }
    }

    /// Build a random byte stream mixing printable ASCII runs, C0/C1
    /// controls, escape sequences (CSI, OSC, DCS, APC, ESC and their 8-bit
    /// forms), UTF-8 multi-byte characters (sometimes truncated mid
    /// sequence) and invalid/stray bytes.
    fn gen_bytes(rng: &mut Rng, events: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for _ in 0..events {
            match rng.below(18) {
                // A run of printable ASCII, sometimes long enough that the
                // chunked variants of the tests cut it in half.
                0..=2 => {
                    let len = 1 + rng.below(64);
                    for _ in 0..len {
                        out.push(0x20 + rng.below(0x60) as u8);
                    }
                }
                // C0 controls (includes ESC, CAN and SUB)
                3 => out.push(rng.below(0x20) as u8),
                // DEL, which is printed in the Ground state
                4 => out.push(0x7f),
                // 8-bit controls and introducers
                5 => out.push(0x80 + rng.below(0x20) as u8),
                // Any byte at all, including invalid utf-8 lead bytes
                6 => out.push(rng.byte()),
                7 => {
                    // CSI: ESC [ params intermediate final
                    out.push(0x1b);
                    out.push(b'[');
                    for _ in 0..rng.below(4) {
                        out.extend_from_slice(format!("{}", rng.below(1000)).as_bytes());
                        out.push(b';');
                    }
                    out.push(0x40 + rng.below(0x3f) as u8);
                }
                8 => {
                    // OSC: ESC ] code ; payload, terminated by BEL or ST
                    out.push(0x1b);
                    out.push(b']');
                    out.extend_from_slice(format!("{}", rng.below(8)).as_bytes());
                    out.push(b';');
                    for _ in 0..rng.below(20) {
                        out.push(0x20 + rng.below(0x5f) as u8);
                    }
                    out.push(if rng.below(2) == 0 { 0x07 } else { 0x9c });
                }
                9 => {
                    // DCS: ESC P params final payload ST
                    out.push(0x1b);
                    out.push(b'P');
                    for _ in 0..rng.below(3) {
                        out.extend_from_slice(format!("{}", rng.below(100)).as_bytes());
                        out.push(b';');
                    }
                    out.push(0x40 + rng.below(0x3f) as u8);
                    for _ in 0..rng.below(20) {
                        out.push(0x20 + rng.below(0x5f) as u8);
                    }
                    out.extend_from_slice(b"\x1b\\");
                }
                10 => {
                    // APC: ESC _ payload ST
                    out.push(0x1b);
                    out.push(b'_');
                    for _ in 0..rng.below(20) {
                        out.push(0x20 + rng.below(0x5f) as u8);
                    }
                    out.extend_from_slice(b"\x1b\\");
                }
                11 => {
                    // ESC, a couple of intermediates and a final byte
                    out.push(0x1b);
                    for _ in 0..rng.below(3) {
                        out.push(0x20 + rng.below(0x10) as u8);
                    }
                    out.push(0x30 + rng.below(0x4f) as u8);
                }
                12 => out.push(0x9b), // 8-bit CSI
                13 => out.push(0x9d), // 8-bit OSC
                14 => out.push(0x90), // 8-bit DCS
                15 => {
                    // A 2, 3 or 4 byte utf-8 sequence; a quarter of the time
                    // the continuation bytes are dropped so that the next
                    // chunk has to finish the sequence.
                    let len = 2 + rng.below(3);
                    let lead: u8 = match len {
                        2 => 0xc2 + rng.below(0x1e) as u8,
                        3 => 0xe0 + rng.below(0x10) as u8,
                        _ => 0xf0 + rng.below(5) as u8,
                    };
                    out.push(lead);
                    for _ in 0..(len - 1) {
                        out.push(0x80 + rng.below(0x40) as u8);
                    }
                    if rng.below(4) == 0 {
                        out.truncate(out.len() - (len - 1));
                    }
                }
                16 => out.extend_from_slice(b"\x1b[0m"),
                _ => match rng.below(6) {
                    0 => out.push(0x98),
                    1 => out.push(0x9e),
                    2 => out.push(0x9f),
                    3 => out.push(0x9c),
                    4 => out.push(0x18),
                    _ => out.push(0x1a),
                },
            }
        }
        out
    }

    fn parse_fast(bytes: &[u8]) -> Vec<VTAction> {
        let mut parser = VTParser::new();
        let mut actor = CollectingVTActor::default();
        parser.parse(bytes, &mut actor);
        actor.into_vec()
    }

    /// Parse `bytes` in fixed size chunks with a single parser instance.
    fn parse_chunked(bytes: &[u8], chunk: usize) -> Vec<VTAction> {
        let mut parser = VTParser::new();
        let mut actor = CollectingVTActor::default();
        for chunk in bytes.chunks(chunk.max(1)) {
            parser.parse(chunk, &mut actor);
        }
        actor.into_vec()
    }

    /// Parse `bytes`, cutting it at the given sizes, with a single parser
    /// instance.  Any trailing bytes are parsed as a final chunk.
    fn parse_random_chunks(bytes: &[u8], plan: &[usize]) -> Vec<VTAction> {
        let mut parser = VTParser::new();
        let mut actor = CollectingVTActor::default();
        let mut pos = 0;
        for &len in plan {
            if pos >= bytes.len() {
                break;
            }
            let end = (pos + len.max(1)).min(bytes.len());
            parser.parse(&bytes[pos..end], &mut actor);
            pos = end;
        }
        if pos < bytes.len() {
            parser.parse(&bytes[pos..], &mut actor);
        }
        actor.into_vec()
    }

    /// The bulk printable-ASCII fast path in `parse` must produce exactly
    /// the same sequence of actions as the original per-byte algorithm, for
    /// the whole stream and for every chunking of it.
    #[test]
    fn fast_path_matches_reference_for_random_streams() {
        for seed in 0..256u64 {
            let mut rng = Rng::new(seed);
            let events = 1 + rng.below(600);
            let stream = gen_bytes(&mut rng, events);

            let mut reference_parser = VTParser::new();
            let mut reference_actor = CollectingVTActor::default();
            reference_parser.parse_reference(&stream, &mut reference_actor);
            let reference = reference_actor.into_vec();

            assert_eq!(
                parse_fast(&stream),
                reference,
                "seed {}: whole-stream parse diverged",
                seed
            );

            for &chunk in &[1usize, 2, 3, 5, 7, 13, 64, 4096] {
                assert_eq!(
                    parse_chunked(&stream, chunk),
                    reference,
                    "seed {}: chunk size {} diverged",
                    seed,
                    chunk
                );
            }

            // A random chunking, including single byte chunks.
            let mut plan = Vec::new();
            let mut remaining = stream.len();
            while remaining > 0 {
                let len = 1 + rng.below(128);
                plan.push(len);
                remaining = remaining.saturating_sub(len);
            }
            assert_eq!(
                parse_random_chunks(&stream, &plan),
                reference,
                "seed {}: random chunking diverged",
                seed
            );
        }
    }

    /// `print_run` must be handed maximal runs of printable ASCII bytes
    /// while the parser is in the Ground state, and nothing at all while it
    /// is inside an escape sequence.
    #[test]
    fn fast_path_reports_maximal_ground_runs() {
        let input = b"abc\x1b[0mde\x07f";

        let mut parser = VTParser::new();
        let mut actor = RunRecordingActor::default();
        parser.parse(input, &mut actor);
        assert_eq!(actor.run_lens, vec![3, 2, 1]);
        assert_eq!(actor.inner.into_vec(), parse_fast(input));

        // The same input, chunked at different points.  A run is cut short
        // by a chunk boundary and the remainder is reported as a new run,
        // but the resulting actions are always the same.
        let cases: &[(&[usize], &[usize])] = &[
            (&[12], &[3, 2, 1]),
            (&[6, 6], &[3, 2, 1]),
            (&[3, 1, 8], &[3, 2, 1]),
            (&[3, 4, 2, 3], &[3, 2, 1]),
            (&[4, 4, 4], &[3, 1, 1, 1]),
            (&[1; 12], &[1; 6]),
        ];
        for (plan, expected_runs) in cases {
            let mut parser = VTParser::new();
            let mut actor = RunRecordingActor::default();
            let mut pos = 0;
            for &len in plan.iter() {
                if pos >= input.len() {
                    break;
                }
                let end = (pos + len).min(input.len());
                parser.parse(&input[pos..end], &mut actor);
                pos = end;
            }
            assert_eq!(pos, input.len(), "plan {:?} does not cover the input", plan);
            assert_eq!(
                actor.run_lens,
                expected_runs.to_vec(),
                "unexpected print runs for plan {:?}",
                plan
            );
            assert_eq!(
                actor.inner.into_vec(),
                parse_fast(input),
                "actions diverged for plan {:?}",
                plan
            );
        }
    }

    #[test]
    fn fast_path_empty_input() {
        assert_eq!(parse_fast(b""), Vec::<VTAction>::new());

        let mut parser = VTParser::new();
        let mut actor = CollectingVTActor::default();
        parser.parse(b"", &mut actor);
        assert_eq!(actor.into_vec(), Vec::<VTAction>::new());
    }

    /// A run that ends exactly at a chunk boundary, including the case where
    /// an escape sequence starts on the very next byte.
    #[test]
    fn fast_path_run_ending_at_chunk_end() {
        let stream = b"abc\x1b[0mxyz";
        let reference = {
            let mut parser = VTParser::new();
            let mut actor = CollectingVTActor::default();
            parser.parse_reference(stream, &mut actor);
            actor.into_vec()
        };
        assert_eq!(
            reference,
            vec![
                VTAction::Print('a'),
                VTAction::Print('b'),
                VTAction::Print('c'),
                VTAction::CsiDispatch {
                    params: vec![CsiParam::Integer(0)],
                    parameters_truncated: false,
                    byte: b'm',
                },
                VTAction::Print('x'),
                VTAction::Print('y'),
                VTAction::Print('z'),
            ]
        );
        for len in 1..=stream.len() {
            assert_eq!(
                parse_chunked(stream, len),
                reference,
                "chunk size {} diverged",
                len
            );
        }
    }

    /// DEL (0x7f) is a printable character in the Ground state, so it must
    /// be reported through the fast path just like any other byte in
    /// 0x20..=0x7f.
    #[test]
    fn fast_path_prints_del_in_ground() {
        assert_eq!(
            parse_fast(b"\x7f"),
            vec![VTAction::Print('\x7f')],
            "DEL should be printed in the Ground state"
        );

        let stream = b"abc\x7fdef";
        let mut parser = VTParser::new();
        let mut actor = CollectingVTActor::default();
        parser.parse_reference(stream, &mut actor);
        assert_eq!(parse_fast(stream), actor.into_vec());
    }

    /// Printable bytes inside an OSC or DCS payload are not in the Ground
    /// state, so they must not go through the fast path: they are OSC/DCS
    /// data instead of printed characters.
    #[test]
    fn fast_path_not_used_inside_osc_or_dcs_payload() {
        let stream = b"\x1b]0;hello world\x07\x1bP1;2qabc def\x1b\\";

        let mut parser = VTParser::new();
        let mut actor = RunRecordingActor::default();
        parser.parse(stream, &mut actor);
        // No printable run is ever reported: everything between the OSC/DCS
        // introducer and their terminators belongs to those sequences.
        assert!(
            actor.run_lens.is_empty(),
            "unexpected print runs inside escape sequence payloads: {:?}",
            actor.run_lens
        );
        assert_eq!(
            actor.inner.into_vec(),
            parse_fast(stream),
            "OSC/DCS payload must not be treated as printable text"
        );

        assert_eq!(
            parse_fast(stream),
            vec![
                VTAction::OscDispatch(vec![b"0".to_vec(), b"hello world".to_vec()]),
                VTAction::DcsHook {
                    params: vec![1, 2, 0],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                    byte: b'q',
                },
                VTAction::DcsPut(b'a'),
                VTAction::DcsPut(b'b'),
                VTAction::DcsPut(b'c'),
                VTAction::DcsPut(b' '),
                VTAction::DcsPut(b'd'),
                VTAction::DcsPut(b'e'),
                VTAction::DcsPut(b'f'),
                VTAction::DcsUnhook,
                VTAction::EscDispatch {
                    params: vec![],
                    intermediates: vec![],
                    ignored_excess_intermediates: false,
                    byte: b'\\',
                },
            ]
        );
    }

    /// Printable ASCII that follows a utf-8 sequence which was split across
    /// chunk boundaries must still be printed by the fast path, and the
    /// partially received sequence must be finished by the next chunk.
    #[test]
    fn fast_path_after_utf8_split_across_chunks() {
        let stream = "a\u{451}bc\u{1f915}d".as_bytes().to_vec();
        let mut parser = VTParser::new();
        let mut actor = CollectingVTActor::default();
        parser.parse_reference(&stream, &mut actor);
        let reference = actor.into_vec();

        assert_eq!(parse_fast(&stream), reference);

        // Every chunk size, so that each possible split of the multi-byte
        // sequences is exercised.
        for len in 1..=stream.len() {
            assert_eq!(
                parse_chunked(&stream, len),
                reference,
                "chunk size {} diverged",
                len
            );
        }

        // Explicitly: "a" plus the lead byte of "ё" in one chunk, the rest of
        // the sequence plus "bc" in the next.
        let mut parser = VTParser::new();
        let mut actor = RunRecordingActor::default();
        parser.parse(b"a\xd1", &mut actor);
        assert_eq!(actor.run_lens, vec![1], "runs: {:?}", actor.run_lens);
        parser.parse(b"\x91bc", &mut actor);
        assert_eq!(
            actor.run_lens,
            vec![1, 2],
            "the pending utf-8 sequence must be completed before printing"
        );
        let mut reference_parser = VTParser::new();
        let mut reference_actor = CollectingVTActor::default();
        reference_parser.parse_reference("a\u{451}bc".as_bytes(), &mut reference_actor);
        assert_eq!(actor.inner.into_vec(), reference_actor.into_vec());
    }
}
