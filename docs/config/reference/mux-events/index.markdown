# Events emitted by the Multiplexer

!!! danger "Removed: no scripting engine"

    Event hooks required `onlyterm.on`, part of the rhai (and, before that,
    Lua) **scripting API**, which has been removed entirely. OnlyTerm's
    configuration format is now [ktav](../../../migration-to-ktav.md), a
    static `key: value` data format with no expressions, function calls, or
    callbacks of any kind -- there is nothing left in OnlyTerm that could
    register or emit these events. The descriptions below are kept for
    historical reference. See the
    [changelog](../../../changelog.md#continuousnightly) for the full
    rationale and the list of removed event hooks.

The following events could previously be handled using [onlyterm.on](../onlyterm/on.md):

