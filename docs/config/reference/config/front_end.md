---
tags:
  - gpu
---
# `front_end = "OpenGL"`

Specifies which render front-end to use.  This option used to have
more scope in earlier versions of wezterm, but today it allows three
possible values:

* `OpenGL` - use GPU accelerated rasterization
* `Software` - use CPU-based rasterization.
* `WebGpu` - use GPU accelerated rasterization {{since('20221119-145034-49b9839f', inline=True)}}

{{since('20240127-113634-bbcac864', outline=true)}}
    The default is `"WebGpu"`. In earlier versions it was `"OpenGL"`

{{since('20240128-202157-1e552d76', outline=true)}}
    The default has been reverted to `"OpenGL"`.

On Windows, this fork now defaults to `"WebGpu"`, with automatic fallback to
`"OpenGL"` if WebGpu adapter/device initialization fails (for example in an
RDP session, on an old or software-only GPU, in a VM without GPU passthrough,
or due to a driver mismatch). Other platforms still default to `"OpenGL"`.

Only the `WebGpu` front end gets a dedicated per-window render thread (see
`webgpu_render_thread`), which isolates a stuck GPU driver call so it can't
freeze the whole process. `OpenGL` and `Software` remain fully synchronous on
the GUI thread, so a hung GL driver call can still freeze every window in
the process.

You may wish (or need!) to select `Software` if there are issues with your
GPU/OpenGL drivers.

WezTerm will force software rasterization (SWRAST) within the OpenGL/EGL/WGL
code path if it detects that it is being started in a Remote Desktop
environment on Windows; this does not change the `front_end` setting itself,
it only affects how the OpenGL backend renders.

## WebGpu

{{since('20221119-145034-49b9839f')}}

The WebGpu front end allows wezterm to use GPU acceleration provided by
a number of platform-specific backends:

* Metal (on macOS)
* Vulkan
* DirectX 12 (on Windows)

See also:

* [webgpu_preferred_adapter](webgpu_preferred_adapter.md)
* [webgpu_power_preference](webgpu_power_preference.md)
* [webgpu_force_fallback_adapter](webgpu_force_fallback_adapter.md)
