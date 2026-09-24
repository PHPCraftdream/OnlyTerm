use crate::domain::DomainId;
use anyhow::anyhow;
use std::fmt::Debug;
use termwiz::tmux_cc::*;

pub(crate) trait TmuxCommand: Send + Debug {
    fn get_command(&self, domain_id: DomainId) -> String;
    fn process_result(&self, domain_id: DomainId, result: &Guarded) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PaneItem {
    session_id: TmuxSessionId,
    window_id: TmuxWindowId,
    pane_id: TmuxPaneId,
    _pane_index: u64,
    cursor_x: u64,
    cursor_y: u64,
    pane_width: u64,
    pane_height: u64,
    pane_left: u64,
    pane_top: u64,
    pane_active: bool,
    pane_mouse_any: bool,
    pane_mouse_buttton: bool,
}

#[derive(Debug)]
struct WindowItem {
    session_id: TmuxSessionId,
    window_id: TmuxWindowId,
    window_width: u64,
    window_height: u64,
    window_active: bool,
    window_name: String,
    layout: Vec<WindowLayout>,
    layout_csum: String,
    history_limit: isize,
}

fn parse_sigil_number(text: &str) -> anyhow::Result<u64> {
    let num = text
        .get(1..)
        .ok_or_else(|| anyhow!("wrong prefixed id"))?
        .parse()?;

    Ok(num)
}

/// Parse a single line emitted by `tmux list-panes -F` into a [`PaneItem`].
///
/// The field order must match the format string built in
/// [`ListAllPanes::get_command`]:
/// `session_id window_id pane_id pane_index cursor_x cursor_y pane_width
/// pane_height pane_left pane_top pane_active mouse_any_flag mouse_button_flag`.
fn parse_pane_line(line: &str) -> anyhow::Result<PaneItem> {
    let mut fields = line.split(' ');
    // These ids all have various sigils such as `$`, `%`, `@`,
    // so skip those prior to parsing them
    let session_id =
        parse_sigil_number(fields.next().ok_or_else(|| anyhow!("missing session_id"))?)?;
    let window_id = parse_sigil_number(fields.next().ok_or_else(|| anyhow!("missing window_id"))?)?;
    let pane_id = parse_sigil_number(fields.next().ok_or_else(|| anyhow!("missing pane_id"))?)?;
    let _pane_index = fields
        .next()
        .ok_or_else(|| anyhow!("missing pane_index"))?
        .parse()?;
    let cursor_x = fields
        .next()
        .ok_or_else(|| anyhow!("missing cursor_x"))?
        .parse()?;
    let cursor_y = fields
        .next()
        .ok_or_else(|| anyhow!("missing cursor_y"))?
        .parse()?;
    let pane_width = fields
        .next()
        .ok_or_else(|| anyhow!("missing pane_width"))?
        .parse()?;
    let pane_height = fields
        .next()
        .ok_or_else(|| anyhow!("missing pane_height"))?
        .parse()?;
    let pane_left = fields
        .next()
        .ok_or_else(|| anyhow!("missing pane_left"))?
        .parse()?;
    let pane_top = fields
        .next()
        .ok_or_else(|| anyhow!("missing pane_top"))?
        .parse()?;
    let pane_active = fields
        .next()
        .ok_or_else(|| anyhow!("missing pane_active"))?
        .parse::<usize>()?;
    let pane_active = pane_active == 1;

    let pane_mouse_any = fields
        .next()
        .ok_or_else(|| anyhow!("missing pane_any_flag"))?
        .parse::<usize>()?;
    let pane_mouse_any = pane_mouse_any == 1;

    let pane_mouse_buttton = fields
        .next()
        .ok_or_else(|| anyhow!("missing pane_button_flag"))?
        .parse::<usize>()?;
    let pane_mouse_buttton = pane_mouse_buttton == 1;

    Ok(PaneItem {
        session_id,
        window_id,
        pane_id,
        _pane_index,
        cursor_x,
        cursor_y,
        pane_width,
        pane_height,
        pane_left,
        pane_top,
        pane_active,
        pane_mouse_any,
        pane_mouse_buttton,
    })
}

mod panes;
mod queries;
mod sync;
mod windows;

pub(crate) use panes::{Resize, SendKeys, SplitPane};
pub(crate) use queries::{ListAllPanes, ListAllWindows, ListCommands};
pub(crate) use windows::NewWindow;

#[cfg(test)]
mod tests;
