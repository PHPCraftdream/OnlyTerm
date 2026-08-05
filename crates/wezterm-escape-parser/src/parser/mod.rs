#![allow(clippy::many_single_char_names)]
#[cfg(feature = "tmux_cc")]
use crate::tmux_cc::Event;
use crate::{
    Action, CSI, DeviceControlMode, EnterDeviceControlMode, Esc, OperatingSystemCommand,
    ShortDeviceControl,
};
#[cfg(feature = "tmux_cc")]
use core::borrow::BorrowMut;
use core::cell::RefCell;
use log::error;
use num_traits::FromPrimitive;
use vtparse::{CsiParam, VTActor, VTParser};

use crate::allocate::*;

mod sixel;
use sixel::SixelBuilder;

#[derive(Default)]
struct GetTcapBuilder {
    current: Vec<u8>,
    names: Vec<String>,
}

impl GetTcapBuilder {
    fn flush(&mut self) {
        let decoded = hex::decode(&self.current)
            .map(|s| String::from_utf8_lossy(&s).to_string())
            .unwrap_or_else(|_| String::from_utf8_lossy(&self.current).to_string());
        self.names.push(decoded);
        self.current.clear();
    }

    pub fn push(&mut self, data: u8) {
        if data == b';' {
            self.flush();
        } else {
            self.current.push(data);
        }
    }

    pub fn finish(mut self) -> Vec<String> {
        self.flush();
        self.names
    }
}

#[derive(Default)]
struct ParseState {
    sixel: Option<SixelBuilder>,
    dcs: Option<ShortDeviceControl>,
    get_tcap: Option<GetTcapBuilder>,
    #[cfg(feature = "tmux_cc")]
    tmux_state: Option<RefCell<crate::tmux_cc::Parser>>,
}

/// The `Parser` struct holds the state machine that is used to decode
/// a sequence of bytes.  The byte sequence can be streaming into the
/// state machine.
/// You can either have the parser trigger a callback as `Action`s are
/// decoded, or have it return a `Vec<Action>` holding zero-or-more
/// decoded actions.
pub struct Parser {
    state_machine: VTParser,
    state: RefCell<ParseState>,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    pub fn new() -> Self {
        Self {
            state_machine: VTParser::new(),
            state: RefCell::new(Default::default()),
        }
    }

    /// advance with tmux parser, bypass VTParse
    #[cfg(feature = "tmux_cc")]
    fn advance_tmux_bytes(&mut self, bytes: &[u8]) -> crate::Result<Vec<Event>> {
        let parser_state = self.state.borrow();
        let tmux_state = parser_state.tmux_state.as_ref().unwrap();
        let mut tmux_parser = tmux_state.borrow_mut();
        tmux_parser.advance_bytes(bytes)
    }

    pub fn parse<F: FnMut(Action)>(&mut self, bytes: &[u8], mut callback: F) {
        #[cfg(feature = "tmux_cc")]
        let is_tmux_mode: bool = self.state.borrow().tmux_state.is_some();
        #[cfg(feature = "tmux_cc")]
        if is_tmux_mode {
            match self.advance_tmux_bytes(bytes) {
                Ok(tmux_events) => {
                    callback(Action::DeviceControl(DeviceControlMode::TmuxEvents(
                        Box::new(tmux_events),
                    )));
                }
                Err(err_buf) => {
                    // capture bytes cannot be parsed
                    let unparsed_str = err_buf.to_string().to_owned();
                    let mut parser_state = self.state.borrow_mut();
                    parser_state.tmux_state = None;
                    let mut perform = Performer {
                        callback: &mut callback,
                        state: &mut parser_state,
                    };
                    self.state_machine
                        .parse(unparsed_str.as_bytes(), &mut perform);
                }
            }
            return;
        }

        let mut perform = Performer {
            callback: &mut callback,
            state: &mut self.state.borrow_mut(),
        };
        self.state_machine.parse(bytes, &mut perform);
    }

