use crate::scripting::guiwin::GuiWin;
use chrono::prelude::*;
use log::Level;
use mux::termwiztermtab::TermWizTerminal;
use rhai::{AST, Dynamic, Engine, ParseError, ParseErrorType, Scope};
use std::cell::RefCell;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use termwiz::cell::{AttributeChange, CellAttributes, Intensity};
use termwiz::color::AnsiColor;
use termwiz::input::{InputEvent, KeyCode, KeyEvent};
use termwiz::lineedit::*;
use termwiz::surface::Change;
use termwiz::terminal::Terminal;

lazy_static::lazy_static! {
    static ref LATEST_LOG_ENTRY: Mutex<Option<DateTime<Local>>> = Mutex::new(None);
}

/// The persistent, main-thread-only rhai state the REPL evaluates against.
///
/// This lives in a [`thread_local`] (see [`MAIN_HOST`]) rather than being
/// shuttled across threads the way the old `LuaReplHost` was, because
/// `rhai::Engine`/`Scope`/`AST` are not `Send` in this workspace (no `sync`
/// feature; see the module doc comment on `config/src/rhai_engine.rs`).
/// `mlua::Lua`, by contrast, was built with the `send` feature, which is the
/// only reason the former REPL could round-trip its host through
/// `spawn_into_main_thread`. The rhai host therefore stays on the main thread;
/// the worker-thread line editor (see [`ReplEditor`]) only ever sends it
/// `String` buffers and receives `Send` results back.
///
/// The engine is built by [`config::rhai_engine::make_rhai_engine`], so every
/// `register_rhai` binding (`wezterm.mux.*`, `GuiWin`, `TabInformation`, the
/// 13 `lua-api-crates`, the `on`/`add_to_config_reload_watch_list` event
/// surface) is available in the REPL exactly as it is to a real
/// `.wezterm.rhai` config callback -- the debug overlay no longer spins up its
/// own isolated interpreter the way the old `make_lua_context(Path::new(""))`
/// arm did.
///
/// `main_ast` mirrors the upstream `rhai-repl` example: each completed fragment
/// is merged into it and re-evaluated, then [`AST::clear_statements`] leaves
/// only the accumulated `fn` definitions behind. That is how top-level `let`
/// bindings (which persist in `scope`) *and* `fn` declarations (which live in
/// the AST, not the scope) both survive across REPL lines.
struct MainReplHost {
    engine: Engine,
    scope: Scope<'static>,
    main_ast: AST,
}

impl MainReplHost {
    fn new(window: GuiWin) -> anyhow::Result<Self> {
        let rhai_engine = config::rhai_engine::make_rhai_engine(std::path::Path::new(""))?;
        let mut scope = Scope::new();
        // `window` is the rhai analogue of the old
        // `lua.globals().set("window", gui_win)`: the live `GuiWin` for the
        // window that opened the overlay, available to every REPL expression
        // just like it is inside a `wezterm.on(...)` handler.
        scope.push("window", window);
        Ok(Self {
            engine: rhai_engine.engine,
            scope,
            main_ast: AST::empty(),
        })
    }
}

thread_local! {
    /// Holds the [`MainReplHost`] for the lifetime of one debug-overlay session.
    /// Populated on the main thread at overlay start, cleared at overlay end.
    static MAIN_HOST: RefCell<Option<MainReplHost>> = const { RefCell::new(None) };
}

/// Worker-thread side of the REPL: everything the `termwiz` line editor needs
/// (history, the accumulated multi-line buffer, and a throwaway engine for live
/// preview compilation). None of this is `Send`-sensitive in a way that matters
/// -- it all stays on the worker thread that `start_overlay` spun up.
struct ReplEditor {
    history: BasicHistory,
    /// Lines accumulated while the input still parses as an incomplete
    /// fragment (open `{`/`(`/`[`, trailing operator, ...). Once the buffer
    /// compiles it is shipped to the main-thread host for evaluation and then
    /// cleared.
    pending: String,
    /// A bare `Engine` used only to compile the in-progress line for the inline
    /// preview shown by [`LineEditorHost::render_preview`]. Compilation is
    /// grammar-only and does not resolve registered functions (rhai is
    /// dynamically typed), so this does not need the full `make_rhai_engine`
    /// binding set -- it just needs to detect "incomplete" vs "syntax error".
    preview_engine: Engine,
}

