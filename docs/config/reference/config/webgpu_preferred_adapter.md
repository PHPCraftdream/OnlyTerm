---
tags:
  - gpu
---
# `webgpu_preferred_adapter`

{{since('20221119-145034-49b9839f')}}

Specifies which WebGpu adapter should be used.

This option is only applicable when you have configured `front_end = "WebGpu"`.

You can use the [wezterm.gui.enumerate_gpus()](../wezterm.gui/enumerate_gpus.md) function
to return a list of GPUs.

If you open the [Debug Overlay](../keyassignment/ShowDebugOverlay.md) (default:
<kbd>CTRL</kbd> + <kbd>SHIFT</kbd> + <kbd>L</kbd>) you can interactively review
the list:

```
> wezterm.gui.enumerate_gpus()
[
    {
        "backend": "Vulkan",
        "device": 29730,
        "device_type": "DiscreteGpu",
        "driver": "radv",
        "driver_info": "Mesa 22.3.4",
        "name": "AMD Radeon Pro W6400 (RADV NAVI24)",
        "vendor": 4098,
    },
    {
        "backend": "Vulkan",
        "device": 0,
        "device_type": "Cpu",
        "driver": "llvmpipe",
        "driver_info": "Mesa 22.3.4 (LLVM 15.0.7)",
        "name": "llvmpipe (LLVM 15.0.7, 256 bits)",
        "vendor": 65541,
    },
    {
        "backend": "Gl",
        "device": 0,
        "device_type": "Other",
        "name": "AMD Radeon Pro W6400 (navi24, LLVM 15.0.7, DRM 3.49, 6.1.9-200.fc37.x86_64)",
        "vendor": 4098,
    },
]
```

Based on that list, I might choose to explicitly target the discrete Gpu like
this (but note that this would be the default selection anyway):

```
webgpu_preferred_adapter: {
  backend: Vulkan
  device: 29730
  device_type: DiscreteGpu
  driver: radv
  driver_info: "Mesa 22.3.4"
  name: "AMD Radeon Pro W6400 (RADV NAVI24)"
  vendor: 4098
}
front_end: WebGpu
```

!!! note "Selecting a GPU programmatically is no longer possible"

    Earlier versions of this page also showed how to call
    `wezterm.gui.enumerate_gpus()` from a config script to pick the first
    available GPU, or to loop over the list and pick the first Vulkan
    integrated GPU. `wezterm.gui` was part of the scripting API and has been
    removed along with the rest of the scripting engine — see the
    [changelog](../../../changelog.md#continuousnightly) — and ktav has no
    loops or function calls to replace that logic with. You can still find
    the exact `backend`/`device`/`device_type`/`driver`/`driver_info`/`name`/
    `vendor` values to hardcode via the Debug Overlay, as shown above; there
    is currently no way to select a GPU by category (e.g. "the first
    integrated GPU") without knowing its exact identifying fields ahead of
    time.

See also [webgpu_power_preference](webgpu_power_preference.md),
[webgpu_force_fallback_adapter](webgpu_force_fallback_adapter.md).