    /// A specialized version of the parser that halts after recognizing the
    /// first action from the stream of bytes.  The return value is the action
    /// that was recognized and the length of the byte stream that was fed in
    /// to the parser to yield it.
    pub fn parse_first(&mut self, bytes: &[u8]) -> Option<(Action, usize)> {
        // holds the first action.  We need to use RefCell to deal with
        // the Performer holding a reference to this via the closure we set up.
        let first = RefCell::new(None);
        // will hold the iterator index when we emit an action
        let mut first_idx = None;
        {
            let mut perform = Performer {
                callback: &mut |action| {
                    // capture the action, but only if it is the first one
                    // we've seen.  Preserve an existing one if any.
                    if first.borrow().is_some() {
                        return;
                    }
                    *first.borrow_mut() = Some(action);
                },
                state: &mut self.state.borrow_mut(),
            };
            for (idx, b) in bytes.iter().enumerate() {
                self.state_machine.parse_byte(*b, &mut perform);
                if first.borrow().is_some() {
                    // if we recognized an action, record the iterator index
                    first_idx = Some(idx);
                    break;
                }
            }
        }

        match (first.into_inner(), first_idx) {
            // if we matched an action, transform the iterator index to
            // the length of the string that was consumed (+1)
            (Some(action), Some(idx)) => Some((action, idx + 1)),
            _ => None,
        }
    }

    pub fn parse_as_vec(&mut self, bytes: &[u8]) -> Vec<Action> {
        let mut result = Vec::new();
        self.parse(bytes, |action| result.push(action));
        result
    }

    /// Similar to `parse_first` but collects all actions from the first sequence,
    /// and guarantees the state machine is in the ground state at the end of this
    /// sequence.
    pub fn parse_first_as_vec(&mut self, bytes: &[u8]) -> Option<(Vec<Action>, usize)> {
        let mut actions = Vec::new();
        let mut first_idx = None;
        for (idx, b) in bytes.iter().enumerate() {
            self.state_machine.parse_byte(
                *b,
                &mut Performer {
                    callback: &mut |action| actions.push(action),
                    state: &mut self.state.borrow_mut(),
                },
            );
            if !actions.is_empty() && self.state_machine.is_ground() {
                // if we recognized any actions, record the iterator index
                first_idx = Some(idx);
                break;
            }
        }
        first_idx.map(|idx| (actions, idx + 1))
    }
}

struct Performer<'a, F: FnMut(Action) + 'a> {
    callback: &'a mut F,
    state: &'a mut ParseState,
}

fn is_short_dcs(intermediates: &[u8], byte: u8) -> bool {
    if intermediates == b"$" && byte == b'q' {
        // DECRQSS
        true
    } else {
        false
    }
}

impl<'a, F: FnMut(Action)> VTActor for Performer<'a, F> {
    fn print(&mut self, c: char) {
        (self.callback)(Action::Print(c));
    }

    fn execute_c0_or_c1(&mut self, byte: u8) {
        match FromPrimitive::from_u8(byte) {
            Some(code) => (self.callback)(Action::Control(code)),
            None => error!(
                "impossible C0/C1 control code {:?} 0x{:x} was dropped",
                byte as char, byte
            ),
        }
    }

    fn apc_dispatch(&mut self, data: Vec<u8>) {
        if let Some(img) = super::KittyImage::parse_apc(&data) {
            (self.callback)(Action::KittyImage(Box::new(img)))
        } else {
            log::trace!("Ignoring APC data: {:?}", String::from_utf8_lossy(&data));
        }
    }