fn history_file_name() -> PathBuf {
    config::DATA_DIR.join("repl-history")
}

impl ReplEditor {
    fn new() -> Self {
        let mut history = BasicHistory::default();
        if let Ok(data) = std::fs::read_to_string(history_file_name()) {
            for line in data.lines() {
                history.add(line);
            }
        }
        // Match `make_rhai_engine`'s parse-time limits so the preview's
        // incomplete/syntax-error verdict agrees with the real evaluation.
        let mut preview_engine = Engine::new();
        preview_engine.set_max_expr_depths(64, 64);
        preview_engine.set_max_call_levels(64);
        Self {
            history,
            pending: String::new(),
            preview_engine,
        }
    }

    fn add_history(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }

        if let Some(last) = self.history.last() {
            if self.history.get(last).as_deref() == Some(line) {
                // Don't add duplicate lines
                return;
            }
        }
        self.history.add(line);
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(history_file_name())
        {
            writeln!(file, "{}", line).ok();
        }
    }
}

/// The primary incomplete-input signal, checked *before* compiling: returns
/// true if `buffer` has an unclosed `{`, `(`, or `[`, or an unterminated
/// string/char literal.
///
/// This is parser-independent, which matters because rhai's parser does not
/// always surface an unclosed group as `UnexpectedEOF` or `MissingToken("}")`.
/// In particular, a block-body statement that is missing its terminating `;`
/// *inside* an open `{` (e.g. `if true {\n    123`) is reported as
/// `MissingToken(";", "to terminate this statement")` -- which
/// [`is_incomplete_parse`] would otherwise classify as a real syntax error.
/// Counting brackets directly catches that case: the `;` complaint is just a
/// symptom of the not-yet-closed block. String/char literals and `//` line
/// comments are skipped so a `}` or `{` appearing inside one does not skew the
/// count, and an unescaped newline or EOF inside a literal counts as an
/// unterminated literal (more input could close it).
fn has_open_group(buffer: &str) -> bool {
    let mut depth_curly: i32 = 0;
    let mut depth_paren: i32 = 0;
    let mut depth_bracket: i32 = 0;
    let chars: Vec<char> = buffer.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            // `//` line comment: ignore everything (including brackets) to the
            // end of the line.
            '/' if i + 1 < chars.len() && chars[i + 1] == '/' => {
                i += 2;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '"' | '\'' => {
                let quote = c;
                i += 1;
                let mut closed = false;
                while i < chars.len() {
                    if chars[i] == '\\' {
                        // Skip the escaped character (handles `"`, `\`, etc.).
                        i += 2;
                        continue;
                    }
                    if chars[i] == quote {
                        closed = true;
                        i += 1;
                        break;
                    }
                    // An unescaped newline means the literal is unterminated
                    // (rhai string/char literals do not span raw newlines).
                    if chars[i] == '\n' {
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    return true;
                }
            }
            '{' => depth_curly += 1,
            '}' => depth_curly -= 1,
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            _ => {}
        }
        i += 1;
    }
    depth_curly > 0 || depth_paren > 0 || depth_bracket > 0
}

/// Secondary incomplete-input signal, consulted when [`has_open_group`] reports
/// no open group but `Engine::compile` still fails. rhai signals "the script ends
/// prematurely" via [`ParseErrorType::UnexpectedEOF`] ("Script is incomplete"),
/// and "an open group was never closed" via [`ParseErrorType::MissingToken`]
/// whose expected token is one of `)`/`}`/`]`. Both mean the fragment would
/// parse given more input, so the REPL should keep accumulating lines rather
/// than reporting an error. This catches balanced-bracket but still-incomplete
/// fragments such as a trailing operator (`let x =`).
fn is_incomplete_parse(err: &ParseError) -> bool {
    match err.err_type() {
        ParseErrorType::UnexpectedEOF => true,
        // "Expecting ')'/'}'/']' ..." at EOF: an unclosed group that more lines
        // could legitimately close. Other `MissingToken` values (e.g. a missing
        // token in the middle of otherwise-complete input) are genuine syntax
        // errors, not continuation prompts.
        ParseErrorType::MissingToken(token, _) => matches!(token.as_str(), ")" | "}" | "]"),
        _ => false,
    }
}

