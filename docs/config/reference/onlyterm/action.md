---
title: onlyterm.action
tags:
 - keys
---

# `act` (formerly `onlyterm.action`)

!!! danger "Removed: no scripting engine"

    This page documents part of the rhai (and, before that, Lua) **scripting
    API**, which has been removed entirely. OnlyTerm's configuration format
    is now [ktav](../../../migration-to-ktav.md), a static `key: value` data
    format with no expressions, function calls, or callbacks of any kind --
    there is nothing left in OnlyTerm that could call this function, invoke
    this method, or construct this object. The description and examples
    below are kept for historical reference (e.g. if you're migrating a very
    old config and trying to understand what it used to do), but none of it
    is callable today. See the [changelog](../../../changelog.md#continuousnightly)
    for the full rationale.

Helper for defining key assignment actions in your configuration file.
This is really just sugar for the underlying Rust deserialization mapping
that makes it a bit easier to identify where syntax errors may exist in
your configuration file.

## Constructor Syntax

{{since('20220624-141144-bd1b7c5d')}}

`act` is a special module that makes it a bit more ergonomic to express
the various actions than the plain object-map form described in *Older
versions* below. The older form is still supported, so you needn't
scramble to update your configuration files.

Referencing `act` with a valid
[KeyAssignment](../keyassignment/index.md) name will act as a constructor for
that key assignment type.  For example, the rhai expression:

```
act.QuickSelectArgs
```

is a constructor for [QuickSelectArgs](../keyassignment/QuickSelectArgs.md).

If the key assignment type is a *unit variant* (has no parameters) such as
[Copy](../keyassignment/Copy.md), or can be constructed with default values
such as [QuickSelectArgs](../keyassignment/QuickSelectArgs.md) then you can
reference the constructor directly to have it evaluate as that value without
having to add any extra punctuation:

```rhai
return #{
  keys: [
    #{
      key: " ",
      mods: "CTRL|SHIFT",
      action: act.QuickSelectArgs,
    },
  ],
}
```

You may pass the optional parameters to `QuickSelectArgs` as you need
them, like this:

```rhai
return #{
  keys: [
    #{
      key: " ",
      mods: "CTRL|SHIFT",
      action: act.QuickSelectArgs(#{
        alphabet: "abc",
      }),
    },
  ],
}
```

If the key assignment type is a *tuple variant* (has positional parameters)
such as [ActivatePaneByIndex](../keyassignment/ActivatePaneByIndex.md), then
you can pass those by calling the constructor:

```rhai
// shortcut to save typing below

return #{
  keys: [
    #{ key: "F1", mods: "ALT", action: act.ActivatePaneByIndex(0) },
    #{ key: "F2", mods: "ALT", action: act.ActivatePaneByIndex(1) },
    #{ key: "F3", mods: "ALT", action: act.ActivatePaneByIndex(2) },
    #{ key: "F4", mods: "ALT", action: act.ActivatePaneByIndex(3) },
    #{ key: "F5", mods: "ALT", action: act.ActivatePaneByIndex(4) },
    #{ key: "F6", mods: "ALT", action: act.ActivatePaneByIndex(5) },
    #{ key: "F7", mods: "ALT", action: act.ActivatePaneByIndex(6) },
    #{ key: "F8", mods: "ALT", action: act.ActivatePaneByIndex(7) },
    #{ key: "F9", mods: "ALT", action: act.ActivatePaneByIndex(8) },
    #{ key: "F10", mods: "ALT", action: act.ActivatePaneByIndex(9) },

    // Compare this with the older syntax shown in the section below
    #{ key: "{", mods: "CTRL", action: act.ActivateTabRelative(-1) },
    #{ key: "}", mods: "CTRL", action: act.ActivateTabRelative(1) },
  ],
}
```

## Older versions

For versions before *20220624-141144-bd1b7c5d*, usage looks like this:

```rhai
return #{
  keys: [
    #{
      key: "{",
      mods: "CTRL",
      action: #{
        ActivateTabRelative: -1,
      },
    },
    #{
      key: "}",
      mods: "CTRL",
      action: #{
        ActivateTabRelative: 1,
      },
    },
  ],
}
```

The action value is simply an object map whose single key names the
KeyAssignment variant and whose value carries its parameters (see [Key
bindings and actions](../../../migration-to-ktav.md#key-bindings-and-actions)
in the migration guide). These docs aim to spell out sufficient examples that
you shouldn't need to learn to read Rust code, but there are occasions where
newly developed features are not yet documented and an enterprising user may
wish to go spelunking to figure them out!

[You can find the reference for available KeyAssignment values here](../keyassignment/index.md).