    fn dcs_hook(
        &mut self,
        byte: u8,
        params: &[i64],
        intermediates: &[u8],
        ignored_extra_intermediates: bool,
    ) {
        self.state.sixel.take();
        self.state.get_tcap.take();
        self.state.dcs.take();
        if byte == b'q' && intermediates.is_empty() && !ignored_extra_intermediates {
            self.state.sixel.replace(SixelBuilder::new(params));
        } else if byte == b'q' && intermediates == *b"+" {
            self.state.get_tcap.replace(GetTcapBuilder::default());
        } else if !ignored_extra_intermediates && is_short_dcs(intermediates, byte) {
            self.state.dcs.replace(ShortDeviceControl {
                params: params.to_vec(),
                intermediates: intermediates.to_vec(),
                byte,
                data: vec![],
            });
        } else {
            #[cfg(feature = "tmux_cc")]
            if byte == b'p' && params == [1000] {
                // into tmux_cc mode
                self.state.borrow_mut().tmux_state =
                    Some(RefCell::new(crate::tmux_cc::Parser::new()));
            }
            (self.callback)(Action::DeviceControl(DeviceControlMode::Enter(Box::new(
                EnterDeviceControlMode {
                    byte,
                    params: params.to_vec(),
                    intermediates: intermediates.to_vec(),
                    ignored_extra_intermediates,
                },
            ))));
        }
    }

    fn dcs_put(&mut self, data: u8) {
        if let Some(dcs) = self.state.dcs.as_mut() {
            dcs.data.push(data);
        } else if let Some(sixel) = self.state.sixel.as_mut() {
            sixel.push(data);
        } else if let Some(tcap) = self.state.get_tcap.as_mut() {
            tcap.push(data);
        } else {
            #[cfg(feature = "tmux_cc")]
            if let Some(tmux_state) = &self.state.tmux_state {
                let mut tmux_parser = tmux_state.borrow_mut();
                match tmux_parser.advance_byte(data) {
                    Ok(optional_events) => {
                        if let Some(tmux_event) = optional_events {
                            (self.callback)(Action::DeviceControl(DeviceControlMode::TmuxEvents(
                                Box::new(vec![tmux_event]),
                            )));
                        }
                    }
                    Err(_) => {
                        drop(tmux_parser);
                        self.state.tmux_state = None; // drop tmux state
                    }
                }
                return;
            }
            (self.callback)(Action::DeviceControl(DeviceControlMode::Data(data)));
        }
    }

    fn dcs_unhook(&mut self) {
        if let Some(dcs) = self.state.dcs.take() {
            (self.callback)(Action::DeviceControl(
                DeviceControlMode::ShortDeviceControl(Box::new(dcs)),
            ));
        } else if let Some(mut sixel) = self.state.sixel.take() {
            sixel.finish();
            (self.callback)(Action::Sixel(Box::new(sixel.sixel)));
        } else if let Some(tcap) = self.state.get_tcap.take() {
            (self.callback)(Action::XtGetTcap(tcap.finish()));
        } else {
            (self.callback)(Action::DeviceControl(DeviceControlMode::Exit));
        }
    }

    fn osc_dispatch(&mut self, osc: &[&[u8]]) {
        let osc = OperatingSystemCommand::parse(osc);
        (self.callback)(Action::OperatingSystemCommand(Box::new(osc)));
    }

    fn csi_dispatch(&mut self, params: &[CsiParam], parameters_truncated: bool, control: u8) {
        for action in CSI::parse(params, parameters_truncated, control as char) {
            (self.callback)(Action::CSI(action));
        }
    }

    fn esc_dispatch(
        &mut self,
        _params: &[i64],
        intermediates: &[u8],
        _ignored_extra_intermediates: bool,
        control: u8,
    ) {
        // It doesn't appear to be possible for params.len() > 1 due to the way
        // that the state machine in vte functions.  As such, it also seems to
        // be impossible for ignored_extra_intermediates to be true too.
        (self.callback)(Action::Esc(Esc::parse(
            if intermediates.len() == 1 {
                Some(intermediates[0])
            } else {
                None
            },
            control,
        )));
    }
}

#[cfg(all(test, feature = "std"))]
mod test;
