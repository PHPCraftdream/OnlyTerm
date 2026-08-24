# Аудит горячих путей и аллокаций OnlyTerm

Дата: 2026-08-23
Режим: read-only аудит исходников; изменён только этот отчёт.

## Итог

Главный остаточный резерв находится в GUI paint/render path, а не в терминальном parser path. По имеющимся замерам:

| Участок | Наблюдение |
|---|---:|
| `perform_actions` | p95 около 0,156 мс |
| `render_screen_line` | p95 около 0,181 мс |
| Rustybuzz shaping | p95 около 0,118 мс |
| `gui.paint.impl` | p95 около 40 мс |

Недавние крупные аллокационные проблемы уже закрыты: wire-frame draw buffers и serialization body переиспользуются, child `gpu-tab-host` читает frame borrowed-срезами, а glyph cache использует instanced `QuadInstance` и быстрый внутренний hasher. Поэтому массовая замена всех контейнеров или числовых типов сейчас не оправдана.

## Приоритетные кандидаты

### 1. Перемещать `CellCluster`, а не клонировать

Файл: `crates/onlyterm-gui/src/termwindow/render/screen_line.rs:766-893`.

`build_line_element_shape` итерирует `&cell_clusters`, а затем делает `cluster.clone()`. `CellCluster` содержит `String` и индексные `Vec`, поэтому clone создаёт реальные heap-аллокации на каждый shape-cache miss.

Безопасный вариант: потреблять `cell_clusters` через `into_iter()` и перемещать кластер в `LineToElementShape`. Дополнительно заранее задать `Vec::with_capacity(cell_clusters.len())` для `shaped`.

Ожидаемый эффект: меньше копирования текста и индексных массивов при изменении строк. Риск низкий, но нужны тесты bidi, wide-глифов и ligatures.

### 2. Убрать временные `Vec` из frame signature

Файл: `crates/onlyterm-gui/src/termwindow/render/draw.rs:263-281`.

Каждый вызов `call_draw_webgpu` создаёт несколько временных коллекций:

- `Vec<Rc<RenderLayer>>`;
- `Vec<Ref<[TripleVertexBuffer; 3]>>`;
- `Vec<Ref<Vec<QuadInstance>>>`;
- `Vec<&[QuadInstance]>`.

Signature считается до `build_webgpu_frame`/`build_wire_frame`, поэтому можно сделать отдельный helper, который проходит по слоям и хеширует `vb.instances` непосредственно внутри borrow-области. Это уберёт несколько heap-аллокаций на каждый paint, сохранив текущую проверку идентичности frame.

Сам проход по `QuadInstance` останется O(N); оптимизировать его нужно отдельно, например через generation/dirty counter, только после измерения стоимости signature.

### 3. Упростить структуру `AtlasMirrorLog`

Файл: `crates/onlyterm-gpu-render/src/webgpu/mod.rs:271-320`.

Сейчас используются:

- `BTreeMap<AtlasRect, Arc<[u8]>>` — O(log N) на запись;
- `Vec<AtlasRect>::contains` — O(N) на проверку pending rect.

Atlas rectangles генерируются локальным allocator’ом, а порядок применения updates не имеет значения: rectangles не перекрываются. Поэтому стоит измерить замену на:

```text
HashMap<AtlasRect, Arc<[u8]>>
HashSet<AtlasRect>
```

Для этих доверенных числовых ключей допустим `ahash::RandomState`; фиксированный seed не нужен. Средняя сложность записи станет O(1), а проверка pending перестанет деградировать при большом количестве updates.

Это не является причиной прежнего OOM: его вызвали крупные непереиспользуемые wire allocations, уже устранённые pooling-патчами. Это отдельная оптимизация host-process atlas path.

### 4. Immutable snapshots строк с переиспользованием по seqno

Файлы: `crates/term/src/screen.rs:565`, `crates/mux/src/pane.rs:591`, GUI вызов из `crates/onlyterm-gui/src/termwindow/render/pane.rs:860`.

Сейчас видимые строки глубоко клонируются при каждом paint, чтобы отпустить terminal mutex до shaping/render. Это правильно с точки зрения блокировок, но дорого по памяти.

Безопасный дизайн — не оборачивать мутируемый `Line` в общий `Cow<Line>`, а ввести immutable renderer snapshot, кешируемый по `(pane_id, row, seqno)` и возвращаемый как `Arc<LineSnapshot>`:

