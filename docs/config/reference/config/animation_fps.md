---
tags:
  - tuning
---
# `animation_fps = 10`

{{since('20220319-142410-0fcdea07')}}

This setting controls the maximum frame rate used when rendering easing effects
for blinking cursors, blinking text and visual bell.

Setting it larger will result in smoother easing effects but will increase GPU
utilization.

If your system doesn't have a usable GPU (or is otherwise slow to render
easing effects, e.g. in a VM without GPU passthrough), then setting
`animation_fps = 1` is recommended, as doing so will disable easing effects
and use transitions:

```
animation_fps: 1
cursor_blink_ease_in: Constant
cursor_blink_ease_out: Constant
```

