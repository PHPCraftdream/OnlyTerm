# План: выпилить cairo/pixman (C + asm) → tiny-skia (чистый Rust)

Дата: 2026-07-23. Репо: форк `PHPCraftdream/onlyterm`, ветка от `main` (76b606ec5).

## Цель

Удалить `deps/cairo` целиком (~170k строк C cairo + ~52k строк C/asm pixman, из них
151 `.c`-файл реально компилируется через `build.rs` + `cc`), заменив единственного
потребителя — рендер цветных COLR/COLRv1-глифов в `onlyterm-font` — на `tiny-skia`
(чистый Rust). После этого в проекте не останется вендоренного C-кода cairo/pixman
и ассемблера (ARM/MIPS `.S`-файлы и так не компилировались на нашем таргете).

## Результаты исследования (зафиксировано)

### Потребители cairo (весь workspace, вне deps/)

Только 5 файлов, все в `onlyterm-font`:

| Файл | Что использует |
|---|---|
| `src/rasterizer/colr.rs` | Типы `PaintOp`/`DrawOp`/`ColorLine` (общие для обоих путей), `LinearGradient`, `RadialGradient`, `Mesh`/`MeshCorner` (ручная тесселяция sweep-градиента — workaround отсутствия conic-градиента в cairo), `apply_draw_ops_to_context` |
| `src/rasterizer/freetype.rs` | Путь FreeType COLR: `RecordingSurface` + `ink_extents()` → `ImageSurface` (ARgb32) → `take_data()`; state stack (`save`/`restore`), `push_group`/`pop_group_to_source` + `set_operator`, клипы по путям |
| `src/rasterizer/harfbuzz.rs` | Путь HarfBuzz paint: то же самое + `SurfacePattern` (Extend::Pad) для встроенных PNG-слоёв, rect-клипы, slant-трансформация |
| `src/ftwrap.rs` | `composite_mode_to_operator`: FT_Composite_Mode → `cairo::Operator` (29 вариантов) |
| `src/hbwrap.rs` | `hb_extend_to_cairo`: hb_paint_extend_t → `cairo::Extend` |

Терминальный рендер (wgpu/OpenGL) cairo не трогает вообще. Задача изолирована.

### Используемые фичи cairo → покрытие tiny-skia

| Нужно | tiny-skia | Примечание |
|---|---|---|
| Linear/Radial градиент + Extend (Pad/Repeat/Reflect) | ✅ `LinearGradient`/`RadialGradient` + `SpreadMode` | 1:1 |
| Sweep-градиент | ❌ **Поправка (найдено при реализации фазы C, 2026-07-23): раннее исследование через WebFetch было ошибочным** — у tiny-skia НЕТ нативного sweep/conic градиента ни в 0.11, ни в 0.12 (`Shader` — только Solid/Linear/Radial/Pattern). Реализовано вручную через точный per-pixel расчёт угла (`Painter::paint_with(FnMut(f32,f32)->Color)`, добавлен в paint.rs) — без тесселяции в меш, точнее прежнего cairo-пути | Весь mesh-код в `colr.rs` (~300 строк, Patch/add_sweep_gradient_patches/apply_sweep_gradient_patches) удалён, заменён точным per-pixel расчётом, не тесселяцией |
| Радиальный градиент с r0≠0 (двухрадиусный конический) | ❌ tiny-skia `RadialGradient::new` — только одноцентровый (r0=0). COLRv1-шрифты реально используют r0≠0 | Реализовано вручную: классическое квадратное уравнение two-point-conical (та же математика, что в Skia/cairo), через тот же `Painter::paint_with` |
| 29 операторов Porter-Duff/CSS blend | ✅ `BlendMode` — ровно те же 29 вариантов, включая Hue/Saturation/Color/Luminosity | 1:1 маппинг Operator→BlendMode |
| SurfacePattern (PNG-слои) | ✅ `Pattern::new(pixmap, spread_mode, quality, opacity, transform)` | 1:1 |
| Пути: move/line/curve/quad/close | ✅ `PathBuilder` (quad_to есть нативно — cairo-шный костыль quad→cubic в `apply_draw_ops_to_context` можно убрать) | Упрощение |
| `RecordingSurface` + `ink_extents()` | ❌ нет | Пишем сами: двухпроходный рендер — 1-й проход считает bbox по ops (см. ниже), 2-й растрит в Pixmap нужного размера |
| `save`/`restore` (стек состояний) | ❌ нет | Пишем сами: явный стек (transform, clip) |
| `push_group`/`pop_group_to_source` + operator | ❌ нет | Пишем сами: группа = отдельный Pixmap, композит через `PixmapPaint { blend_mode }` |
| Клип по произвольному пути (anti-aliased) | ✅ `Mask` + `fill_path` в маску; пересечение клипов = intersect масок | Чуть больше кода, чем в cairo, но прямолинейно |
| ARgb32 (premultiplied) → RGBA | ✅ Pixmap хранит premultiplied RGBA | Конверсия argb_to_rgba упрощается/меняется на demultiply при необходимости (RasterizedGlyph ожидает текущий формат — проверить, что именно ждёт glyph cache: сейчас на выходе argb_to_rgba даёт RGBA premultiplied) |