/// Format a [`rhai::Dynamic`] result for display in the overlay, the rhai
/// counterpart of `luahelper::ValuePrinter`. Returns an empty string for `()`
/// (the rhai equivalent of Lua's `nil`), which the overlay loop interprets as
/// "nothing to print" -- exactly matching the old `if text != "nil"` guard.
fn format_dynamic(value: &Dynamic) -> String {
    if value.is_unit() {
        return String::new();
    }
    // Strings are quoted (and escaped), matching ValuePrinter's Lua-string
    // rendering; the bare `Display` impl would print the raw contents without
    // delimiters, which is ambiguous for the REPL.
    if value.is_string() {
        if let Ok(s) = value.as_immutable_string_ref() {
            return format!("\"{}\"", s.escape_default());
        }
    }
    if value.is_array() {
        if let Ok(arr) = value.as_array_ref() {
            let items: Vec<String> = arr.iter().map(format_dynamic).collect();
            return format!("[{}]", items.join(", "));
        }
    }
    // `rhai::Map` is a `BTreeMap`, so iteration is already in stable key order.
    if value.is_map() {
        if let Ok(map) = value.as_map_ref() {
            let items: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("\"{}\": {}", k, format_dynamic(v)))
                .collect();
            return format!("#{{{}}}", items.join(", "));
        }
    }
    // Integers, floats, booleans, chars, ranges, timestamps and any custom
    // type fall through to `Display`, which rhai implements sensibly for the
    // built-ins (and prints the registered type name for custom values).
    format!("{}", value)
}

/// Outcome of attempting to evaluate the accumulated REPL buffer. This is a
/// `Send` value (it carries only a `String`), so it is what crosses the
/// main-thread -> worker-thread boundary inside `spawn_into_main_thread`.
enum ReplOutcome {
    /// The buffer parsed as an incomplete fragment; the REPL should keep
    /// accumulating lines (the `pending` buffer is left untouched by the
    /// caller).
    Incomplete,
    /// The buffer was complete and has been evaluated against the scope.
    /// `text` is the formatted result; an empty string means "nothing to print"
    /// (a statement that produced `()`).
    Done { text: String },
}

/// Core REPL evaluation, factored out of the host so it is unit-testable with a
/// plain `Engine::new()` (no `GuiWin`/`Mux` required) and runnable on any
/// thread.
///
/// `buffer` is the full accumulated input. On success the freshly-compiled
/// fragment is merged into `main_ast` (so `fn` definitions persist), the whole
/// `main_ast` is evaluated against `scope` (so `let` bindings persist), and
/// [`AST::clear_statements`] then leaves only the accumulated functions behind
/// for the next line -- the same cycle the upstream `rhai-repl` binary uses.
fn eval_repl_buffer(
    engine: &Engine,
    scope: &mut Scope,
    main_ast: &mut AST,
    buffer: &str,
) -> ReplOutcome {
    // Primary incomplete signal: an unclosed group/quote means the fragment
    // cannot be complete yet, regardless of the specific parse error rhai would
    // emit (it may ask for a `;` inside an open block before noticing the
    // missing `}`). Wait for more lines rather than reporting an error.
    if has_open_group(buffer) {
        return ReplOutcome::Incomplete;
    }
    let ast = match engine.compile(buffer) {
        Ok(ast) => ast,
        Err(err) if is_incomplete_parse(&err) => return ReplOutcome::Incomplete,
        Err(err) => return ReplOutcome::Done { text: err.to_string() },
    };

    *main_ast += ast;

    let text = match engine.eval_ast_with_scope::<Dynamic>(scope, main_ast) {
        Ok(value) => format_dynamic(&value),
        Err(err) => format!("{}", err),
    };

    // Drop the just-run statements, keeping only `fn` definitions so they remain
    // callable from later lines without re-running side-effecting statements.
    main_ast.clear_statements();
    ReplOutcome::Done { text }
}

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

