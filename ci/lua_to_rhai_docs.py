#!/usr/bin/env python3
"""
One-shot helper used to convert the Lua code examples embedded in
docs/config/reference/**/*.md into Rhai syntax, following the mapping
documented in docs/migration-lua-to-rhai.md.

This is a scratch/utility script (not part of the build) used during the
Lua-purge documentation pass. It targets the recurring idioms found across
the doc tree:

  * `local onlyterm = require 'onlyterm'` / `local config = onlyterm.config_builder()`
    / `return config` boilerplate -> dropped (the returned map is the whole file)
  * `config.xxx = value` -> kept as-is (still valid: `config` is just a
    variable name used for illustration in these snippets)
  * `onlyterm.action.Foo` / `onlyterm.action.Foo{...}` -> `"Foo"` / `#{ Foo: #{...} }`
  * `onlyterm.foo(...)`  -> `foo(...)` for the documented top-level helpers
  * `onlyterm.color.*`, `onlyterm.serde.*`, `onlyterm.procinfo.*`,
    `onlyterm.plugin.*`, `onlyterm.mux.*` -> `color::*`, `serde::*`, etc.
  * Lua table literals used as maps `{ key = value }` -> `#{ key: value }`
  * Lua table literals used as arrays `{ 'a', 'b' }` -> `[ "a", "b" ]`
  * `foo 'bar'` / `foo { ... }` (Lua's paren-less single-argument call
    syntax) -> `foo("bar")` / `foo(#{ ... })`
  * `obj:method(args)` -> `obj.method(args)`
  * `--` / `--[[ ]]` comments -> `//` / `/* */`
  * single-quoted strings -> double-quoted strings; `[[ ... ]]` long
    strings -> a normal double-quoted string

String literal *contents* (single-quoted, double-quoted, or `[[ ]]`
long-bracket) are extracted up front and replaced with opaque placeholder
tokens before any of the structural (brace/comment/call-syntax) rewriting
runs, then re-inserted verbatim at the end (only re-encoded to the target
quote style). This is what keeps a regex inside a string, or a `key=val`
looking substring inside a URL, from being misinterpreted as code syntax.

It is deliberately conservative: it is meant to be run on individual files
(passed as argv) so that each conversion can be reviewed, not blindly
applied repo-wide in one shot.
"""
import re
import sys

PLACEHOLDER_FMT = "\x00STR{}\x00"
PLACEHOLDER_RE = re.compile(r"\x00STR(\d+)\x00")


def extract_strings(s: str):
    """Replace every Lua string literal ('...', "...", [[...]]) with an
    opaque placeholder, returning (new_text, [rhai_literal_for_each_index]).
    """
    literals = []

    def stash(rhai_text):
        idx = len(literals)
        literals.append(rhai_text)
        return PLACEHOLDER_FMT.format(idx)

    out = []
    i = 0
    n = len(s)
    while i < n:
        c = s[i]
        # Lua block comment --[[ ... ]] : protect verbatim as a comment,
        # not a string, so its contents don't get quote-swapped. Handled
        # before the plain [[ ]] long-string case.
        if s.startswith("--[[", i):
            end = s.find("]]", i + 4)
            if end == -1:
                out.append(s[i:])
                break
            comment_body = s[i + 4:end]
            out.append("/*" + comment_body + "*/")
            i = end + 2
            continue
        if c == "-" and i + 1 < n and s[i + 1] == "-":
            # line comment: copy through end of line verbatim (as `//`),
            # do NOT scan its contents for strings.
            eol = s.find("\n", i)
            if eol == -1:
                eol = n
            out.append("//" + s[i + 2:eol])
            i = eol
            continue
        if s.startswith("[[", i):
            end = s.find("]]", i + 2)
            if end == -1:
                out.append(s[i:])
                break
            inner = s[i + 2:end]
            escaped = inner.replace("\\", "\\\\").replace('"', '\\"')
            out.append(stash(f'"{escaped}"'))
            i = end + 2
            continue
        if c == "'" or c == '"':
            quote = c
            j = i + 1
            buf = []
            while j < n and s[j] != quote:
                if s[j] == "\\" and j + 1 < n:
                    buf.append(s[j:j + 2])
                    j += 2
                else:
                    buf.append(s[j])
                    j += 1
            inner = "".join(buf)
            if quote == "'":
                # re-escape for double-quoted rhai string: unescape \' -> ',
                # escape bare " -> \"
                inner = inner.replace("\\'", "'").replace('"', '\\"')
            out.append(stash(f'"{inner}"'))
            i = j + 1
            continue
        out.append(c)
        i += 1
    return "".join(out), literals


