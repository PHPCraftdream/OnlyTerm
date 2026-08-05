use crate::error::Context;
use crate::{Result, bail, format_err};
use parser::Rule;
use pest::Parser as _;
use pest::iterators::{Pair, Pairs};

pub type TmuxWindowId = u64;
pub type TmuxPaneId = u64;
pub type TmuxSessionId = u64;

pub mod parser {
    use pest_derive::Parser;
    #[derive(Parser)]
    #[grammar = "tmux_cc/tmux.pest"]
    pub struct TmuxParser;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guarded {
    pub error: bool,
    pub timestamp: i64,
    pub number: u64,
    pub flags: i64,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    // Tmux generic events
    Begin {
        timestamp: i64,
        number: u64,
        flags: i64,
    },
    End {
        timestamp: i64,
        number: u64,
        flags: i64,
    },
    Error {
        timestamp: i64,
        number: u64,
        flags: i64,
    },
    Guarded(Guarded),

    // Tmux specific events
    ClientDetached {
        client_name: String,
    },
    ClientSessionChanged {
        client_name: String,
        session: TmuxSessionId,
        session_name: String,
    },
    ConfigError {
        error: String,
    },
    Continue {
        pane: TmuxPaneId,
    },
    ExtendedOutput {
        pane: TmuxPaneId,
        text: Vec<u8>,
    },
    Exit {
        reason: Option<String>,
    },
    LayoutChange {
        window: TmuxWindowId,
        layout: String,
        visible_layout: Option<String>,
        raw_flags: Option<String>,
    },
    Message {
        message: String,
    },
    Output {
        pane: TmuxPaneId,
        text: Vec<u8>,
    },
    PaneModeChanged {
        pane: TmuxPaneId,
    },
    PasteBufferChanged {
        buffer: String,
    },
    PasteBufferDeleted {
        buffer: String,
    },
    Pause {
        pane: TmuxPaneId,
    },
    SessionChanged {
        session: TmuxSessionId,
        name: String,
    },
    SessionRenamed {
        name: String,
    },
    SessionsChanged,
    SessionWindowChanged {
        session: TmuxSessionId,
        window: TmuxWindowId,
    },
    SubscriptionChanged,
    UnlinkedWindowAdd {
        window: TmuxWindowId,
    },
    UnlinkedWindowClose {
        window: TmuxWindowId,
    },
    UnlinkedWindowRenamed {
        window: TmuxWindowId,
    },
    WindowAdd {
        window: TmuxWindowId,
    },
    WindowClose {
        window: TmuxWindowId,
    },
    WindowPaneChanged {
        window: TmuxWindowId,
        pane: TmuxPaneId,
    },
    WindowRenamed {
        window: TmuxWindowId,
        name: String,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct PaneLayout {
    pub pane_id: TmuxPaneId,
    pub pane_width: u64,
    pub pane_height: u64,
    pub pane_left: u64,
    pub pane_top: u64,
}

#[derive(Debug)]
pub enum WindowLayout {
    SplitVertical(Vec<PaneLayout>),
    SplitHorizontal(Vec<PaneLayout>),
    SinglePane(PaneLayout),
}

fn parse_pane_id(pair: Pair<Rule>) -> Result<TmuxPaneId> {
    match pair.as_rule() {
        Rule::pane_id => {
            let mut pairs = pair.into_inner();
            pairs
                .next()
                .ok_or_else(|| format_err!("missing pane id"))?
                .as_str()
                .parse()
                .context("pane_id is somehow not digits")
        }
        _ => bail!("parse_pane_id can only parse Rule::pane_id, got {:?}", pair),
    }
}

fn parse_window_id(pair: Pair<Rule>) -> Result<TmuxWindowId> {
    match pair.as_rule() {
        Rule::window_id => {
            let mut pairs = pair.into_inner();
            pairs
                .next()
                .ok_or_else(|| format_err!("missing window id"))?
                .as_str()
                .parse()
                .context("window_id is somehow not digits")
        }
        _ => bail!(
            "parse_window_id can only parse Rule::window_id, got {:?}",
            pair
        ),
    }
}

fn parse_session_id(pair: Pair<Rule>) -> Result<TmuxSessionId> {
    match pair.as_rule() {
        Rule::session_id => {
            let mut pairs = pair.into_inner();
            pairs
                .next()
                .ok_or_else(|| format_err!("missing session id"))?
                .as_str()
                .parse()
                .context("session_id is somehow not digits")
        }
        _ => bail!(
            "parse_session_id can only parse Rule::session_id, got {:?}",
            pair
        ),
    }
}

/// Parses a %begin, %end, %error guard line tuple
fn parse_guard(mut pairs: Pairs<Rule>) -> Result<(i64, u64, i64)> {
    let timestamp = pairs
        .next()
        .ok_or_else(|| format_err!("missing timestamp"))?
        .as_str()
        .parse::<i64>()?;
    let number = pairs
        .next()
        .ok_or_else(|| format_err!("missing number"))?
        .as_str()
        .parse::<u64>()?;
    let flags = pairs
        .next()
        .ok_or_else(|| format_err!("missing flags"))?
        .as_str()
        .parse::<i64>()?;
    Ok((timestamp, number, flags))
}

fn parse_line(line: &[u8]) -> Result<Event> {
    let binding = String::from_utf8_lossy(line);
    let parsed_line = binding.as_ref();
    let mut pairs = parser::TmuxParser::parse(Rule::line_entire, parsed_line)?;
    let pair = pairs.next().ok_or_else(|| format_err!("no pairs!?"))?;
    match pair.as_rule() {
        // Tmux generic rules
        Rule::begin => {
            let (timestamp, number, flags) = parse_guard(pair.into_inner())?;
            Ok(Event::Begin {
                timestamp,
                number,
                flags,
            })
        }
        Rule::end => {
            let (timestamp, number, flags) = parse_guard(pair.into_inner())?;
            Ok(Event::End {
                timestamp,
                number,
                flags,
            })
        }
        Rule::error => {
            let (timestamp, number, flags) = parse_guard(pair.into_inner())?;
            Ok(Event::Error {
                timestamp,
                number,
                flags,
            })
        }

        // Tmux specific rules
        Rule::client_detached => {
            let mut pairs = pair.into_inner();
            let client_name = unvis(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing name"))?
                    .as_str(),
            )?;
            Ok(Event::ClientDetached { client_name })
        }
        Rule::client_session_changed => {
            let mut pairs = pair.into_inner();
            let client_name = unvis(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing name"))?
                    .as_str(),
            )?;
            let session = parse_session_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing session id"))?,
            )?;
            let session_name = unvis(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing session name"))?
                    .as_str(),
            )?;
            Ok(Event::ClientSessionChanged {
                client_name,
                session,
                session_name,
            })
        }
        Rule::config_error => {
            let mut pairs = pair.into_inner();
            let error = unvis(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing name"))?
                    .as_str(),
            )?;
            Ok(Event::ConfigError { error })
        }
        Rule::r#continue => {
            let mut pairs = pair.into_inner();
            let pane = parse_pane_id(pairs.next().ok_or_else(|| format_err!("missing pane id"))?)?;
            Ok(Event::Continue { pane })
        }
        Rule::extended_output => {
            let mut pairs = pair.into_inner();
            let pane = parse_pane_id(pairs.next().ok_or_else(|| format_err!("missing pane id"))?)?;
            let pair = pairs.next().ok_or_else(|| format_err!("missing text"))?;

            let (_, pos) = pair.line_col();
            let text = unvis_bytes(&line[pos - 1..])?;
            Ok(Event::ExtendedOutput { pane, text })
        }
        Rule::exit => {
            let mut pairs = pair.into_inner();
            let reason = pairs.next().map(|pair| pair.as_str().to_owned());
            Ok(Event::Exit { reason })
        }
        Rule::layout_change => {
            let mut pairs = pair.into_inner();
            let window = parse_window_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing window id"))?,
            )?;
            let layout = unvis(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing layout"))?
                    .as_str(),
            )?;
            let visible_layout = pairs.next().map(|pair| pair.as_str().to_owned());
            let raw_flags = pairs.next().map(|r| r.as_str().to_owned());
            Ok(Event::LayoutChange {
                window,
                layout,
                visible_layout,
                raw_flags,
            })
        }
        Rule::message => {
            let mut pairs = pair.into_inner();
            let message = unvis(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing text"))?
                    .as_str(),
            )?;
            Ok(Event::Message { message })
        }
        Rule::output => {
            let mut pairs = pair.into_inner();
            let pane = parse_pane_id(pairs.next().ok_or_else(|| format_err!("missing pane id"))?)?;
            let pair = pairs.next().ok_or_else(|| format_err!("missing text"))?;

            let (_, pos) = pair.line_col();
            let text = unvis_bytes(&line[pos - 1..])?;
            Ok(Event::Output { pane, text })
        }
        Rule::pane_mode_changed => {
            let mut pairs = pair.into_inner();
            let pane = parse_pane_id(pairs.next().ok_or_else(|| format_err!("missing pane id"))?)?;
            Ok(Event::PaneModeChanged { pane })
        }
        Rule::paste_buffer_changed => {
            let mut pairs = pair.into_inner();
            let buffer = unvis(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing text"))?
                    .as_str(),
            )?;
            Ok(Event::PasteBufferChanged { buffer })
        }
        Rule::paste_buffer_deleted => {
            let mut pairs = pair.into_inner();
            let buffer = unvis(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing text"))?
                    .as_str(),
            )?;
            Ok(Event::PasteBufferDeleted { buffer })
        }
        Rule::pause => {
            let mut pairs = pair.into_inner();
            let pane = parse_pane_id(pairs.next().ok_or_else(|| format_err!("missing pane id"))?)?;
            Ok(Event::Pause { pane })
        }
        Rule::session_changed => {
            let mut pairs = pair.into_inner();
            let session = parse_session_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing session id"))?,
            )?;
            let name = unvis(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing name"))?
                    .as_str(),
            )?;
            Ok(Event::SessionChanged { session, name })
        }
        Rule::session_renamed => {
            let mut pairs = pair.into_inner();
            let name = unvis(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing name"))?
                    .as_str(),
            )?;
            Ok(Event::SessionRenamed { name })
        }
        Rule::session_window_changed => {
            let mut pairs = pair.into_inner();
            let session = parse_session_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing session id"))?,
            )?;
            let window = parse_window_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing window id"))?,
            )?;
            Ok(Event::SessionWindowChanged { session, window })
        }
        Rule::sessions_changed => Ok(Event::SessionsChanged),
        Rule::subscription_changed => Ok(Event::SubscriptionChanged),
        Rule::unlinked_window_add => {
            let mut pairs = pair.into_inner();
            let window = parse_window_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing window id"))?,
            )?;
            Ok(Event::UnlinkedWindowAdd { window })
        }
        Rule::unlinked_window_close => {
            let mut pairs = pair.into_inner();
            let window = parse_window_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing window id"))?,
            )?;
            Ok(Event::UnlinkedWindowClose { window })
        }
        Rule::unlinked_window_renamed => {
            let mut pairs = pair.into_inner();
            let window = parse_window_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing window id"))?,
            )?;
            Ok(Event::UnlinkedWindowRenamed { window })
        }
        Rule::window_add => {
            let mut pairs = pair.into_inner();
            let window = parse_window_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing window id"))?,
            )?;
            Ok(Event::WindowAdd { window })
        }
        Rule::window_close => {
            let mut pairs = pair.into_inner();
            let window = parse_window_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing window id"))?,
            )?;
            Ok(Event::WindowClose { window })
        }
        Rule::window_pane_changed => {
            let mut pairs = pair.into_inner();
            let window = parse_window_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing window id"))?,
            )?;
            let pane = parse_pane_id(pairs.next().ok_or_else(|| format_err!("missing pane id"))?)?;
            Ok(Event::WindowPaneChanged { window, pane })
        }
        Rule::window_renamed => {
            let mut pairs = pair.into_inner();
            let window = parse_window_id(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing window id"))?,
            )?;
            let name = unvis(
                pairs
                    .next()
                    .ok_or_else(|| format_err!("missing name"))?
                    .as_str(),
            )?;
            Ok(Event::WindowRenamed { window, name })
        }
        Rule::EOI
        | Rule::any_text
        | Rule::client_name
        | Rule::layout_pane
        | Rule::layout_split_horizontal
        | Rule::layout_split_pane
        | Rule::layout_split_vertical
        | Rule::layout_window
        | Rule::line
        | Rule::line_entire
        | Rule::number
        | Rule::pane_id
        | Rule::session_id
        | Rule::window_id
        | Rule::window_layout
        | Rule::word => bail!("Should not reach here"),
    }
}

