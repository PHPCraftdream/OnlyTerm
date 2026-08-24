# Аудит производительности OnlyTerm

Дата: 2026-08-07. Режим исследования: read-only для исходников; добавлен только этот отчёт.

Состояние репозитория при исследовании: `bf233a8a2` плюс незакоммиченные пользовательские
изменения. Поэтому наблюдения о текущем рабочем дереве, особенно о новом process-aware bidi,
нужно оценивать как замечания к незавершённой работе, а не как регрессию уже выпущенной версии.

## Короткий итог

Главный следующий выигрыш находится не в замене `u64` на `u32` и не в terminal mutex.
Под длительным выводом GUI накапливает долг в очереди main-thread executor: одно событие
`PaneOutput` проходит до трёх отдельных итераций очереди, а Windows намеренно выполняет только
одну такую задачу за оборот message pump. В живом прогоне задержка выполнения задач выросла до
7.35 с, хотя ожидание terminal mutex оставалось на уровне микросекунд, а применение parser
actions — около 0.1–0.16 мс.

Приоритеты:

1. **P0 — сделать `PaneOutput` level-triggered и коалесцировать его до конечного
   `InvalidateRect`, а не только до первого mux callback.** Это должно убрать секунды долга GUI
   при активном выводе, не возвращая прежнее голодание Win32 message pump.
2. **P1 — разделить WebGpu на общий для процесса `GpuContext` и per-window `SurfaceState`.**
   Сейчас каждое окно заново создаёт `Instance`, выбирает adapter, запрашивает device и строит
   pipeline. Это повторяет самую дорогую часть запуска.
3. **P1 — перейти от четырёх полных вершин на quad к instanced quads и перестать создавать
   новый mapped GPU buffer полной capacity каждый кадр.** Только представление данных уменьшает
   vertex payload с 272 до 84 байт на quad, то есть на 69%.
4. **P2 — убрать аллокацию `Box` на каждый cached quad, измерить LFU против обычного LRU и
   ввести seqno-based immutable snapshots строк.** Это хорошие кандидаты, но они идут после
   очереди уведомлений и GPU upload.
5. **Не менять вслепую `Cell`, `CellAttributes`, `TeenyString`, `VecDeque<Line>` и целочисленные
   типы.** Эти представления уже компактны или соответствуют операции; без профиля такая замена
   даст риск и почти наверняка не даст заметного ускорения.

## Что измерено сейчас

Использовался текущий `target/release/onlyterm-gui.exe` (`onlyterm v0.0.2-alpha-dirty`), окно
80×24 и периодический вывод внутренних metrics. Были сделаны два прогона: короткий burst на
50 000 строк и 15-секундный прогон, в котором после четырёх секунд покоя выводились 100 000
строк. Второй прогон важнее: renderer уже был готов до начала потока, поэтому он показывает не
только cold first paint.

| Метрика во время длительного вывода | Наблюдение |
|---|---:|
| Скорость входа parser | около 0.60 МБ/с |
| Parser flush / `send_actions` | около 315/с |
| `perform_actions` | p95 около 0.156 мс |
| Ожидание terminal mutex | обычно 1–2 мкс |
| `executor.spawn_delay` | p50 3.74 с, p95 7.35 с |
| Частота фактических paint во время flood | примерно 1–3 кадра/с |
| `gui.paint.impl` | p50 19.53 мс, p95 40.63 мс |
| `paint_pane.lines` | p50 7.37 мс, p95 18.74 мс |
| `render_screen_line` | p50 0.133 мс, p95 0.181 мс |
| Rustybuzz shaping | p50 0.084 мс, p95 0.118 мс |
| Vertex buffer recreate | p50 1.18 мс, p75 4.65 мс, p95 23.07 мс |
| Размер recreating vertex buffer | p50 114 КБ, p75 1.22 МБ, p95 1.29 МБ |
| Render-thread submit/present | p50 23.72 мс, p95 32.11 мс |
| `HeapQuadAllocator::apply_to` | p50 1.9 мкс, p95 3 мкс |

