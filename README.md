# OnlyTerm

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows-0078D6?logo=windows&logoColor=white)](https://github.com/PHPCraftdream/OnlyTerm)
[![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![100% Rust](https://img.shields.io/badge/100%25-Rust-dea584?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Last commit](https://img.shields.io/github/last-commit/PHPCraftdream/OnlyTerm)](https://github.com/PHPCraftdream/OnlyTerm/commits/main)
[![Issues](https://img.shields.io/github/issues/PHPCraftdream/OnlyTerm)](https://github.com/PHPCraftdream/OnlyTerm/issues)
[![Stars](https://img.shields.io/github/stars/PHPCraftdream/OnlyTerm?style=social)](https://github.com/PHPCraftdream/OnlyTerm/stargazers)

OnlyTerm is a fork of [wezterm/wezterm](https://github.com/wezterm/wezterm) focused on stability and ease of use on Windows. Other platforms supported by upstream (macOS, Linux/X11, Wayland) are a secondary priority here — support for them will grow as additional maintainers join who are willing to put in the time and have access to modern AI tooling/subscriptions to help accelerate the work.

*A GPU-accelerated cross-platform terminal emulator and multiplexer, forked from the project originally written by <a href="https://github.com/wez">@wez</a> and implemented in <a href="https://www.rust-lang.org/">Rust</a>.*

## What this is

OnlyTerm is a terminal emulator and multiplexer with GPU-accelerated rendering. Key capabilities inherited from upstream:

* Cross-platform architecture (Windows/macOS/Linux, X11/Wayland) — with maintenance focus on Windows in this fork.
* Multiplexing of panes/tabs/windows, including across remote unix-domain mux domains.
* Flexible configuration via rhai: color schemes, fonts, key bindings, custom events.
* Support for modern terminal protocols (Kitty graphics/keyboard, OSC 52 clipboard, synchronized output, and more).

## What this fork focuses on

This is built on the wezterm/wezterm codebase, with a set of real-world bug and stability fixes ported in — primarily ones affecting Windows: GUI hangs and crashes under load, ConPTY-related races, correctness of pane resize/split handling, input and scroll responsiveness, keyboard-layout-independent standard key bindings, and other targeted fixes. The internal naming of the code (crate names, environment variables, system identifiers) has been left as-is, as a mark of appreciation for the upstream authors and contributors.

OnlyTerm is also **100% Rust**: the last remaining C dependency (`zstd-sys`, used to compress mux-protocol messages between the client and server) has been replaced with `flate2`'s pure-Rust `miniz_oxide` backend. Nothing in the runtime binaries links or compiles any bundled C library anymore — the only C/C++ still touched anywhere in the build is a tiny build-time-only helper (`vswhom-sys`, via `embed-resource`) used to locate the MSVC toolchain so the application icon can be embedded into the `.exe`, which never ships as part of the running program.

## Installation

There isn't a separate binary distribution for OnlyTerm yet — build it from source:

```
git clone https://github.com/PHPCraftdream/OnlyTerm.git
cd onlyterm
cargo build --release
```

For platform-specific build dependencies, see upstream's [Install from Source](https://wezfurlong.org/wezterm/install/source.html) guide.

## Getting help

Since this is a personal/small fork, the [issue tracker for this repository](https://github.com/PHPCraftdream/OnlyTerm/issues) is the first place to look. For general questions about the terminal itself (not specific to this fork), the upstream community channels are also available:

* [Upstream GitHub issue tracker](https://github.com/wezterm/wezterm/issues)
* [Upstream GitHub Discussions](https://github.com/wezterm/wezterm/discussions)
* [Upstream Matrix room via Element.io](https://matrix.to/#/#wezterm:matrix.org)

## Attribution and license

This project is a fork of [wezterm/wezterm](https://github.com/wezterm/wezterm), created and maintained by [Wez Furlong](https://github.com/wez) and upstream contributors. The original project's license and copyright are preserved unchanged — see [LICENSE.md](LICENSE.md).