/// Decode OpenBSD `vis` encoded strings
/// See: https://github.com/tmux/tmux/blob/486ce9b09855ae30a2bf5e576cb6f7ad37792699/compat/unvis.c
fn unvis_bytes(s: &[u8]) -> Result<Vec<u8>> {
    enum State {
        Ground,
        Start,
        Meta,
        Meta1,
        Ctrl(u8),
        Octal2(u8),
        Octal3(u8),
    }

    let mut state = State::Ground;
    let mut result: Vec<u8> = vec![];
    let bytes = s.iter();

    fn is_octal(b: u8) -> bool {
        (b'0'..=b'7').contains(&b)
    }

    fn unvis_byte(b: u8, state: &mut State, result: &mut Vec<u8>) -> Result<bool> {
        match state {
            State::Ground => {
                if b == b'\\' {
                    *state = State::Start;
                } else {
                    result.push(b);
                }
            }

            State::Start => {
                match b {
                    b'\\' => {
                        result.push(b'\\');
                        *state = State::Ground;
                    }
                    b'0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6' | b'7' => {
                        let value = b - b'0';
                        *state = State::Octal2(value);
                    }
                    b'M' => {
                        *state = State::Meta;
                    }
                    b'^' => {
                        *state = State::Ctrl(0);
                    }
                    b'n' => {
                        result.push(b'\n');
                        *state = State::Ground;
                    }
                    b'r' => {
                        result.push(b'\r');
                        *state = State::Ground;
                    }
                    b'b' => {
                        result.push(b'\x08');
                        *state = State::Ground;
                    }
                    b'a' => {
                        result.push(b'\x07');
                        *state = State::Ground;
                    }
                    b'v' => {
                        result.push(b'\x0b');
                        *state = State::Ground;
                    }
                    b't' => {
                        result.push(b'\t');
                        *state = State::Ground;
                    }
                    b'f' => {
                        result.push(b'\x0c');
                        *state = State::Ground;
                    }
                    b's' => {
                        result.push(b' ');
                        *state = State::Ground;
                    }
                    b'E' => {
                        result.push(b'\x1b');
                        *state = State::Ground;
                    }
                    b'\n' => {
                        // Hidden newline
                        // result.push(b'\n');
                        *state = State::Ground;
                    }
                    b'$' => {
                        // Hidden marker
                        *state = State::Ground;
                    }
                    _ => {
                        // Invalid syntax
                        bail!("Invalid \\ escape: {}", b);
                    }
                }
            }

            State::Meta => {
                if b == b'-' {
                    *state = State::Meta1;
                } else if b == b'^' {
                    *state = State::Ctrl(0o200);
                } else {
                    bail!("invalid \\M escape: {}", b);
                }
            }

            State::Meta1 => {
                result.push(b | 0o200);
                *state = State::Ground;
            }

            State::Ctrl(c) => {
                if b == b'?' {
                    result.push(*c | 0o177);
                } else {
                    result.push((b & 0o37) | *c);
                }
                *state = State::Ground;
            }

            State::Octal2(prior) => {
                if is_octal(b) {
                    // It's the second in a 2 or 3 byte octal sequence
                    let value = (*prior << 3) + (b - b'0');
                    *state = State::Octal3(value);
                } else {
                    // Prior character was a single octal value
                    result.push(*prior);
                    *state = State::Ground;
                    // re-process the current byte
                    return Ok(true);
                }
            }

            State::Octal3(prior) => {
                if is_octal(b) {
                    // It's the third in a 3 byte octal sequence
                    let value = (*prior << 3) + (b - b'0');
                    result.push(value);
                    *state = State::Ground;
                } else {
                    // Prior was a 2-byte octal sequence
                    result.push(*prior);
                    *state = State::Ground;
                    // re-process the current byte
                    return Ok(true);
                }
            }
        }
        // Don't process this byte again
        Ok(false)
    }

    for &b in bytes {
        let again = unvis_byte(b, &mut state, &mut result)?;
        if again {
            unvis_byte(b, &mut state, &mut result)?;
        }
    }

    Ok(result)
}