- неизменившаяся строка переиспользует уже созданный snapshot;
- изменившаяся строка копируется один раз;
- snapshot содержит только данные, необходимые renderer’у.

Полный COW mutable `Line` опасен: parser часто мутирует текущую строку, и стоимость copy-on-write может просто переместиться в parser path. Это более крупный проект с повышенным риском для reflow, bidi и cursor overlays.

### 5. Уменьшить аллокации Rustybuzz shaping

Файл: `crates/onlyterm-font/src/shaper/rustybuzz.rs:606-683, 958-1034`.

Текущая реализация создаёт `Vec<Vec<Info>>`, то есть потенциально отдельную heap-аллокацию на cluster. Дополнительные `HashMap` создаются в `ClusterResolver` на каждый shape invocation.

Кандидаты для эксперимента:

- плоский `Vec<Info>` плюс диапазоны cluster’ов;
- `SmallVec<[Info; 1]>`, если реальные размеры подтверждают выигрыш;
- `with_capacity(rb_infos.len())` вместо `s.len()`;
- переиспользование resolver storage.

Нельзя без тестов заменять resolver maps на линейные Vec: RTL и reordered clusters уже требуют сортировки по byte position. Shaping имеет меньший абсолютный бюджет, поэтому это второй эшелон после paint allocations.

### 6. Scratch pool всё ещё делает полную копию

Файл: `crates/onlyterm-gui/src/renderstate.rs:359-381`.

`scratch_pool` убирает новые allocations, но `accumulate_instances` копирует весь scratch `Vec<QuadInstance>` в accumulator. Можно исследовать chunked/page storage или прямой append с явной поддержкой вложенных `with_quad_allocator` вызовов.

Это сложнее и требует измерять `gui.paint.collect`, объём копируемых bytes и поведение reentrant UI painting. Простая отмена scratch pool будет регрессией.

## Низкоприоритетные кандидаты

- `crates/mux/src/lib.rs:508-520`: `snapshot` и `dead` в `Mux::notify` можно перевести на `SmallVec`; integer-key map с AHash проверять только после измерения. Lock/queue semantics важнее hasher’а.
- `crates/onlyterm-gui/src/termwindow/render/pane.rs:550`: `Value::String("password_input".to_string())` создаётся для cursor row; нужен typed metadata accessor или переиспользуемый ключ.
- `crates/onlyterm-gui/src/termwindow/render/mod.rs:1196`: basename foreground process всё ещё материализуется через `into_owned`, но после hoist это одна аллокация на pane/frame, а не на каждую строку.
- `build_webgpu_frame`/`build_wire_frame` создают маленький внешний `Vec<GpuDraw`; `SmallVec` возможен, но ожидаемый выигрыш ниже, чем у устранения line clone и signature vectors.

## Хешеры и контейнеры: что не менять вслепую

- Glyph cache, line/block glyph caches уже используют AHash.
- Generic `LfuCache` уже использует AHash, а `LfuCacheU64` — специализированный FNV.
- Строковые и внешние ключи следует оставлять на защищённом стандартном hasher’е, если нет доказанного ограничения входа.
- `COLOR_SCHEMES` и большая часть Mux registry maps относятся к startup/control path, а не к steady-state rendering.
- `shape_plans` и metrics cache маленькие и read-mostly; сначала нужны реальные miss/hit и allocation counters.

## Порядок проверки

1. Добавить/собрать метрики для frame signature, `gui.paint.collect`, количества cloned lines и bytes cloned.
2. Добавить counters для `AtlasMirrorLog.record`, размера `pending` и числа unique rectangles.
3. Запустить существующие release benchmarks: `perf_probe`, glyph HashMap/AHash benchmark и LFU/LRU benchmark.
4. Сравнить static screen, flood output, scrollback, bidi/RTL и host-process full-resync.
5. После каждого изменения проверять retained rows, ghost-cursor, atlas resync и wire round-trip tests.

Итого: первые практические эксперименты — move вместо `CellCluster::clone`, устранение временных Vec в frame signature и HashMap/HashSet для atlas pending. COW snapshots строк могут дать самый большой эффект по памяти, но их следует проектировать отдельно и только после измерения текущего clone volume.