impl LineEditorHost for ReplEditor {
    fn history(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn resolve_action(
        &mut self,
        event: &InputEvent,
        editor: &mut LineEditor<'_>,
    ) -> Option<Action> {
        let (line, _cursor) = editor.get_line_and_cursor();
        if line.is_empty()
            && matches!(
                event,
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Escape,
                    ..
                })
            )
        {
            Some(Action::Cancel)
        } else {
            None
        }
    }

    fn render_preview(&self, line: &str) -> Vec<OutputElement> {
        // Preview the accumulated buffer plus the line currently being typed,
        // so the inline feedback stays correct while spanning multiple lines.
        let mut buffer = self.pending.clone();
        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(line);
        if buffer.is_empty() {
            return vec![];
        }

        let mut preview = vec![];
        // "..." mirrors the old Lua formatter's incomplete-input marker: a
        // low-noise hint that the fragment is valid-but-unfinished, rather
        // than a parse error the user must fix. Check `has_open_group` first so
        // an open block shows "..." instead of a misleading missing-`;` error
        // (same rationale as in `eval_repl_buffer`).
        if has_open_group(&buffer) {
            preview.push(OutputElement::Text("...".to_string()));
        } else if let Err(err) = self.preview_engine.compile(&buffer) {
            let text = if is_incomplete_parse(&err) {
                "...".to_string()
            } else {
                err.to_string()
            };
            preview.push(OutputElement::Text(text));
        }
        preview
    }
}

pub fn show_debug_overlay(
    mut term: TermWizTerminal,
    gui_win: GuiWin,
    opengl_info: String,
    connection_info: String,
) -> anyhow::Result<()> {
    term.no_grab_mouse_in_raw_mode();

    // L4.7: the overlay REPL now runs on rhai, reusing the same engine factory
    // (`config::rhai_engine::make_rhai_engine`) the config-loading (L4.5) and
    // runtime event-callback bridge (L4.6) paths use, so the REPL shares the
    // exact same `wezterm.*` / `mux.*` / `GuiWin` / `on(...)` surface a real
    // `.wezterm.rhai` callback sees. This was the last live consumer of
    // `mlua::Lua` (the old arm called `config::lua::make_lua_context(...)` to
    // build its own isolated Lua context); removing it leaves no production
    // code path that instantiates `mlua::Lua`, unblocking L5.
    //
    // The host is built on the main thread (and stays there -- `rhai::Engine`
    // is not `Send`): `spawn_into_main_thread` runs `MainReplHost::new` on the
    // GUI thread and stashes the result in the `MAIN_HOST` thread-local, where
    // the per-line evaluation below reaches it. Any creation error propagates
    // out of the overlay via `?`.
    let init_window = gui_win.clone();
    smol::block_on(promise::spawn::spawn_into_main_thread(async move {
        match MainReplHost::new(init_window) {
            Ok(host) => {
                MAIN_HOST.with(|cell| *cell.borrow_mut() = Some(host));
                Ok::<(), anyhow::Error>(())
            }
            Err(e) => Err(e),
        }
    }))?;

    let mut editor_host = Some(ReplEditor::new());

    term.render(&[Change::Title("Debug".to_string())])?;

    let version = config::wezterm_version();
    let triple = config::wezterm_target_triple();

    term.render(&[Change::Text(format!(
        "Debug Overlay\r\n\
         wezterm version: {version} {triple}\r\n\
         Window Environment: {connection_info}\r\n\
         Scripting Engine: rhai\r\n\
         {opengl_info}\r\n\
         Enter rhai statements or expressions and hit Enter.\r\n\
         Press ESC or CTRL-D to exit\r\n",
    ))])?;

    // Run the read/eval loop; the returned `anyhow::Result` carries any
    // terminal/render error so the cleanup below always runs.
    let result = run_repl_loop(&mut term, editor_host.as_mut().unwrap());

    // Drop the main-thread host so the next overlay session starts from a clean
    // scope/AST (and releases its `GuiWin` reference). This runs on the main
    // thread because `MAIN_HOST` is a thread-local that only exists there.
    let _ = smol::block_on(promise::spawn::spawn_into_main_thread(async {
        MAIN_HOST.with(|cell| *cell.borrow_mut() = None);
    }));

    result
}