pub fn unvis(s: &str) -> Result<String> {
    let bytes = s.as_bytes();

    let result = unvis_bytes(bytes)?;

    String::from_utf8(result)
        .map_err(|err| format_err!("Unescaped string is not valid UTF8: {}", err))
}

fn parse_layout_pane(pair: Pair<Rule>) -> Result<PaneLayout> {
    let mut pairs = pair.into_inner();

    let pane_width = pairs
        .next()
        .ok_or_else(|| format_err!("wrong pane layout format"))?
        .as_str()
        .parse()?;
    let pane_height = pairs
        .next()
        .ok_or_else(|| format_err!("wrong pane layout format"))?
        .as_str()
        .parse()?;
    let pane_left = pairs
        .next()
        .ok_or_else(|| format_err!("wrong pane layout format"))?
        .as_str()
        .parse()?;
    let pane_top = pairs
        .next()
        .ok_or_else(|| format_err!("wrong pane layout format"))?
        .as_str()
        .parse()?;

    let pane_id = match pairs.next() {
        Some(x) => x.as_str().parse()?,
        None => 0,
    };

    Ok(PaneLayout {
        pane_id,
        pane_width,
        pane_height,
        pane_left,
        pane_top,
    })
}

fn parse_layout_inner(
    pairs: Pairs<Rule>,
    result: &mut Vec<WindowLayout>,
) -> Result<Vec<PaneLayout>> {
    let mut stack = Vec::new();

    for pair in pairs {
        let rule = pair.as_rule();
        match rule {
            Rule::layout_split_horizontal | Rule::layout_split_vertical => {
                let mut pairs_inner = pair.into_inner();
                let pair = pairs_inner
                    .next()
                    .ok_or_else(|| format_err!("no pairs!?"))?;
                let mut pane = parse_layout_pane(pair)?;

                if result.is_empty() {
                    // Fake one, to flag it is not a TmuxLayout::SinglePane will pop
                    result.push(WindowLayout::SplitHorizontal(vec![]));
                }

                let mut layout_inner = parse_layout_inner(pairs_inner, result)?;

                let last_item = layout_inner
                    .pop()
                    .ok_or_else(|| format_err!("wrong layout format"))?;

                pane.pane_id = last_item.pane_id;

                layout_inner.insert(0, pane);

                if let Rule::layout_split_horizontal = rule {
                    result.insert(0, WindowLayout::SplitHorizontal(layout_inner));
                } else {
                    result.insert(0, WindowLayout::SplitVertical(layout_inner));
                }

                stack.push(pane);
            }
            Rule::layout_pane => {
                let pane = parse_layout_pane(pair)?;

                // SinglePane
                if result.is_empty() {
                    result.insert(0, WindowLayout::SinglePane(pane));
                    return Ok(stack);
                }

                stack.push(pane);
            }
            Rule::EOI
            | Rule::any_text
            | Rule::begin
            | Rule::client_detached
            | Rule::client_name
            | Rule::client_session_changed
            | Rule::config_error
            | Rule::r#continue
            | Rule::end
            | Rule::error
            | Rule::exit
            | Rule::extended_output
            | Rule::layout_change
            | Rule::layout_split_pane
            | Rule::layout_window
            | Rule::line
            | Rule::line_entire
            | Rule::message
            | Rule::number
            | Rule::output
            | Rule::pane_id
            | Rule::pane_mode_changed
            | Rule::paste_buffer_changed
            | Rule::paste_buffer_deleted
            | Rule::pause
            | Rule::session_changed
            | Rule::session_id
            | Rule::session_renamed
            | Rule::session_window_changed
            | Rule::sessions_changed
            | Rule::subscription_changed
            | Rule::unlinked_window_add
            | Rule::unlinked_window_close
            | Rule::unlinked_window_renamed
            | Rule::window_add
            | Rule::window_close
            | Rule::window_id
            | Rule::window_layout
            | Rule::window_pane_changed
            | Rule::window_renamed
            | Rule::word => bail!("Should not reach here"),
        }
    }

    Ok(stack)
}