### Тонкие места (не забыть при реализации)

1. **Вычисление ink extents без RecordingSurface.** В cairo bbox определяется
   фактически нарисованным. У нас: bbox = объединение по всем paint-операциям
   пересечения (текущий клип-стек ∩ покрытие источника). Для `PaintSolid`/градиентов
   покрытие бесконечно ⇒ вклад = bbox текущего клип-стека. Клипы — это пути;
   bbox пути консервативно = bbox контрольных точек (Безье лежит в выпуклой
   оболочке контрольных точек). Всё под текущей матрицей трансформации.
   `paint_sweep_gradient` использует `clip_extents()` для радиуса — при нативном
   SweepGradient это не нужно, но радиус-подобная логика уйдёт, проверить.
2. **Матрицы.** cairo `Matrix::new(xx, yx, xy, yy, x0, y0)` ↔ tiny-skia
   `Transform::from_row(sx, ky, kx, sy, tx, ty)`. Внимание на порядок аргументов.
   ⚠ В `freetype.rs:790-798` `affine2x3_to_matrix` передаёт `t.dy, t.dx` (в этом
   порядке!) в позиции (x0, y0) — проверить, это баг апстрима или намеренно
   (FT_Affine23 документирован как 2x3 матрица с dx/dy). Воспроизвести поведение
   1:1 при портировании, отдельно пометить для проверки.
3. **Y-flip и масштаб 1/64.** HarfBuzz-путь: `context.scale(1/64, -1/64)` (font units
   26.6 fixed point, ось Y перевёрнута). FreeType-путь: `scale(scale_x, -scale_y)`.
   В tiny-skia — pre-concat той же трансформации в корне стека.
4. **Группы и клипы вложены произвольно** (PushGroup внутри клипа внутри
   трансформации). Стек состояний должен сохранять/восстанавливать И трансформацию,
   И клип-маску; группа рендерится в отдельный Pixmap того же размера, затем
   композитится целиком с BlendMode поверх родителя с учётом родительской клип-маски.
5. **`is_foreground`/цвет 0xffffffff** — логика `has_color` (глиф монохромный, если
   красился только белым) должна сохраниться бит-в-бит: от неё зависит, будет ли
   глиф перекрашиваться цветом текста.
6. **Antialias::Best** — в tiny-skia анти-алиасинг включается на `Paint { anti_alias: true }`,
   отдельного уровня Best нет; ожидаемое качество сглаживания может чуть отличаться —
   принять как допустимое отличие, проверить визуально.
7. **`cairo-rs` подключён с `default-features=false`** — фичи `win32-surface`/`pdf`/`svg`/
   `ps`/`freetype` НЕ активны (уточнение к прошлой сессии: `win32-surface` объявлен
   в vendored Cargo.toml, но никем не включается). Терять нечего.
8. **SIMD.** `build.rs` не компилирует ни `.S` (ARM/MIPS), ни `pixman-sse2.c`/
   `pixman-ssse3.c`/`pixman-mmx.c` — сейчас работает скалярный C. tiny-skia со своими
   Rust-SIMD путями не медленнее. Плюс это только glyph-растеризация с кэшем —
   не hot loop кадра.

### Инструменты для безручной (agent-driven) верификации

Задача полностью изолирована в `onlyterm-font` и не трогает GUI/рендер терминала —
поэтому никакого управления живым UI/скриншотов окна не требуется вообще. Нужны
только headless-инструменты уровня крейта, чтобы agent мог сам снять и сравнить
результат растеризации без участия пользователя:

- Тестовый шрифт уже в репо: `assets/fonts/NotoColorEmoji.ttf` — проверено через
  `grep -a -o "COLR" ...` — это современный градиентный **COLRv1**-шрифт (не
  битмап CBDT), то есть покрывает ровно то, что мигрируем (линейные/радиальные/
  sweep-градиенты, группы/композитинг). Отдельно искать/скачивать тестовые шрифты
  не нужно.
