# Карта удаления OpenGL из OnlyTerm (задача #412)

Исследование от 2026-08-05. Код не менялся. Документ — основа для задач #413–#416.
Ключевые утверждения перепроверены независимо (не только со слов исследующего агента).

## Блокирующие архитектурные вопросы

Прежде чем удалять GL, нужны решения пользователя по двум пунктам — оба меняют
поведение продукта, а не только внутреннее устройство.

### 1. Чем заменить аварийный фолбэк при отказе WebGpu?

Сейчас существует полноценная подсистема спасения в
`crates/onlyterm-gui/src/termwindow/render_pipeline.rs`:

| Место | Строки | Роль |
|---|---|---|
| `new_window` | 331–359 | если `WebGpuState::new` упал при создании окна → `window.enable_opengl()` |
| `attempt_renderer_rebuild_or_close` | 608–687 | до 3 попыток пересобрать WebGpu за 30 сек |
| `begin_opengl_fallback` | 724–774 | после исчерпания попыток — переход на GL |
| `finish_opengl_fallback` | 802–887 | завершение перехода |
| `permanently_on_opengl: Cell<bool>` | `mod.rs:548` | окно необратимо остаётся на GL |
| `opengl_fallback_relay` | `mod.rs:528-529` | канал для `Rc<glium::backend::Context>` между async-тасками и GUI-потоком |

После удаления GL этот путь исчезает. Варианты замены:

- **(A)** `webgpu_force_fallback_adapter=true` — программный адаптер самого wgpu.
  Это НЕ Mesa/opengl32.dll, а часть wgpu. Требует проверки, что метод реально
  работает на используемой версии wgpu и на всех backend'ах.
- **(B)** Явная понятная ошибка пользователю вместо молчаливой деградации.
- **(C)** Комбинация: сначала (A), при её отказе — (B).

Риск бездействия: на VM, в RDP-сессии или на старом железе без нормального
GPU-адаптера окно может вообще не создаться.

### 2. Судьба `FrontEndSelection::Software`

**Проверено лично:** это не независимый рендерер, а именно OpenGL-режим.
`crates/window/src/configuration.rs:1-12` (`prefer_swrast()`) возвращает `true`
при `front_end == Software` либо при RDP-сессии на Windows. Единственные
потребители флага — `crates/window/src/egl/state.rs:72,75,116,119` и
`crates/window/src/os/windows/wgl.rs:123`; оба грузят Mesa
(`assets/windows/mesa/opengl32.dll`, 37 МБ) как обычный GL-контекст.

Значит удаление GL убивает `Software` как режим. Варианты: удалить понятие
целиком, либо переопределить его поверх wgpu software-adapter (см. вопрос 1).

Отдельно важно: RDP-ветка `prefer_swrast()` существует потому, что
«Using OpenGL in RDP has problematic behavior upon disconnect». После удаления
GL эта проблема исчезает вместе с причиной, но поведение в RDP надо проверить.

## Категория (а): удалить целиком

| Файл | Строк | Содержимое |
|---|---|---|
| `crates/window/src/egl/mod.rs` | 15 | фасад EGL |
| `crates/window/src/egl/state.rs` | 379 | `GlState`, загрузка libEGL, Mesa/RDP-детект |
| `crates/window/src/egl/wrapper.rs` | 327 | обёртка над EGL FFI |
| `crates/window/src/egl/connection.rs` | 35 | `GlConnection`; поля `gl_connection` в `os/*/connection.rs` тоже уходят |
| `crates/window/src/egl/ffi.rs` | 33 | сгенерированные биндинги |
| `crates/window/src/os/windows/wgl.rs` | 642 | WGL-бэкенд Windows |
| `crates/window/examples/async.rs` | 151 | пример целиком на `enable_opengl` + `glium::Frame` |
| `crates/onlyterm-gui/src/uniforms.rs` | 57 | glium-специфичные uniform'ы |
| `crates/onlyterm-gui/src/glyph-frag.glsl` | 161 | GL-шейдер |
| `crates/onlyterm-gui/src/glyph-vertex.glsl` | 38 | GL-шейдер |
| `docs/config/reference/config/prefer_egl.md` | 20 | документация опции |

Фрагменты внутри общих файлов:

- `RenderState::compile_prog`, `RenderState::glyph_shader` — `renderstate.rs:641-682`
- `TermWindow::call_draw_glium` — `termwindow/render/draw.rs:119-241`
- `WindowOps::enable_opengl` / `finish_frame` — `window/src/lib.rs:262-267` и все
  реализации: `os/x_and_wayland.rs:227-238`, `os/macos/window.rs:724,1008,1768`,
  `os/wayland/window.rs:386,657`, `os/windows/window.rs:507`, `os/x11/window.rs:178,1985`
- `impl Texture2d for SrgbTexture2d` — `window/src/bitmaps/mod.rs:46-80`
- macOS модуль `cglbits` — `os/macos/window.rs:242+` (CGL-бэкенд, `NSOpenGLContext`)
- `window/build.rs` — генерация WGL/EGL биндингов
- `onlyterm-gui/build.rs:45-76` — копирование ANGLE и Mesa DLL
- `assets/windows/angle/` (5.1 МБ) и `assets/windows/mesa/` (37 МБ) — **проверено: 42 МБ суммарно**

## Категория (б): разделить (GL и WebGpu переплетены)

