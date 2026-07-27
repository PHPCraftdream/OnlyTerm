---
title: wezterm.json_encode
tags:
 - utility
 - json
---

# `wezterm.json_encode(value)`

{{since('20220807-113146-c2fee766')}}

Encodes the supplied value as json:

```
> json_encode(#{ foo: "bar" })
"{\"foo\":\"bar\"}"
```

This is also available as `serde::json_encode(value)`.