- **Сабмодули не инициализированы** (`deps/freetype/freetype2`, `deps/freetype/libpng`,
  `deps/freetype/zlib`, `deps/harfbuzz/harfbuzz`) — это блокирует вообще любую сборку
  `onlyterm-font`, значит `git submodule update --init --recursive` должен быть сделан
  раньше даже задачи A.
- **`onlyterm-font/examples/dump_glyph.rs`** (новый) — headless-бинарник: путь к шрифту
  + codepoint/glyph-id + size/dpi + выбор растеризатора (freetype/harfbuzz/оба) →
  вызывает `rasterize_glyph`, пишет PNG (`image` crate уже есть в зависимостях) плюс
  JSON-сайдкар с width/height/bearing_x/bearing_y/has_color. Один и тот же бинарник
  используется и в фазе A (эталон на cairo), и в фазе G (результат на tiny-skia).
- **`onlyterm-font/examples/diff_glyph.rs`** (новый) — headless сравнение двух PNG:
  per-pixel/per-channel diff, доля различающихся пикселей выше порога, exit-code
  ненулевой при превышении допуска. Позволяет G пройти полностью автоматически
  (скрипт/agent), без визуального разглядывания человеком.
- Для необязательного финального sanity-прогона в живом терминале (см. риски) —
  уже существующие в системе skills `/run` и `/verify` умеют сами запускать и
  скриншотить приложение; отдельная инфраструктура под это не нужна.

### Порядок работ

Фазы линейны (каждая следующая зависит от предыдущей), кроме A/B — их можно вести параллельно.

- **0. Тулинг (см. выше).** Инициализировать сабмодули, написать
  `dump_glyph`/`diff_glyph`, прогнать на текущем (cairo) билде как smoke-test
  самого инструмента.
- **A. Baseline (до любых изменений).** `dump_glyph` по референсным глифам
  (эмодзи/COLRv1-градиенты из `NotoColorEmoji.ttf`, оба растеризатора) → PNG-файлы —
  эталон для сравнения после миграции.
- **B. Каркас painter-модуля.** Новый `onlyterm-font/src/rasterizer/paint.rs`:
  структура `Recorder`/`Painter` на tiny-skia — стек состояний (Transform + Mask),
  группы (Pixmap + BlendMode), двухпроходный bbox+растр, конверсия в RasterizedGlyph.
  Юнит-тесты на bbox и стек.
- **C. Порт `colr.rs`.** Типы PaintOp/DrawOp/ColorLine перевести с cairo-типов на
  свои/tiny-skia (Extend→SpreadMode, Matrix→Transform, Operator→BlendMode).
  Удалить mesh-код sweep-градиента, заменить на нативный SweepGradient.
- **D. Порт `harfbuzz.rs` растеризатора** на painter из B (включая PaintImage/PNG
  через Pattern). Обновить `hbwrap.rs` (hb_extend → SpreadMode, PopGroup mode →
  BlendMode).
- **E. Порт `freetype.rs` растеризатора** на painter из B. Обновить `ftwrap.rs`
  (composite_mode_to_operator → BlendMode).
- **F. Вычистка.** Убрать `cairo-rs` из `onlyterm-font/Cargo.toml`, `cairo-rs`/
  `cairo-sys-rs`/patch-секцию из корневого `Cargo.toml`, `deps/cairo` из
  `workspace.members`, удалить каталог `deps/cairo/`. Добавить `tiny-skia` в
  workspace-зависимости. `cargo build` всего workspace на Windows.
- **G. Верификация.** Сравнить рендер референсных глифов с эталоном из A
  (допуск на субпиксельные отличия анти-алиасинга), `onlyterm ls-fonts`,
  визуальная проверка эмодзи в живом терминале. `cargo test -p onlyterm-font`.

## Риски

- Пиксельные отличия анти-алиасинга/градиентов от cairo — ожидаемы, критерий:
  визуально неотличимо на реальных эмодзи.
- HSL blend modes у tiny-skia могут численно отличаться от cairo — проверить на
  глифах, реально использующих Hue/Saturation/Color/Luminosity (редкие).
- `affine2x3_to_matrix` dy/dx (см. тонкое место 2) — не «чинить» молча при порте.
- tiny-skia не поддерживает f64 (всё f32) — для глифовых размеров это не проблема,
  но конверсии из f64-интерфейсов делать явно.
