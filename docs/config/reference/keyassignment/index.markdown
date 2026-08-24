# `KeyAssignment` enumeration

A `KeyAssignment` represents a pre-defined function that can be applied
to control the Window, Tab, Pane state typically when a key or mouse event
is triggered.

Internally, in the underlying Rust code, `KeyAssignment` is an enum
type with a variant for each possible action known to onlyterm. In a ktav
config, a *parameterized* variant is written as a single-key object whose
key is the variant name (e.g. `{ SpawnCommandInNewTab: { cwd: /tmp } }`),
while a *unit* variant (no arguments) is just its bare name (e.g. `Copy`).
See [Migrating to a ktav config](../../../migration-to-ktav.md#key-bindings-and-actions)
for the full translation and worked examples.

(`onlyterm.action`, referenced on some of the pages below, was a scripting
helper for constructing these values in Lua/rhai configs; it has been
removed along with the rest of the scripting engine -- see
[onlyterm.action](../onlyterm/action.md) for details.)

## Available Key Assignments


