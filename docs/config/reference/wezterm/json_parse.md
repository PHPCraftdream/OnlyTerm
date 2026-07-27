---
title: wezterm.json_parse
tags:
 - utility
 - json
---


# `wezterm.json_parse(string)`

{{since('20220807-113146-c2fee766')}}

Parses the supplied string as json and returns the equivalent rhai value:

```
> json_parse("{\"foo\":\"bar\"}")
#{
    "foo": "bar",
}
```

This is also available as `serde::json_decode(string)`.