/// The blocking read-line loop, run on the worker thread. Each completed input
/// fragment is shipped to the main-thread [`MainReplHost`] for evaluation; the
/// returned `ReplOutcome` drives printing and multi-line accumulation.
fn run_repl_loop(term: &mut TermWizTerminal, host: &mut ReplEditor) -> anyhow::Result<()> {
    loop {
        print_new_log_entries(term)?;
        let mut editor = LineEditor::new(term);
        // `>> ` continuation prompt while an incomplete fragment is pending, so
        // the user can tell the REPL is waiting for the rest of a multi-line
        // statement.
        let pending_empty = host.pending.is_empty();
        editor.set_prompt(if pending_empty { "> " } else { ">> " });
        if let Some(line) = editor.read_line(host)? {
            if line.is_empty() && pending_empty {
                continue;
            }
            host.add_history(&line);

            // Accumulate the line into the pending buffer; this is what gets
            // evaluated as one (possibly multi-line) fragment.
            if !host.pending.is_empty() {
                host.pending.push('\n');
            }
            host.pending.push_str(&line);

            let buffer = host.pending.clone();
            let outcome = smol::block_on(promise::spawn::spawn_into_main_thread(
                async move {
                    MAIN_HOST.with(|cell| {
                        let mut borrowed = cell.borrow_mut();
                        // The host was populated at overlay start; if it is
                        // somehow absent (e.g. the GUI is shutting down), treat
                        // the buffer as a no-op rather than panicking.
                        match borrowed.as_mut() {
                            Some(h) => eval_repl_buffer(&h.engine, &mut h.scope, &mut h.main_ast, &buffer),
                            None => ReplOutcome::Done {
                                text: "REPL host unavailable".to_string(),
                            },
                        }
                    })
                },
            ));

            match outcome {
                ReplOutcome::Incomplete => {
                    // `pending` is retained; keep reading continuation lines.
                }
                ReplOutcome::Done { text } => {
                    host.pending.clear();
                    if !text.is_empty() {
                        term.render(&[Change::Text(format!(
                            "{}\r\n",
                            text.replace("\n", "\r\n")
                        ))])?;
                    }
                }
            }
        } else {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_engine() -> Engine {
        // Match the limits `make_rhai_engine` imposes so the parse/eval
        // behavior under test reflects what the real overlay gets.
        let mut engine = Engine::new();
        engine.set_max_expr_depths(64, 64);
        engine.set_max_call_levels(64);
        engine
    }

    /// Feed a single line into a simulated REPL session, mirroring exactly what
    /// `run_repl_loop` + the main-thread host do: append to `pending`, run
    /// `eval_repl_buffer`, clear `pending` on completion. Returns `None` while
    /// the accumulated input is still an incomplete fragment.
    fn feed(
        engine: &Engine,
        scope: &mut Scope,
        main_ast: &mut AST,
        pending: &mut String,
        line: &str,
    ) -> Option<String> {
        if !pending.is_empty() {
            pending.push('\n');
        }
        pending.push_str(line);
        match eval_repl_buffer(engine, scope, main_ast, pending) {
            ReplOutcome::Incomplete => None,
            ReplOutcome::Done { text } => {
                pending.clear();
                Some(text)
            }
        }
    }

    #[test]
    fn format_dynamic_primitives() {
        let e = new_engine();
        assert_eq!(format_dynamic(&e.eval::<Dynamic>("()").unwrap()), "");
        assert_eq!(format_dynamic(&e.eval::<Dynamic>("42").unwrap()), "42");
        assert_eq!(format_dynamic(&e.eval::<Dynamic>("3.14").unwrap()), "3.14");
        assert_eq!(format_dynamic(&e.eval::<Dynamic>("true").unwrap()), "true");
        assert_eq!(format_dynamic(&e.eval::<Dynamic>("'x'").unwrap()), "x");
    }

    #[test]
    fn format_dynamic_string_is_quoted_and_escaped() {
        let e = new_engine();
        // The script literal uses an *escaped* `\n` (not a raw newline): rhai
        // string literals cannot span raw newlines. `format_dynamic` then
        // re-escapes the newline when displaying, mirroring ValuePrinter.
        let v = e.eval::<Dynamic>("\"hi\\nthere\"").unwrap();
        assert_eq!(format_dynamic(&v), "\"hi\\nthere\"");
    }

    #[test]
    fn format_dynamic_array_recursive() {
        let e = new_engine();
        let v = e.eval::<Dynamic>("[1, 2, 3]").unwrap();
        assert_eq!(format_dynamic(&v), "[1, 2, 3]");
        let nested = e.eval::<Dynamic>("[[1, 2], [\"a\", \"b\"]]").unwrap();
        assert_eq!(format_dynamic(&nested), "[[1, 2], [\"a\", \"b\"]]");
    }

    #[test]
    fn format_dynamic_map_sorted_keys() {
        let e = new_engine();
        // Keys are emitted in sorted order regardless of literal order, so the
        // REPL output is deterministic (mirrors ValuePrinter's BTreeMap usage).
        let v = e.eval::<Dynamic>("#{b: 2, a: 1, c: 3}").unwrap();
        assert_eq!(format_dynamic(&v), "#{\"a\": 1, \"b\": 2, \"c\": 3}");
    }

    #[test]
    fn eval_expression_prints_value() {
        let e = new_engine();
        let mut scope = Scope::new();
        let mut main_ast = AST::empty();
        let mut pending = String::new();
        let out = feed(&e, &mut scope, &mut main_ast, &mut pending, "1 + 2 * 3");
        assert_eq!(out.as_deref(), Some("7"));
        assert!(pending.is_empty());
    }

    #[test]
    fn eval_statement_prints_nothing_for_unit() {
        // A statement like `let x = 5;` yields `()`; the overlay prints nothing,
        // just like the old Lua REPL skipped `nil`.
        let e = new_engine();
        let mut scope = Scope::new();
        let mut main_ast = AST::empty();
        let mut pending = String::new();
        let out = feed(&e, &mut scope, &mut main_ast, &mut pending, "let x = 5;");
        assert_eq!(out.as_deref(), Some(""));
    }

    #[test]
    fn let_bindings_persist_across_lines() {
        let e = new_engine();
        let mut scope = Scope::new();
        let mut main_ast = AST::empty();
        let mut pending = String::new();

        feed(&e, &mut scope, &mut main_ast, &mut pending, "let x = 21;");
        let out = feed(&e, &mut scope, &mut main_ast, &mut pending, "x * 2");
        assert_eq!(out.as_deref(), Some("42"));
    }

    #[test]
    fn fn_definitions_persist_across_lines_via_main_ast() {
        // Functions live in the AST, not the scope; the main_ast-merge +
        // clear_statements cycle (copied from the upstream rhai-repl) keeps them
        // callable from later lines.
        let e = new_engine();
        let mut scope = Scope::new();
        let mut main_ast = AST::empty();
        let mut pending = String::new();

        feed(
            &e,
            &mut scope,
            &mut main_ast,
            &mut pending,
            "fn double(n) { n * 2 }",
        );
        let out = feed(&e, &mut scope, &mut main_ast, &mut pending, "double(21)");
        assert_eq!(out.as_deref(), Some("42"));
    }

    #[test]
    fn runtime_error_is_reported_and_does_not_break_session() {
        let e = new_engine();
        let mut scope = Scope::new();
        let mut main_ast = AST::empty();
        let mut pending = String::new();

        let err_out = feed(&e, &mut scope, &mut main_ast, &mut pending, "no_such_thing()");
        let err_text = err_out.expect("error fragment should produce text");
        assert!(
            err_text.contains("Function not found") && err_text.contains("no_such_thing"),
            "expected a 'Function not found' runtime error, got: {err_text}"
        );

        // The session survives: a subsequent valid expression still evaluates.
        let out = feed(&e, &mut scope, &mut main_ast, &mut pending, "6 * 7");
        assert_eq!(out.as_deref(), Some("42"));
    }

    #[test]
    fn syntax_error_is_reported_without_multiline_continuation() {
        // A genuine parse error (not an incomplete fragment) is reported
        // immediately and does not put the REPL into continuation mode.
        let e = new_engine();
        let mut scope = Scope::new();
        let mut main_ast = AST::empty();
        let mut pending = String::new();

        let out = feed(&e, &mut scope, &mut main_ast, &mut pending, "let = 5");
        let text = out.expect("genuine syntax error should produce text");
        assert!(
            !text.is_empty(),
            "syntax error should be reported as non-empty text, got empty"
        );
        assert!(
            !text.contains("..."),
            "a real syntax error must not look like an incomplete-input marker: {text}"
        );
        assert!(
            pending.is_empty(),
            "pending must be cleared after a syntax error"
        );
    }

    #[test]
    fn multiline_if_block_accumulates_until_complete() {
        // An unclosed `{` is an incomplete fragment: the first line yields
        // `None` and the buffer is retained, then once the block is closed the
        // whole thing evaluates.
        let e = new_engine();
        let mut scope = Scope::new();
        let mut main_ast = AST::empty();
        let mut pending = String::new();

        let first = feed(&e, &mut scope, &mut main_ast, &mut pending, "if true {");
        assert_eq!(first, None, "open block should be treated as incomplete");
        assert!(
            !pending.is_empty(),
            "pending must be retained across the open block"
        );

        let second = feed(&e, &mut scope, &mut main_ast, &mut pending, "    123");
        assert_eq!(
            second, None,
            "block still open after a body line must remain incomplete"
        );

        let third = feed(&e, &mut scope, &mut main_ast, &mut pending, "}");
        // `if` is an expression in rhai and yields the block's value.
        assert_eq!(third.as_deref(), Some("123"));
        assert!(pending.is_empty());
    }

    #[test]
    fn multiline_fn_accumulates() {
        let e = new_engine();
        let mut scope = Scope::new();
        let mut main_ast = AST::empty();
        let mut pending = String::new();

        assert_eq!(
            feed(&e, &mut scope, &mut main_ast, &mut pending, "fn inc(n) {"),
            None
        );
        assert_eq!(
            feed(&e, &mut scope, &mut main_ast, &mut pending, "    n + 1"),
            None
        );
        // Still incomplete after the body line; closing the block yields the fn
        // definition, which evaluates to `()` and so prints nothing.
        assert_eq!(
            feed(&e, &mut scope, &mut main_ast, &mut pending, "}"),
            Some(String::new())
        );

        let out = feed(&e, &mut scope, &mut main_ast, &mut pending, "inc(41)");
        assert_eq!(out.as_deref(), Some("42"));
    }

    #[test]
    fn unclosed_array_is_incomplete() {
        let e = new_engine();
        let mut scope = Scope::new();
        let mut main_ast = AST::empty();
        let mut pending = String::new();

        assert_eq!(
            feed(&e, &mut scope, &mut main_ast, &mut pending, "let a = [1, 2,"),
            None
        );
        let out = feed(&e, &mut scope, &mut main_ast, &mut pending, "3];");
        assert_eq!(out.as_deref(), Some(""));
        let out2 = feed(&e, &mut scope, &mut main_ast, &mut pending, "a");
        // `a` is the array built across two lines.
        assert_eq!(out2.as_deref(), Some("[1, 2, 3]"));
    }

    #[test]
    fn is_incomplete_parse_classifies_eof_and_unclosed_groups() {
        let e = new_engine();

        for incomplete in &[
            "if true {",
            "let a = [1, 2,",
            "let x =",
            "fn f() {",
            "#{a: 1,",
            "print(",
        ] {
            let err = e
                .compile(incomplete)
                .expect_err(&format!("`{incomplete}` should fail to compile"));
            assert!(
                is_incomplete_parse(&err),
                "`{incomplete}` should be classified incomplete, got: {err}"
            );
        }

        for complete_or_broken in &["let x = 5;", "1 + 2", "let = 5"] {
            let err = e.compile(complete_or_broken).err();
            if let Some(err) = err {
                assert!(
                    !is_incomplete_parse(&err),
                    "`{complete_or_broken}` should NOT be classified incomplete, got: {err}"
                );
            }
        }
    }

    #[test]
    fn has_open_group_detects_unclosed_groups_and_quotes() {
        // Unclosed groups -> open (this is what unblocks the multi-line if/fn
        // bodies: rhai reports a missing `;` here, but the real issue is the
        // open brace).
        for open in &[
            "if true {",
            "if true {\n    123",
            "fn f() {",
            "fn f() {\n    x",
            "let a = [1, 2,",
            "print(",
            "#{a: 1,",
        ] {
            assert!(has_open_group(open), "should be open: {:?}", open);
        }

        // Unterminated string/char literals -> open (more input could close).
        assert!(has_open_group("\"unterminated"));
        assert!(has_open_group("let s = \"foo"));
        assert!(has_open_group("let c = 'x"));

        // Balanced / complete -> not open.
        for closed in &[
            "let x = 5;",
            "1 + 2",
            "if true { 123 }",
            "fn f() { x }",
            "[1, 2, 3]",
        ] {
            assert!(!has_open_group(closed), "should NOT be open: {:?}", closed);
        }

        // Brackets inside string/char literals or comments must not skew the
        // count -- these are the false-positive guards.
        assert!(!has_open_group("let c = '}';"), "brace inside char literal");
        assert!(!has_open_group("let c = '{';"), "brace inside char literal");
        assert!(!has_open_group("let s = \"}\";"), "brace inside string");
        assert!(
            !has_open_group("// comment with { open brace\nlet x = 1;"),
            "brace inside line comment"
        );
    }
}