Рабочий набор процесса дошёл примерно до 574 МБ, private bytes — до 658 МБ. Эти числа нельзя
сразу приписывать Rust-объектам: сюда входят wgpu/DX12 allocations, mapped buffers и политика
глобального allocator. Для решений по памяти нужен отдельный DHAT + GPU/ETW прогон. По прежнему
решению проекта `sefer-alloc` в этом аудите не рассматривается как кандидат на замену.

## P0: долг main-thread очереди при выводе

Текущий путь одного уведомления выглядит так:

```text
parser thread
  -> spawn #1: Mux::notify на main thread
  -> spawn #2: subscribe_to_pane_updates callback
  -> spawn #3: Window::notify / Connection::with_window_inner
  -> TermWindowNotif::MuxNotification
  -> is_pane_visible
  -> InvalidateRect
```

Это не предположение: три hop прямо описаны в комментарии
`crates/mux/src/lib.rs:111-129` и реализованы в:

- `Mux::dispatch_notification`, `crates/mux/src/lib.rs:571-583`;
- `TermWindow::subscribe_to_pane_updates`,
  `crates/onlyterm-gui/src/termwindow/render_pipeline.rs:1636-1654`;
- `Window::notify` и безусловном `Connection::with_window_inner`,
  `crates/window/src/os/windows/window.rs:1891-1902` и
  `crates/window/src/os/windows/connection.rs:191-215`.

Коммит `d278142de9` правильно изменил Windows `SpawnQueue`: теперь он исполняет одну задачу за
итерацию и возвращается к `PeekMessageW`/`DispatchMessageW`. Откатывать это нельзя: прежний
полный drain мог навсегда заморозить ввод и paint при непрерывном producer.

Коммит `47eaeb91f` добавил полезную коалесценцию `PaneOutput`, но pending marker очищается в
самом начале `Mux::notify` (`crates/mux/src/lib.rs:475-482`). В этот момент впереди ещё два
queue hop. Parser сразу может поставить следующий spawn #1; при примерно 315 flush/с producer
снова обгоняет consumer. Именно это согласуется одновременно с секундным `spawn_delay`, редкими
paint и микросекундным terminal lock.

### Рекомендуемая форма исправления

`PaneOutput` должен быть не очередью событий, а состоянием «pane изменился». Нужна одна
pending/generation запись на pane или на окно, которая остаётся активной **до обработки
конечного window notification/invalidation**. Если во время обработки пришёл новый вывод,
generation меняется и после завершения планируется ровно ещё одна доставка.

Минимальная последовательность эксперимента:

1. Убрать spawn #2: `Mux::notify` уже вызывается на main thread и перед callback больше не
   держит `subscribers` lock, поэтому subscriber можно вызвать inline.
2. Для `PaneOutput` не использовать общий всегда-отложенный `Window::notify`. Нужен отдельный
   main-thread-safe путь, который либо выполняет TermWindow callback inline вне оконного event
   dispatch, либо имеет собственный coalesced `TermWindowNotif` с флагом, очищаемым только в
   конце dispatch.
3. Защититься от гонки поколением, а не простым `bool`: output между последней проверкой и
   очисткой не должен потеряться.
4. Добавить counters `spawn_queue.depth`, `pane_output.coalesced`, `pane_output.end_to_end` и
   regression test с непрерывным producer. Условия успеха: queue depth ограничен, input/paint
   продолжают обслуживаться, а финальное содержимое pane не теряется.

Не стоит начинать с «исполнять больше задач за pump». После устранения producer amplification
можно испытать небольшой временной budget drain, но это вторичная настройка и она снова несёт
риск starvation OS messages.

## P1: общий GPU context и политика отдельных процессов

`WebGpuState::new_impl` на каждом создании окна выполняет:

- `wgpu::Instance::new`;
- `request_adapter` (на измеренной Intel UHD DX12 это около 2.0 с);
- `request_device`;
- создание shader, bind-group layouts и render pipeline;
- создание per-window surface и ресурсов.

Код находится в `crates/onlyterm-gui/src/termwindow/webgpu/state_impl.rs:183-478` и
`:621-692`. Pipeline создаётся с `cache: None`.

Полезное разделение типов:

