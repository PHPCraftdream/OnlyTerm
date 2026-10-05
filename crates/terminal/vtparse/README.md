# vtparse

This is an implementation of a parser for escape and control sequences.
It is based on the [DEC ANSI Parser](https://vt100.net/emu/dec_ansi_parser).

It has been modified slightly to support UTF-8 sequences.

`vtparse` is the lowest level parser; it categorizes the basic
types of sequences but does not ascribe any semantic meaning
to them.

You may wish to look at `termwiz::escape::parser::Parser` in the
[termwiz](https://docs.rs/termwiz) crate if you're looking for semantic
parsing.

## Comparison with the `vte` crate

`vtparse` has support for dynamically sized OSC buffers, which makes
it suitable for processing large escape sequences, such as those
used by the `iTerm2` image protocol.

## Cargo features

- `std` (enabled by default) uses dynamically sized OSC/APC buffers and exposes
  `CollectingVTActor` and `VTAction`.
- `alloc` provides the same allocation-backed parser APIs from a `no_std` crate;
  the final application must provide an allocator.
- With neither `std` nor `alloc` (including `--no-default-features`), the crate
  remains `no_std`, uses fixed-capacity `heapless` OSC storage, and does not
  expose allocation-backed collecting/APC APIs. The OSC byte buffer is limited
  to 1,024 bytes in this mode.
- `no_std` is an explicit feature marker for downstream feature forwarding. The
  crate is already `no_std` whenever `std` is disabled; enabling `no_std` does
  not disable `std` if both features are selected.

The `heapless` dependency is required because the featureless configuration is
a supported parser mode, not an invalid feature combination.

Run this workspace-root matrix to exercise every declared feature combination:

```sh
cargo test -p vtparse
cargo test -p vtparse --no-default-features
cargo test -p vtparse --no-default-features --features std
cargo test -p vtparse --no-default-features --features alloc
cargo test -p vtparse --no-default-features --features "std,alloc"
cargo test -p vtparse --no-default-features --features no_std
cargo test -p vtparse --no-default-features --features "no_std,alloc"
cargo test -p vtparse --no-default-features --features "std,no_std"
cargo test -p vtparse --no-default-features --features "std,alloc,no_std"
```
