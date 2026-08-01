use crate::scripting::guiwin::GuiWin;
use chrono::prelude::*;
use log::Level;
use mux::termwiztermtab::TermWizTerminal;
use std::sync::Mutex;
use std::time::Duration;
use termwiz::cell::{AttributeChange, CellAttributes, Intensity};
use termwiz::color::AnsiColor;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers};
use termwiz::surface::Change;
use termwiz::terminal::Terminal;

lazy_static::lazy_static! {
    static ref LATEST_LOG_ENTRY: Mutex<Option<DateTime<Local>>> = Mutex::new(None);
}

/// How often the overlay polls for new log entries while idle. There is no
/// input to evaluate any more (see the module doc comment below), so this
/// just needs to be short enough that a fresh log entry shows up promptly
/// and long enough not to busy-loop.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// This overlay used to host a rhai REPL (`MainReplHost`/`ReplEditor`,
/// backed by [`config::rhai_engine::make_rhai_engine`]) that exposed every
/// `register_rhai` binding (`wezterm.mux.*`, `GuiWin`, `TabInformation`, the
/// `lua-api-crates`, the `on`/`add_to_config_reload_watch_list` event
/// surface) for live evaluation, mirroring what a real `.wezterm.rhai`
/// config callback would see.
///
/// With the scripting layer removed entirely, there is nothing left for a
/// REPL to evaluate against, so the eval engine has been deleted along with
/// it. What remains is the overlay's other diagnostic purpose: a static
/// summary of the running wezterm (version, target triple, window/OpenGL
/// environment) plus a live tail of the application's own log ring buffer
/// (see [`print_new_log_entries`]), which has nothing to do with rhai and is
/// still useful for troubleshooting.
fn print_new_log_entries(term: &mut TermWizTerminal) -> termwiz::Result<()> {
    let entries = env_bootstrap::ringlog::get_entries();
    let mut changes = vec![];
    for entry in entries {
        if let Some(latest) = LATEST_LOG_ENTRY.lock().unwrap().as_ref() {
            if entry.then <= *latest {
                // already seen this one
                continue;
            }
        }
        LATEST_LOG_ENTRY.lock().unwrap().replace(entry.then);

        changes.push(Change::AllAttributes(CellAttributes::default()));
        changes.push(Change::Text(
            entry.then.format("%H:%M:%S%.3f ").to_string(),
        ));

        changes.push(
            AttributeChange::Foreground(match entry.level {
                Level::Error => AnsiColor::Maroon.into(),
                Level::Warn => AnsiColor::Red.into(),
                Level::Info => AnsiColor::Green.into(),
                Level::Debug => AnsiColor::Blue.into(),
                Level::Trace => AnsiColor::Fuchsia.into(),
            })
            .into(),
        );
        changes.push(Change::Text(entry.level.as_str().to_string()));
        changes.push(Change::AllAttributes(CellAttributes::default()));
        changes.push(AttributeChange::Intensity(Intensity::Bold).into());
        changes.push(Change::Text(format!(" {}", entry.target)));
        changes.push(Change::AllAttributes(CellAttributes::default()));
        changes.push(Change::Text(format!(
            " > {}\r\n",
            entry.msg.replace("\n", "\r\n")
        )));
    }
    term.render(&changes)
}

pub fn show_debug_overlay(
    mut term: TermWizTerminal,
    _gui_win: GuiWin,
    opengl_info: String,
    connection_info: String,
) -> anyhow::Result<()> {
    term.no_grab_mouse_in_raw_mode();
    term.set_raw_mode()?;

    term.render(&[Change::Title("Debug".to_string())])?;

    let version = config::wezterm_version();
    let triple = config::wezterm_target_triple();

    term.render(&[Change::Text(format!(
        "Debug Overlay\r\n\
         wezterm version: {version} {triple}\r\n\
         Window Environment: {connection_info}\r\n\
         Scripting: removed (rhai eval REPL no longer available)\r\n\
         {opengl_info}\r\n\
         Showing a live tail of the application log.\r\n\
         Press ESC or CTRL-D to exit\r\n",
    ))])?;

    run_log_tail_loop(&mut term)
}

/// Replaces the old REPL read/eval loop: just polls for new log entries and
/// renders them until the user asks to exit.
fn run_log_tail_loop(term: &mut TermWizTerminal) -> anyhow::Result<()> {
    loop {
        print_new_log_entries(term)?;
        match term.poll_input(Some(POLL_INTERVAL))? {
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                ..
            })) => return Ok(()),
            Some(InputEvent::Key(KeyEvent {
                key: KeyCode::Char('d'),
                modifiers,
                ..
            })) if modifiers.contains(Modifiers::CTRL) => return Ok(()),
            _ => {}
        }
    }
}