```text
ProcessGpuContext
  Instance + Adapter + Device + Queue
  shader/layouts + pipeline(s) + optional persistent pipeline cache

WindowGpuSurface
  Surface + Dimensions + surface configuration + HWND
```

Это не только ускорение типа: это устранение повторной инициализации драйвера. Второе окно в
том же процессе сможет платить в основном за surface/configure и per-window atlas, а rebuild
не обязан заново перечислять adapter и создавать device.

Риски, которые должны быть частью дизайна:

- device-lost становится общепроцессным событием, а не проблемой одного окна;
- окна на GPU разных мониторов могут потребовать другой adapter;
- параллельные render threads будут делить `Queue`; нужно подтвердить порядок submit и
  отсутствие лишней сериализации;
- pipeline зависит от surface format, поэтому практически это cache по format, а не один
  безусловный объект.

Для первого окна общий context можно начать готовить раньше и параллельно с созданием mux/pane:
сам adapter выбирается с `compatible_surface: None`, то есть текущий код уже доказал, что для
дорогой части HWND не нужен. Persistent wgpu pipeline cache тоже стоит испытать, но он ускорит
компиляцию pipeline, а не измеренные две секунды `request_adapter`; сначала нужна отдельная
разбивка `request_device`/shader/pipeline.

Есть важная продуктовая граница. Коммит `b03646222` намеренно отключил делегирование запуска
уже работающему GUI: каждый обычный `start` теперь всегда отдельный процесс. Поэтому общий GPU
context ускорит окна внутри процесса, но **не** второй запуск из Start Menu. Если приоритетом
станет максимальная скорость повторного запуска, самый сильный вариант — вернуть delegation,
но дополнить handshake точным build fingerprint/mtime/hash executable. Тогда новый бинарник не
будет молча делегировать старому процессу — именно эта путаница была причиной `b03646222` — а
идентичная уже работающая версия сможет создать окно без полного cold process startup.

## P1: тип GPU-данных — instanced quads

Сейчас `Vertex` — 17 `f32`, 68 байт. На quad записываются четыре почти одинаковые вершины:
272 байта (`crates/onlyterm-gui/src/quad.rs:27-58`, size assertion на `:403-407`). Цвета, HSV,
flags и большая часть геометрии повторяются четыре раза.

`BoxedQuad` уже показывает естественное instance-представление: 84 байта уникальных данных на
quad. Статические четыре corner-вершины плюс один `QuadInstance` дают:

- 272 → 84 байта на quad;
- уменьшение CPU→GPU payload на 69.1% (3.24 раза);
- один общий набор из шести индексов вместо per-capacity index array;
- меньше CPU stores при построении кадра.

После этого имеет смысл заменить per-frame `create_buffer(mapped_at_creation=true)` в
`WebGpuVertexBuffer::recreate` (`crates/onlyterm-gui/src/renderstate.rs:136-145`) на persistent
`VERTEX | COPY_DST` buffer с `Queue::write_buffer`, staging belt либо небольшой ring. Записывать
нужно только реально использованный prefix, не всю capacity. Single-slot frame backpressure в
`RenderThreadHandle::send_frame` уже не даёт расти очереди кадров; её сохраняем.

Живые p75/p95 recreate 4.65/23.07 мс делают этот пункт серьёзным, хотя обычный p50 1.18 мс.
Нужно повторить замер с большим окном и отдельно считать `used_bytes` против `capacity_bytes`.

## P2: копирование строк без возврата длинного terminal lock

Коммит `6aab9a859` правильно сократил время terminal lock: render забирает snapshot видимых
строк, отпускает lock и только потом shape/render. Но snapshot является глубоким clone:

- `impl_get_lines_via_with_lines` вызывает `line.clone()` для каждой строки,
  `crates/mux/src/pane.rs:562-586`;
- `Line::clone` клонирует `CellStorage` и zones,
  `crates/onlyterm-surface/src/line/line.rs:61-71`;
- `VecStorage` содержит обычный `Vec<Cell>`, а `ClusteredLine` — `String` и `Vec<Cluster>`.

Возвращаться к shaping под terminal mutex не нужно. Более безопасная цель — immutable render
snapshot, кешированный по `Line::current_seqno()`:

