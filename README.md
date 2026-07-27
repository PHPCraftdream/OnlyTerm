# OnlyTerm

OnlyTerm is a fork of [wezterm/wezterm](https://github.com/wezterm/wezterm) focused on stability and ease of use on Windows. Other platforms supported by upstream (macOS, Linux/X11, Wayland) are a secondary priority here — support for them will grow as additional maintainers join who are willing to put in the time and have access to modern AI tooling/subscriptions to help accelerate the work.

*A GPU-accelerated cross-platform terminal emulator and multiplexer, forked from the project originally written by <a href="https://github.com/wez">@wez</a> and implemented in <a href="https://www.rust-lang.org/">Rust</a>.*

## What this is

OnlyTerm is a terminal emulator and multiplexer with GPU-accelerated rendering. Key capabilities inherited from upstream:

* Cross-platform architecture (Windows/macOS/Linux, X11/Wayland) — with maintenance focus on Windows in this fork.
* Multiplexing of panes/tabs/windows, including across remote unix-domain mux domains.
* Flexible configuration via Lua/Rhai: color schemes, fonts, key bindings, custom events.
* Support for modern terminal protocols (Kitty graphics/keyboard, OSC 52 clipboard, synchronized output, and more).

## What this fork focuses on

This is built on the wezterm/wezterm codebase, with a set of real-world bug and stability fixes ported in — primarily ones affecting Windows: GUI hangs and crashes under load, ConPTY-related races, correctness of pane resize/split handling, input and scroll responsiveness, keyboard-layout-independent standard key bindings, and other targeted fixes. The internal naming of the code (crate names, environment variables, system identifiers) has been left as-is, as a mark of appreciation for the upstream authors and contributors.

## Installation

There isn't a separate binary distribution for OnlyTerm yet — build the fork from source (`cargo build --release`) or refer to upstream's general instructions: https://wezterm.org/installation

## Getting help

Since this is a personal/small fork, the issue tracker for this repository is the first place to look. For general questions about the terminal itself (not specific to this fork), the upstream community channels are also available:

* [Upstream GitHub issue tracker](https://github.com/wezterm/wezterm/issues)
* [Upstream GitHub Discussions](https://github.com/wezterm/wezterm/discussions)
* [Upstream Matrix room via Element.io](https://matrix.to/#/#wezterm:matrix.org)

## Attribution and license

This project is a fork of [wezterm/wezterm](https://github.com/wezterm/wezterm), created and maintained by [Wez Furlong](https://github.com/wez) and upstream contributors. The original project's license and copyright are preserved unchanged — see [LICENSE.md](LICENSE.md).