pub fn parse_layout(layout: &str) -> Result<Vec<WindowLayout>> {
    let mut result = Vec::new();
    let pairs = parser::TmuxParser::parse(Rule::layout_window, layout)?;

    let _ = parse_layout_inner(pairs, &mut result)?;
    if result.len() > 1 {
        let _ = result.pop();
    }

    Ok(result)
}

pub struct Parser {
    buffer: Vec<u8>,
    begun: Option<Guarded>,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    pub fn new() -> Self {
        Self {
            buffer: vec![],
            begun: None,
        }
    }

    pub fn advance_byte(&mut self, c: u8) -> Result<Option<Event>> {
        if c == b'\n' {
            self.process_line()
        } else {
            self.buffer.push(c);
            Ok(None)
        }
    }

    pub fn advance_string(&mut self, s: &str) -> Result<Vec<Event>> {
        self.advance_bytes(s.as_bytes())
    }

    pub fn advance_bytes(&mut self, bytes: &[u8]) -> Result<Vec<Event>> {
        let mut events = vec![];
        for (i, &b) in bytes.iter().enumerate() {
            match self.advance_byte(b) {
                Ok(option_event) => {
                    if let Some(e) = option_event {
                        events.push(e);
                    }
                }
                Err(err) => {
                    // concat remained bytes after digested bytes
                    bail!("{}{}", err, String::from_utf8_lossy(&bytes[i..]));
                }
            }
        }
        Ok(events)
    }