- для неизменившейся строки возвращать `Arc<LineSnapshot>`;
- при новом seqno клонировать/собирать snapshot один раз;
- snapshot может хранить только нужные renderer данные, а не весь mutable `Line`;
- dirty lines при flood всё равно копируются, стабильные строки и повторные paint — нет.

До реализации добавить метрики времени `get_lines`, числа cloned lines и приблизительных
cloned bytes с разделением `VecStorage`/`ClusteredLine`. COW всего `Line` вслепую опасен: terminal
часто мутирует текущую строку, и copy-on-write может просто перенести дорогой clone в parser.

## P2: конкретные замены контейнеров

### `Vec<Box<BoxedQuad>>` → contiguous или chunked storage

`HeapQuadAllocator` хранит три `Vec<Box<BoxedQuad>>` и делает отдельный heap allocation на каждый
quad (`crates/onlyterm-gui/src/quad.rs:304-378`). Комментарий объясняет это страхом перед
многомегабайтным contiguous realloc, но текущий allocator создаётся **на одну строку** в
`render/pane.rs:537`, затем кладётся в line cache. Для 80 обычных quad один слой занимает всего
около 6.7 КБ логических данных; даже несколько quad на cell — это десятки КБ, не мегабайты.

Первый benchmark-кандидат — `Vec<BoxedQuad>` с разумным `reserve`; если большие строки дают
нежелательные realloc, использовать chunked arena (`Box<[BoxedQuad; N]>`). `apply_to` уже стоит
лишь 2–3 мкс на cache hit, поэтому измерять надо miss-build allocations и RSS, а не только
существующую apply metric.

### Custom `LfuCache` → измерить против `lru::LruCache`

Каждая LFU entry содержит `Rc`, три intrusive links, два `RefCell` и участвует одновременно в
hash buckets, recency list и RB-tree. На cache hit код двигает LRU node, обновляет tick/frequency
и remove/insert в RB-tree (`crates/lfucache/src/lib.rs:237-283`). Для renderer cache с тысячей
элементов это заметно сложнее обычного O(1) LRU.

Во время flood caches в основном промахивались: например line quad 2 hit / 88 miss, shape cache
около 172 miss без полезных hit в одном интервале. Здесь политика eviction почти не помогает;
на стабильном экране hit ratio, наоборот, важен. Поэтому не заменять LFU догматически, а сделать
microbenchmark реальных key/value и A/B прогон:

- hit latency;
- insert/evict latency;
- bytes/entry;
- итоговый hit ratio LFU и LRU на записанной последовательности обращений.

Особенно подозрителен `line_state_cache`: `Line` уже несёт weak appdata с seqno/hash, а LFU hit
нужен преимущественно для удержания strong `Arc`. Для него простой LRU либо специализированный
snapshot cache вероятнее всего выгоднее общего LFU.

### `std::HashMap` → `AHashMap` только для внутренних горячих caches

Glyph cache делает lookup на каждый glyph и пока использует стандартный `HashMap`. `ahash` уже
есть в workspace и используется LFU. A/B benchmark `AHashMap` оправдан для внутренних ключей
`GlyphKey`, `LineKey`, numeric pane/tab state. Глобально заменять все карты не следует: часть
строковых ключей приходит из внешнего terminal/config input, а ускорение cold/control-path maps
не будет видно. Приоритет ниже, чем layout quad и очередь событий.

### Что оставить как есть

- `Cell` — 24 байта, `CellAttributes` — 16, `TeenyString` — 8 с inline UTF-8 до семи байт.
  Редкие тяжёлые attributes уже вынесены отдельно.
- `Screen.lines: VecDeque<Line>` соответствует `push_back/pop_front` scrollback. Замена на `Vec`
  сделает удаление сверху линейным.
- `CellStorage` в 64 байта выглядит большим, но boxing `ClusteredLine` добавит allocation и
  pointer chasing на каждую строку. Это memory experiment, не очевидное ускорение.
- `usize` для индексов/capacity — нативный размер адреса. Переход на `u32/u16` создаст casts и
  проверки; делать его можно только внутри реально bandwidth-bound packed GPU/cache структуры.
