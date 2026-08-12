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

On Windows, the only other dependency besides Rust is
[Strawberry Perl](https://strawberryperl.com) for building openssl.
Make sure your `PATH` is set up to find that particular `perl.exe` ahead of any other perl.

If building from the git repo, did you update the submodules? Not doing this
is a common source of problems; see the information at
<https://wezfurlong.org/wezterm/install/source.html> for more information.

## The build output

Please include the output from running the build command:

```
cargo build --release
```