def restore_strings(s: str, literals) -> str:
    return PLACEHOLDER_RE.sub(lambda m: literals[int(m.group(1))], s)


def strip_boilerplate(s: str) -> str:
    s = re.sub(r"^[ \t]*local onlyterm = require\(?\s*\x00STR\d+\x00\s*\)?\n", "", s, flags=re.M)
    s = re.sub(r"^[ \t]*local act = onlyterm\.action\n", "", s, flags=re.M)
    s = re.sub(r"^[ \t]*local mux = onlyterm\.mux\n", "", s, flags=re.M)
    s = re.sub(r"^[ \t]*local config = onlyterm\.config_builder\(\)\n", "", s, flags=re.M)
    s = re.sub(r"^[ \t]*local config = \{\}\n", "", s, flags=re.M)
    s = re.sub(r"\n[ \t]*return config[ \t]*$", "", s)
    s = re.sub(r"^[ \t]*return config\n", "", s, flags=re.M)
    s = re.sub(r"\n[ \t]*return onlyterm\.config_builder\(\)[ \t]*$", "", s)
    return s


def convert_onlyterm_refs(s: str) -> str:
    s = re.sub(r"\bonlyterm\.action_callback\(", "action_callback(", s)
    s = re.sub(r"\bonlyterm\.action\.", "act.", s)
    s = re.sub(r"\bonlyterm\.color\.", "color::", s)
    s = re.sub(r"\bonlyterm\.serde\.", "serde::", s)
    s = re.sub(r"\bonlyterm\.procinfo\.", "procinfo::", s)
    s = re.sub(r"\bonlyterm\.plugin\.", "plugin::", s)
    s = re.sub(r"\bonlyterm\.mux\.", "mux::", s)
    s = re.sub(r"\bonlyterm\.gui\.", "gui::", s)
    s = re.sub(r"\bonlyterm\.nerdfonts\.([A-Za-z0-9_]+)", r'nerdfonts("\1")', s)

    s = re.sub(r"\bonlyterm\.on\(", "on(", s)
    s = re.sub(r"\bonlyterm\.format\(", "format(", s)
    s = re.sub(r"\bonlyterm\.json_encode\(", "serde::json_encode(", s)
    s = re.sub(r"\bonlyterm\.json_decode\(", "serde::json_decode(", s)
    s = re.sub(r"\bonlyterm\.has_action\(", "has_action(", s)
    s = re.sub(
        r"\bonlyterm\.add_to_config_reload_watch_list\(",
        "add_to_config_reload_watch_list(",
        s,
    )

    s = re.sub(r"\bonlyterm\.home_dir\b(?!\()", "home_dir()", s)
    s = re.sub(r"\bonlyterm\.hostname\b(?!\()", "hostname()", s)
    s = re.sub(r"\bonlyterm\.version\b(?!\()", "version()", s)
    s = re.sub(r"\bonlyterm\.config_file\b(?!\()", "config_file()", s)
    s = re.sub(r"\bonlyterm\.config_dir\b(?!\()", "config_dir()", s)
    s = re.sub(r"\bonlyterm\.target_triple\b(?!\()", "target_triple()", s)

    # remaining top-level onlyterm.<fn>(...) calls (e.g. onlyterm.font,
    # onlyterm.font_with_fallback, onlyterm.default_hyperlink_rules,
    # onlyterm.permute_any_mods, ...) simply drop the `onlyterm.` prefix --
    # in rhai these are global functions.
    s = re.sub(r"\bonlyterm\.([A-Za-z_][A-Za-z0-9_]*)", r"\1", s)
    s = re.sub(r"\brequire\(\x00STR\d+\x00\)\.", "", s)

    return s


# Lua/rhai keywords that must never be treated as a paren-less "function
# call" identifier by convert_no_paren_calls (most importantly `return`,
# which is a statement keyword in both languages: `return { ... }` /
# `return #{ ... }` is valid as-is and must NOT become `return(#{ ... })`).
_NOT_A_CALL = {
    "return", "local", "function", "if", "then", "else", "elseif", "end",
    "and", "or", "not", "true", "false", "for", "while", "do", "in",
    "let", "fn", "on",
}