    fn process_guarded_line(&mut self) -> Result<Option<Event>> {
        let line = std::str::from_utf8(&self.buffer)?;
        let result = match parse_line(&self.buffer) {
            Ok(Event::End {
                timestamp,
                number,
                flags,
            }) => {
                if let Some(begun) = self.begun.take() {
                    if begun.timestamp == timestamp
                        && begun.number == number
                        && begun.flags == flags
                    {
                        Some(Event::Guarded(begun))
                    } else {
                        log::error!("mismatched %end; expected {:?} but got {}", begun, line);
                        None
                    }
                } else {
                    log::error!("unexpected %end with no %begin ({})", line);
                    None
                }
            }
            Ok(Event::Error {
                timestamp,
                number,
                flags,
            }) => {
                if let Some(mut begun) = self.begun.take() {
                    if begun.timestamp == timestamp
                        && begun.number == number
                        && begun.flags == flags
                    {
                        begun.error = true;
                        Some(Event::Guarded(begun))
                    } else {
                        log::error!("mismatched %error; expected {:?} but got {}", begun, line);
                        None
                    }
                } else {
                    log::error!("unexpected %error with no %begin ({})", line);
                    None
                }
            }
            _ => {
                let begun = self
                    .begun
                    .as_mut()
                    .ok_or_else(|| format_err!("missing begun"))?;
                begun.output.push_str(line);
                begun.output.push('\n');
                None
            }
        };
        self.buffer.clear();
        Ok(result)
    }

    fn process_line(&mut self) -> Result<Option<Event>> {
        if self.buffer.last() == Some(&b'\r') {
            self.buffer.pop();
        }
        if self.begun.is_some() {
            return self.process_guarded_line();
        }

        let result = match parse_line(&self.buffer) {
            Ok(Event::Begin {
                timestamp,
                number,
                flags,
            }) => {
                if self.begun.is_some() {
                    log::error!(
                        "expected %end or %error before %begin ({})",
                        String::from_utf8_lossy(&self.buffer)
                    );
                }
                self.begun.replace(Guarded {
                    timestamp,
                    number,
                    flags,
                    error: false,
                    output: String::new(),
                });
                None
            }
            Ok(event) => Some(event),
            Err(err) => {
                log::error!("Unrecognized tmux cc line: {}", err);
                bail!("{}", String::from_utf8_lossy(&self.buffer));
            }
        };

        self.buffer.clear();
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
