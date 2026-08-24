---
tags:
  - gpu
---
# `front_end = "WebGpu"`

Specifies which render front-end to use.

{{since('20221119-145034-49b9839f', inline=True)}}
    `WebGpu` is available as a front end.

This fork has removed the legacy OpenGL renderer (and its `Software`/Mesa
variant) entirely, so `WebGpu` is now the only supported value, and also the
default.

If you have an old config with `front_end = "OpenGL"` or `front_end =
"Software"` left over from before this change, it will still load: those
values are recognized as deprecated aliases for `"WebGpu"` and a warning is
logged explaining that the OpenGL renderer is gone. You should update your
config to remove the setting (or set it explicitly to `"WebGpu"`) to silence
the warning.

The `WebGpu` front end gets a dedicated per-window render thread (see
`webgpu_render_thread`), which isolates a stuck GPU driver call so that it
can't freeze the whole process.

The WebGpu front end allows onlyterm to use GPU acceleration provided by
a number of platform-specific backends:

* Metal (on macOS)
* Vulkan
* DirectX 12 (on Windows)

If WebGpu adapter/device initialization fails outright (for example in a VM
without GPU passthrough, or due to a driver mismatch), onlyterm will report a
clear error explaining what went wrong rather than silently degrading or
leaving a blank window on screen.

See also:

* [webgpu_preferred_adapter](webgpu_preferred_adapter.md)
* [webgpu_power_preference](webgpu_power_preference.md)
* [webgpu_force_fallback_adapter](webgpu_force_fallback_adapter.md)
