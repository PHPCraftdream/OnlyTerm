//! Task #422: investigating a real `EXCEPTION_STACK_OVERFLOW` seen in
//! production (two crash dumps, identical crash address, both at
//! 5-7s of uptime) with the reported crash frame inside
//! `FontDatabase::with_font_dirs` (`crates/onlyterm-font/src/db.rs`)
//! while scanning `C:/Windows/Fonts` led here instead of to font
//! parsing: bisecting the crash (shrinking the font-file set under
//! test, then isolating individual `ttf_parser`/`swash` accessor
//! calls on the single file the bisection converged on,
//! `EmojiOneColor-SVGinOT.ttf`) showed every font-parsing call
//! succeeding fine even on a 64KiB thread stack -- ruling out
//! COLR/COLRv1 paint-graph recursion, TTC collection handling, and
//! cmap/coverage walking as the cause (see
//! `crates/onlyterm-font/src/db.rs`'s `stack_overflow_repro` module
//! for the font-side half of this investigation).
//!
//! What *did* reproduce the overflow in isolation, with no font file
//! involved at all, was the first call to `Config::default()` in the
//! process. `Config` derives `FromDynamic`
//! (`crates/onlyterm-dynamic/derive/src/fromdynamic.rs`), and for a
//! struct with 200+ fields that derive expands into one large
//! `from_dynamic` function body containing one field-initializer
//! expression per field (each doing a map lookup, an
//! `Option`/`Result` match, and a fallback-to-default) -- all inlined
//! into a single `Ok(Self { .. })` struct literal, not a bounded
//! recursion. In an unoptimized/debug build this function's own
//! stack frame measured as needing on the order of 256-512KiB to
//! avoid overflowing (down to ~128-256KiB in an optimized release
//! build); `Config`'s own resting size (`std::mem::size_of::<Config>()`,
//! asserted below) is a few KiB, so this is about the *construction*
//! being frame-heavy, not the final value being large.
//!
//! This is comfortably within the 1MiB stack Windows gives a
//! process's main thread by default (confirmed nothing in this
//! codebase runs `Config::default()`/`config::configuration()` on a
//! smaller custom-stack-size thread), and re-running the exact
//! crashing configuration directly against a freshly built GUI
//! binary did not reproduce the crash within a comparable uptime
//! window. So this size-of/stack-cost regression test exists as a
//! canary rather than as a fix for a confirmed live bug: if `Config`
//! keeps growing and this ever needs meaningfully more than 512KiB
//! in a debug build, that is worth knowing before it gets any closer
//! to the 1MiB default main-thread budget.
#[test]
#[cfg_attr(
    not(windows),
    ignore = "stack_size probing is most meaningful on Windows' fixed 1MiB main-thread default"
)]
fn config_default_fits_comfortably_under_half_the_default_stack() {
    // Half of the 1MiB Windows gives a process's main thread by
    // default: if `Config::default()` needs anywhere near this much
    // margin, that is a sign the derive-generated `from_dynamic` is
    // creeping towards being a real risk on the actual default stack,
    // not just a probe artifact.
    const HALF_OF_DEFAULT_MAIN_THREAD_STACK: usize = 512 * 1024;

    // The struct's own resting size is unrelated to the derive-generated
    // constructor's stack-frame cost above -- assert it stays small so
    // the doc comment's "construction is frame-heavy, not the value
    // itself" claim keeps being true rather than silently going stale.
    assert!(
        std::mem::size_of::<crate::Config>() < 16 * 1024,
        "Config::size_of() grew unexpectedly large ({} bytes) -- re-check whether the \
             frame-heavy-construction-not-large-value assumption in this module's doc comment \
             still holds",
        std::mem::size_of::<crate::Config>()
    );

    let handle = std::thread::Builder::new()
        .name("config-default-stack-canary".into())
        .stack_size(HALF_OF_DEFAULT_MAIN_THREAD_STACK)
        .spawn(crate::Config::default)
        .expect("failed to spawn probe thread");

    // Note: on Windows, an actual stack overflow hits the guard page and
    // aborts the whole process (Rust cannot turn it into a catchable
    // panic), so if this probe thread genuinely overflows, the test
    // binary dies outright rather than this specific test failing with
    // the message below -- that message is only reachable if
    // `Config::default()` panics here for some *other* reason. The
    // process-level abort is still a visible, loud CI failure either
    // way, just not this friendlier one.
    handle.join().expect(
        "Config::default() panicked (not necessarily a stack overflow -- a real overflow \
             would abort the whole process instead of reaching this message) on a thread stack \
             half the size of Windows' default 1MiB main-thread stack -- FromDynamic's generated \
             from_dynamic for Config has grown large enough to be a real stack-overflow risk \
             (see task #422); consider whether new fields can avoid growing this further, or \
             whether the derive macro needs to split large structs' from_dynamic bodies into \
             smaller, separately-framed helper functions",
    );
}
