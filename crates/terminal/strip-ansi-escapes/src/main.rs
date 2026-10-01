use clap::Parser;
use std::io::{Read, Result};
use termwiz::escape::parser::Parser as TWParser;
use termwiz::escape::{Action, ControlCode};

#[derive(Debug, Parser)]
/// This is a little utility that strips escape sequences from
/// stdin and prints the result on stdout.
/// It preserves only printable characters and CR, LF and HT.
///
/// This utility is part of OnlyTerm.
///
/// https://github.com/wezterm/wezterm
struct Opt {}

fn main() -> Result<()> {
    let _ = Opt::parse();
    let mut buf = [0u8; 4096];

    let mut parser = TWParser::new();

    loop {
        let len = std::io::stdin().read(&mut buf)?;
        if len == 0 {
            return Ok(());
        }

        print!("{}", strip(&mut parser, &buf[0..len]));
    }
}

/// Returns the printable text and CR/LF/HT found in `bytes`, with every
/// escape sequence removed. The parser reports runs of printable text as
/// `PrintString` (single characters as `Print`), so both must be kept.
fn strip(parser: &mut TWParser, bytes: &[u8]) -> String {
    let mut out = String::new();
    parser.parse(bytes, |action| match action {
        Action::Print(c) => out.push(c),
        Action::PrintString(s) => out.push_str(&s),
        Action::Control(c) => match c {
            ControlCode::HorizontalTab | ControlCode::LineFeed | ControlCode::CarriageReturn => {
                out.push(c as u8 as char)
            }
            _ => {}
        },
        _ => {}
    });
    out
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn keeps_text_runs_and_strips_escapes() {
        let mut parser = TWParser::new();
        let out = strip(&mut parser, b"\x1b[31mhello\x1b[0m world\r\n\x07x\ty");
        assert_eq!(out, "hello world\r\nx\ty");
    }

    #[test]
    fn keeps_single_characters_and_text_split_across_reads() {
        let mut parser = TWParser::new();
        let mut out = String::new();
        for chunk in [&b"a"[..], b"bc\x1b[1", b"mdef", b"g"] {
            out.push_str(&strip(&mut parser, chunk));
        }
        assert_eq!(out, "abcdefg");
    }
}