def convert_no_paren_calls(s: str) -> str:
    # `ident.Ident STRPLACEHOLDER` (Lua's paren-less single-string-arg call) ->
    # `ident.Ident(STRPLACEHOLDER)` -- covers `act.CopyMode 'NextMatchPage'`,
    # `onlyterm.font 'Roboto'` (post-prefix-strip: `font 'Roboto'`), etc.
    # Restricted to the same line (no newline in the whitespace) and to
    # identifiers that are themselves in "value position" (preceded by
    # `=`, `(`, `,`, `[`, or start of line) so that this can't accidentally
    # fire on the last word of a `//` comment on the previous line.
    def call_repl(m):
        prefix, ident, arg = m.group(1), m.group(2), m.group(3)
        if ident in _NOT_A_CALL:
            return m.group(0)
        return f"{prefix}{ident}({arg})"

    s = re.sub(
        r"(^|[=(,\[]\s*)([A-Za-z_][A-Za-z0-9_.:]*)[ \t]+(\x00STR\d+\x00)",
        call_repl,
        s,
        flags=re.M,
    )
    # `ident.Ident { ... }` / `ident.Ident #{ ... }` (paren-less
    # single-table-arg call) -> `ident.Ident(#{ ... })`. Must run before
    # convert_table_literals turns `{` into `#{`/`[`, so match plain `{`.
    # Same same-line + value-position restriction as above.
    def call_repl_brace(m):
        prefix, ident, brace = m.group(1), m.group(2), m.group(3)
        if ident in _NOT_A_CALL:
            return m.group(0)
        # Use a marker distinct from a plain "(" so _find_matching_paren_insert
        # can tell a *synthesized* open-paren (which still needs a matching
        # close inserted) apart from a paren that already exists in the
        # source with its own close (e.g. `.push({...})`, `.push(` written by
        # convert_table_insert() already has both parens).
        return f"{prefix}{ident}\x00SYNPAREN\x00{brace}"

    s = re.sub(
        r"(^|[=(,\[]\s*)([A-Za-z_][A-Za-z0-9_.:]*)[ \t]+(\{)",
        call_repl_brace,
        s,
        flags=re.M,
    )
    return s


def convert_local_decls(s: str) -> str:
    # `local NAME = ...` / `local NAME1, NAME2 = ...` -> `let NAME = ...`
    # (the specific `local onlyterm/act/mux/config = ...` boilerplate lines
    # are already stripped entirely by strip_boilerplate(); this handles
    # everything else, e.g. `local SOLID_LEFT_ARROW = onlyterm.nerdfonts...`).
    return re.sub(r"^([ \t]*)local\b", r"\1let", s, flags=re.M)


def _find_matching_paren_insert(s: str) -> str:
    """After the regex above inserts a bare `(` before a `{`, we still need
    to close it with `)` right after the matching `}`. Do a single
    left-to-right scan to insert the missing `)`."""
    out = []
    i = 0
    n = len(s)
    pending_close_after = []  # stack of brace-depths at which we owe a `)`
    depth = 0
    marker = "\x00SYNPAREN\x00"
    marker_len = len(marker)
    while i < n:
        c = s[i]
        if s.startswith(marker, i):
            out.append("(")
            i += marker_len
            # the very next character is guaranteed to be `{` (that's the
            # only shape convert_no_paren_calls ever emits this marker in
            # front of)
            out.append(s[i])
            depth += 1
            pending_close_after.append(depth)
            i += 1
            continue
        if c == "{":
            depth += 1
            out.append(c)
            i += 1
            continue
        if c == "}":
            out.append(c)
            if pending_close_after and pending_close_after[-1] == depth:
                pending_close_after.pop()
                out.append(")")
            depth -= 1
            i += 1
            continue
        out.append(c)
        i += 1
    return "".join(out)


def convert_comments(s: str) -> str:
    # After extract_strings(), line/block comments have already been
    # converted to `//`/`/* */` (done there, since comment scanning needs
    # to happen before string-literal scanning to avoid `--` inside a
    # string being mistaken for a comment marker, and vice versa). Nothing
    # left to do here; kept as a named no-op step for readability of the
    # convert_block() pipeline.
    return s


def convert_method_calls(s: str) -> str:
    # obj:method(args) -> obj.method(args); also obj:method {...} / obj:method
    # STRPLACEHOLDER (Lua's paren-less single-argument call variant applied to
    # a method call, e.g. `pane:split { ... }`, `line:match '...'`) -> the
    # same `obj.method {...}` / `obj.method STRPLACEHOLDER` shape that
    # convert_no_paren_calls() already knows how to turn into a proper
    # `obj.method(...)` call. Simply swapping `:` for `.` here is enough;
    # it deliberately does NOT require a `(` immediately after the method
    # name, unlike a naive `\w+:\w+\(` pattern, which misses the no-paren
    # call forms entirely.
    return re.sub(r"([A-Za-z_][A-Za-z0-9_]*)\s*:\s*([A-Za-z_][A-Za-z0-9_]*)\b", r"\1.\2", s)


