# `wezterm.mux.rename_workspace(old, new)`

{{since('20230408-112425-69ae8472')}}

Renames the workspace *old* to *new*.

```rhai
mux::rename_workspace(
  mux::get_active_workspace(),
  "something different"
)
```
