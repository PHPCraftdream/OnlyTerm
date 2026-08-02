---
title: wezterm.permute_any_mods
tags:
 - utility
 - keys
---
# `wezterm.permute_any_mods(table)`

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

{{since('20201031-154415-9614e117')}}

This function is intended to help with generating key or mouse binding
entries that should apply regardless of the combination of modifier keys
pressed.

For each combination of modifiers `CTRL`, `ALT`, `SHIFT` and `SUPER`,
the supplied table value is copied and has `mods = <value>` set into
the copy.

An entry for `NONE` is *NOT* generated (this is the only
difference between `permute_any_mods` and `permute_any_or_no_mods`).

An array holding all of those combinations is returned.

Since `permute_any_mods` already returns an array, if this is your only
binding you can use its result directly as `mouse_bindings`:

```rhai
return #{
  mouse_bindings: permute_any_mods(#{
    event: #{ Down: #{ streak: 1, button: "Middle" } },
    action: "PastePrimarySelection",
  }),
}
```

If you have other bindings before and/or after, concatenate the arrays with `+`:

```rhai
return #{
  mouse_bindings: my_other_bindings + permute_any_mods(#{
    event: #{ Down: #{ streak: 1, button: "Middle" } },
    action: "PastePrimarySelection",
  }),
}
```

This is equivalent to writing this out, but is much less verbose:

```rhai
return #{
  mouse_bindings: [
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "SUPER",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "ALT",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "ALT | SUPER",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "SHIFT",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "SHIFT | SUPER",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "SHIFT | ALT",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "SHIFT | ALT | SUPER",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "CTRL",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "CTRL | SUPER",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "ALT | CTRL",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "ALT | CTRL | SUPER",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "SHIFT | CTRL",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "SHIFT | CTRL | SUPER",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "SHIFT | ALT | CTRL",
    },
    #{
      action: "PastePrimarySelection",
      event: #{
        Down: #{
          button: "Middle",
          streak: 1,
        },
      },
      mods: "SHIFT | ALT | CTRL | SUPER",
    },
  ],
}
```

