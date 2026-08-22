---
name: Build Problem
about: Having problems building from source?
title: ''
labels: [bug, needs:triage]
assignees: ''

---

## Build Environment (please complete the following information):

 - OS: [e.g. Windows 10, Windows 11]. Please include `systeminfo | findstr /B /C:"OS Name" /C:"OS Version"` in your report.
 - Compiler: are you using `Microsoft Visual Studio` or something else? Which version?
 - Rust version: Please include the output from `rustup show`. Best results are
   generally had with a recent stable version of the rust toolchain.

## Dependencies

No additional dependencies are required beyond Rust and a recent MSVC compiler.

If building from the git repo, did you update the submodules? Not doing this
is a common source of problems: run `git submodule update --init --recursive`.

## The build output

Please include the output from running the build command:

```
cargo build --release
```