| Файл | Что переплетено | Как резать |
|---|---|---|
| `config/src/frontend.rs:3-9` | `enum FrontEndSelection`, `#[default] OpenGL` | убрать вариант; `#[default]` → `WebGpu`; **нужна миграция старых конфигов** с `front_end = "OpenGL"` (маппить с предупреждением, не hard error) |
| `config/src/config.rs:1352-1378` | `default_front_end()` с cfg-ветками + тест | ветки схлопываются в константу |
| `config/src/config.rs:624-629` | поле `prefer_egl` | удалить |
| `onlyterm-gui/src/renderstate.rs` (748) | `RenderContext/IndexBuffer/VertexBuffer/MappedVertexBuffer` — везде enum `{Glium, WebGpu}` | ~15 точек; после удаления enum'ы схлопываются в структуры. Трудоёмко, но низкорисково |
| `onlyterm-gui/src/quad.rs:44-46` | `implement_vertex!` | удалить только макрос |
| `onlyterm-gui/src/termwindow/render/draw.rs` | диспетчер `call_draw` | оставить только webgpu-ветку |
| `onlyterm-gui/src/termwindow/mod.rs` | `gl`, `opengl_fallback_relay`, `permanently_on_opengl` — чисто GL; `opengl_info` (339) — **неудачное имя**, хранит инфу любого бэкенда | первые три удалить; `opengl_info` переименовать в `renderer_info` |
| `onlyterm-gui/src/termwindow/render_pipeline.rs` | `new_window` (32-421), фолбэк (593-1158), `do_paint`/`do_paint_webgpu` (1364-1433) | **самый рискованный файл**; требует решения вопроса 1, а не механической правки |
| `onlyterm-gui/src/overlay/debug.rs:77-96`, `actions.rs:642` | параметр `opengl_info` | переименовать вслед за полем |
| `window/src/os/x11/window.rs` | `enable_opengl` + комментарии 1621-1635 | комментарии перечитать перед удалением — возможно описывают общее поведение закрытия окна |
| `window/src/os/wayland/window.rs` | поля `wegl_surface`/`gl_state`, ресайз 958-967 | вырезать только GL-ветку внутри общего обработчика ресайза |
| `window/src/os/macos/window.rs` | `BackendImpl::{Cgl,Egl}`, `GlContextPair`, тройная делегация `enable_opengl` | **самая рискованная платформа**: Drop-порядок GL-ресурсов уже имеет задокументированный SIGABRT-риск (см. `render_pipeline.rs:1174-1179`) |
| `window/src/os/windows/window.rs:507-556` | `enable_opengl` (выбор EGL/WGL по `prefer_egl`) | удаляется вместе с вызывающим кодом |

## Категория (в): обновить только текст

- `config/src/config.rs:1366-1369` — doc-комментарий теста про «OpenGL fallback»
- `onlyterm-gui/src/overlay/debug.rs:34-38` — doc-комментарий модуля
- `docs/config/reference/config/front_end.md` — переписать (56 строк)
- `webgpu_*.md` — **проверено: GL-упоминаний нет, менять не нужно**
- `termwindow/webgpu/state_impl.rs:510-517` — **не удалять бездумно**: объясняет,
  зачем в `shader.wgsl` есть свой `to_srgb()` (сравнение с поведением glium-блендинга)

## Зависимости

- `glium 0.35` объявлена в корневом `Cargo.toml:106`, используется только крейтом
  `window`; `onlyterm-gui` обращается через ре-экспорт `::window::glium::*`.
- **Важно:** `gl_generator`, `glow`, `glutin_wgl_sys` останутся в `Cargo.lock`
  после удаления нашего GL-кода — их тянет сам `wgpu-hal` как опциональный бэкенд.
  Это чужая транзитивная зависимость, трогать не нужно.
- Реально уйдёт из lock-файла только сама запись `glium`.

## Оценка объёма

- Категория (а): **~2700–2900 строк** + 42 МБ бинарных ассетов
- Категория (б): **~1500–2000 строк** правок (рефакторинг, не удаление)
- Крейты: `config`, `window` (основной объём), `onlyterm-gui`, `Cargo.toml/lock`, `docs`
- Риск платформ по убыванию: **macOS** > **Wayland** > **X11** > **Windows**
  (на Windows GL хорошо изолирован, но там живёт вся фолбэк-подсистема)

## Про задачу #372 (не трогать без разрешения)

Падение происходит при **обратном** переключении: пользователь явно указал
`front_end: OpenGL`, затем что-то возвращает на WebGpu. Это не автоматический
фолбэк WebGpu→OpenGL, а ручной путь OpenGL→WebGpu. После удаления GL состояние
«front_end: OpenGL» станет недостижимым, то есть баг станет невоспроизводимым —
но **причина останется неустановленной**. Формально задача потеряет актуальность
как сценарий, но не как урок о переключении рендер-контекстов.

## Открытые вопросы для исполнителя #413–#416

1. `x11/window.rs:1621-1635` — перечитать комментарии перед удалением.
2. Работоспособность WebGpu на X11/Wayland/macOS подтверждена **только структурно**
   (наличие `HasWindowHandle`/`HasDisplayHandle`, отсутствие cfg-ограничений в
   `WebGpuState::new_impl`), эмпирически не проверялась — GUI не запускался.
3. Решения по вопросам 1 и 2 в начале документа — за пользователем.
4. `window/build.rs`: назначение `cargo:rustc-link-lib=framework=Carbon` для macOS
   не проверено, GL-специфичность не подтверждена — не удалять не глядя.
