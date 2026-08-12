## Installing from source

If your system isn't covered by the pre-built packages then you can build it
for yourself.  OnlyTerm runs on Windows 10 and later.

* Install `rustup` to get the `rust` compiler installed on your system.
  [Install rustup](https://rust-lang.org/tools/install/).
* Rust version 1.71 or later is required
* Build in release mode: `cargo build --release`
* Run it via either `cargo run --release --bin onlyterm-gui` or `target/release/onlyterm-gui`

### Building on Windows

When installing Rust, you must use select the MSVC version of Rust. It is the
only supported way to build OnlyTerm.

The only other dependency that you need is
[Strawberry Perl](https://strawberryperl.com). You must ensure that you have
your `PATH` environment set up to find that particular `perl.exe` ahead of any
other perl that you may have installed on your system. This particular version
of perl is required to build openssl on Windows.

```console
set PATH=c:\Strawberry\perl\bin;%PATH%
```