def convert_table_insert(s: str) -> str:
    # `table.insert(config.some_list, {` ... `})` -> `config.some_list.push(#{` ... `});`
    return re.sub(
        r"table\.insert\(([A-Za-z_][A-Za-z0-9_.:]*),\s*\n?(\{.*?\})\n?\)",
        r"\1.push(\2);",
        s,
        flags=re.S,
    )


def convert_string_concat(s: str) -> str:
    # `a .. b` -> `a + b`. Operands are either identifiers/calls (end/start
    # with a word char or closing paren) or extracted-string placeholders
    # (`\x00STR<n>\x00`, which both start and end with the `\x00` marker
    # byte), so both must be allowed on either side of the `..`.
    return re.sub(r"(?<=[\w\)\x00])\s\.\.\s(?=[\w\(\x00])", " + ", s)


def convert_misc_ops(s: str) -> str:
    s = re.sub(r"~=", "!=", s)
    s = re.sub(r"\bnil\b", "()", s)
    return s


_IS_MAP_RE = re.compile(r"(^|[,\n{\[])\s*[A-Za-z_][A-Za-z0-9_]*\s*=(?!=)")
_FIELD_SEP_RE = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)\s*=(?!=)")


def convert_table_literals(s: str) -> str:
    """Convert Lua `{ ... }` table constructors to rhai `#{ ... }` (map) or
    `[ ... ]` (array), based on whether the immediate contents look like
    `key = value` pairs or a plain value list.

    Implemented as a single left-to-right scan using a stack, so that each
    `{`/`}` pair is classified and rewritten exactly once (a naive repeated
    global regex substitution mis-classifies already-converted `#{ key: v }`
    maps as arrays on its second pass, since the tell-tale `=` has already
    become `:`). String contents are placeholders at this point (opaque
    `\\x00STR<n>\\x00` tokens), so `=`/`{`/`}` characters that originated
    inside a string literal can no longer confuse the classifier."""

    out = []
    stack = []
    i = 0
    n = len(s)
    while i < n:
        c = s[i]
        if c == "{":
            stack.append(len(out))
            out.append("{")
            i += 1
        elif c == "}" and stack:
            start = stack.pop()
            inner = "".join(out[start + 1:])
            is_map = bool(_IS_MAP_RE.search(inner))
            if is_map:
                inner = _FIELD_SEP_RE.sub(lambda m: f"{m.group(1)}:", inner)
                new_piece = "#{" + inner + "}"
            else:
                new_piece = "[" + inner + "]"
            del out[start:]
            out.append(new_piece)
            i += 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


def convert_block(code: str) -> str:
    # Extract strings/comments first so every subsequent structural
    # transform only ever sees real code syntax, never string contents.
    s, literals = extract_strings(code)

    s = strip_boilerplate(s)
    s = convert_onlyterm_refs(s)
    s = convert_table_insert(s)
    s = convert_method_calls(s)
    s = convert_string_concat(s)
    s = convert_misc_ops(s)
    s = convert_local_decls(s)
    s = convert_no_paren_calls(s)
    s = _find_matching_paren_insert(s)
    s = convert_table_literals(s)

    s = restore_strings(s, literals)
    s = s.strip("\n")
    return s


# Only match fences that start at column 0 (top-level fences). Fences
# indented under an admonition (`!!! note` etc.) are intentionally left
# alone -- they're rare (a handful of docs use them to show an alternative
# one-line snippet) and are handled by hand where they appear.
FENCE_RE = re.compile(r"^```lua\n(.*?)\n^```[ \t]*$", re.S | re.M)


def process_file(path):
    with open(path, encoding="utf-8") as f:
        content = f.read()
    if not re.search(r"^```lua$", content, re.M):
        return False

    def repl(m):
        code = m.group(1)
        converted = convert_block(code)
        return f"```rhai\n{converted}\n```"

    new_content = FENCE_RE.sub(repl, content)
    if new_content != content:
        with open(path, "w", encoding="utf-8") as f:
            f.write(new_content)
        return True
    return False


def main():
    targets = sys.argv[1:]
    changed = []
    for p in targets:
        if process_file(p):
            changed.append(p)
    print(f"converted {len(changed)} files")


if __name__ == "__main__":
    main()