- SipHash shape hash не вычисляется каждый кадр для стабильной строки: `shape_hash_for_line`
  кеширует его по seqno. Менять hasher до профиля не нужно; collision здесь способен вернуть
  неправильное shaping-содержимое.

## Маленькая находка в текущих незакоммиченных bidi-изменениях

`bidi_disabled_by_foreground_process` сейчас вызывается для каждой видимой строки при создании
quad key, а на cache miss ещё раз из `render_screen_line`. Каждый вызов получает process name,
разбирает `Path` и делает `to_string_lossy().into_owned()` для basename
(`crates/onlyterm-gui/src/termwindow/render/mod.rs:1042-1063`, вызовы в `render/pane.rs:461-465` и
`render/screen_line.rs:91-96`).

Результат одинаков для всего pane в одном кадре. Его лучше вычислить один раз до `LineRender`,
передать как `bool` в renderer/cache key и сравнивать basename как заимствованный `&str` через
`file_name().and_then(OsStr::to_str)`. Это убирает десятки process-cache lookups, path parses и
`String` allocations на кадр. Аналогично `Value::String("password_input".to_string())` создаётся
для cursor row каждый кадр; это меньшая, но легко измеримая аллокация.

## P3: cold startup до показа окна и release profile

Ранний show уже дал главный UX-результат: медиана появления окна улучшилась с 2909 до 260.6 мс.
В оставшихся примерно 260 мс есть ещё один структурный дубль: `cell_pixel_dims` создаёт
`FontConfiguration` и `RenderMetrics` до spawn pane (`main.rs:375-381`), а
`TermWindow::new_window` снова создаёт их (`render_pipeline.rs:44-61`). Коммит `6eccefb12` кеширует
font directory database, но per-configuration loaded fonts/metrics остаются отдельными.

Сначала нужно отдельно измерить обе конструкции. Если цена заметна, кешировать пару
`(config generation, DPI, font scale) -> loaded font data/RenderMetrics` либо передавать уже
полученный bootstrap result в создание окна. Полностью делить один mutable `FontConfiguration`
между окнами нельзя без учёта различного DPI и per-window font scale.

Текущий release profile — `opt-level = 3`, `debug = 2`, без LTO и явного `codegen-units`.
После алгоритмических исправлений стоит сравнить обычную сборку с:

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
```

Критерии — startup wall time, CPU во время flood, размер binary и время сборки. `debug = 2`
в основном влияет на артефакты/размер и диагностируемость, а не является объяснением секундной
runtime задержки. `target-cpu=native` не подходит для распространяемого бинарника.

## Предлагаемый порядок работ

1. Инструментировать queue depth и end-to-end `PaneOutput`; исправить трёхступенчатую доставку.
2. Повторить delayed 100k-lines тест. Главный acceptance: `spawn_delay` не растёт со временем,
   paint/input продолжаются, terminal state совпадает.
3. Быстро hoist-нуть process-aware bidi result один раз на pane/frame.
4. Прототип instanced `QuadInstance` + persistent upload; сравнить vertex bytes/recreate time и
   полный `gui.paint.impl`.
5. Спроектировать `ProcessGpuContext`/`WindowGpuSurface`; отдельно решить, сохраняется ли
   продуктовая гарантия «каждый launch — отдельный процесс».
6. Добавить метрики line snapshot allocations; только затем выбирать `Arc`/COW/compact snapshot.
7. Провести isolated benchmarks `Vec<BoxedQuad>` и LFU/LRU, затем внутренних hashers.
8. В самом конце — ThinLTO/codegen benchmark. Не смешивать build-profile выигрыш с изменением
   алгоритмов, иначе невозможно будет понять причину результата.

## Вывод

В проекте уже хорошо оптимизированы terminal data types и lock duration. Самый большой живой
дефект производительности сейчас — семантика очереди: GUI обрабатывает корректные маленькие
задачи слишком поздно, потому что на один логический dirty signal создаётся несколько физических
queue hop. После этого основной запас находится в представлении quad/GPU upload и в повторном
создании тяжёлого GPU context. Именно эти три направления должны дать заметный пользователю
результат; массовая замена целых чисел или контейнеров без профиля — нет.
