# Второй движок GPU-старта: полный OS-процесс на вкладку (webgpu_engine: PerTabProcess)

## Контекст

Расследование `docs/per-tab-crash-isolation-investigation.md` установило: сегодня `ProcessGpuContext`
(`crates/onlyterm-gui/src/termwindow/webgpu/context.rs:214-266`) — один `Instance`/`Adapter`/`Device`/`Queue`
на весь процесс `onlyterm-gui.exe`, общий для всех окон и вкладок. Необработанное SEH-исключение из
DXGI/D3D12 (как в реальном крахе PID 27376) убивает процесс целиком. Существующий `per_tab_process_isolation`
(Phase A-F, реальный, отгруженный) изолирует только pty/shell — не GPU.

Пользователь явно выбрал: второй движок должен давать **настоящую изоляцию на уровне ОС-процесса на
вкладку** (не по окну, не in-process). Пре-рефакторинг под это уже сделан (#642): `RenderBackend` —
трейт с единственной сегодня реализацией `RenderThreadHandle` (in-process). Конфиг-ключ:
`webgpu_engine: InProcess | PerTabProcess`, дефолт `InProcess` (без изоляции, подтверждено пользователем).
Каскад: главный `.onlyterm.ktav` → `.ktav` startup-layout на вкладку (по образцу `shell`/`priority`/`admin`
в `StartupTab`/`StartupLayout`).

**Важная находка этой сессии, меняющая расклад расследования:** вендорный `wgpu-hal` (`crates/wgpu-hal-vendored/src/dx12/mod.rs`)
уже поддерживает `SurfaceTarget::SurfaceHandle` — создание swapchain через
`IDXGIFactoryMedia::CreateSwapChainForCompositionSurfaceHandle` (не `CreateSwapChainForHwnd`!) на готовый
DirectComposition composition-surface handle (строки 1251-1266), плюс `SurfaceTarget::Visual` для
`IDCompositionVisual` (строки 1241-1250, 1288-1291). Это ГОТОВЫЙ, апстримный путь ровно для того, что
нужно: дочерний процесс рендерит в composition-surface, полученный по shared handle от родителя; родитель
владеет DirectComposition visual tree и реальным swapchain на настоящий HWND и просто показывает то, что
туда положил ребёнок — без ручного blit каждый кадр. Это НЕ то же самое, что "unprototyped `CreateSharedHandle`
+ ручная композиция", которое расследование считало основным риском — риск ниже, чем считалось.
Важная оговорка: сами крахи #2/#3 (в `CreateSwapChainForHwnd`) при этой архитектуре остаются в родителе
(он один владеет реальным HWND-swapchain на окно) — новый движок изолирует крахи, специфичные для
контента/отрисовки вкладки (draw calls, ресурсы), а не крахи в самом HWND-swapchain лайфсайкле, который
уже был закрыт другими фиксами этой сессии (#631, #641).

## Не-цели (explicit non-goals)

- Не переносим CPU-сторону построения кадра (`RenderState`/`GlyphCache`/box_model) в дочерний процесс —
  она остаётся в родителе, как и было решено в #642. Ребёнок — чистый GPU-consumer.
- Не пытаемся изолировать сам `CreateSwapChainForHwnd`/`Present` родителя на реальный HWND — это меняет
  архитектуру окна целиком (не только вкладки) и не было тем, что запросил пользователь.
- Не трогаем `per_tab_process_isolation` (pty) — независимая ось, переиспользуем только паттерн
  supervision (Job Object + `--supervise-pid`), не код.
- Не даём `webgpu_engine` дефолт `PerTabProcess` — дефолт `InProcess`, включение только через конфиг.
- Не поддерживаем `PerTabProcess` для split-панелей в первой итерации (как и `per_tab_process_isolation`
  в Phase B) — можно расширить позже.

## Фазы

### Phase A — валидационный спайк (вне основного бинаря, gate перед остальным)

Отдельная песочница **внутри репозитория**, не в глобальных папках (`D:\dev\rust\onlyterm\.scratch\dcomp-spike\`,
не отслеживается git, удаляется после спайка) с двумя маленькими процессами:
1. "Родитель": создаёт `IDCompositionDevice`, корневой visual, реальное окно + swapchain на него,
   создаёт composition-surface (через `IDCompositionDesktopDevice`/аналог) и **shared handle** на неё,
   дублирует handle в дочерний процесс (`DuplicateHandle`, тот же примитив, что уже используется для pty
   fd-sharing).
2. "Ребёнок": через vendored `wgpu-hal`'s `Instance::create_surface_from_surface_handle` +
   `Surface::configure`/`present()` (код уже есть, ничего нового писать не нужно) рисует что-то простое
   (цветной прямоугольник, потом текст) в этот surface.
3. Проверить на реальном железе машины (Intel UHD + NVIDIA RTX 3050 Ti — обе комбинации): рендер
   действительно появляется в родительском окне, синхронизация без разрывов, дочерний процесс можно
   убить (`TerminateProcess` на СОБСТВЕННЫЙ тестовый PID спайка, не onlyterm!) — родительское окно не
   падает и не виснет.
4. Замерить: время создания дочернего `Device` (стоимость на "тёплый старт вкладки"), задержку кадра
   через границу процесса, поведение при потере устройства ребёнка.

**Gate:** если базовый путь не работает или числа неприемлемы — остановиться, доложить, не переходить к
Phase B. Не приступать к Phase B без прогона этого спайка.

### Phase B — второй `RenderBackend` + hosting-процесс

- Новый режим бинаря: `onlyterm-gui.exe --gpu-tab-host --supervise-pid <pid> --ipc-pipe <name>`.
  Supervision — тот же паттерн, что и pty-изоляция: Job Object
  (`crates/onlyterm-client/src/client/windows_job.rs::assign_to_kill_on_close_job`) +
  `--supervise-pid` watcher-поток как fallback (по образцу `crates/onlyterm-mux-server/src/main.rs`).
- Формат кадра через границу процесса: те же данные, что несёт `GpuFrame` сегодня (вершины/индексы,
  список draw-команд, дельты обновления glyph-атласа как сырые пиксели), сериализованные в IPC-канал
  (именованный pipe или shared-memory ring buffer) — НЕ прямой шаринг GPU-хендлов на входе (только на
  выходе, через surface handle из Phase A). Ребёнок держит свой собственный кэш текстур/атласа в своём
  `Device` — дублирование памяти атласа принимается как цена простоты.
- Родительская реализация `PerTabProcessBackend: RenderBackend`: спавнит ребёнка, создаёт/владеет
  DirectComposition-деревом для места вкладки в окне, дублирует surface handle ребёнку, сериализует
  `GpuFrame` в канал вместо in-memory `mpsc::Sender`. Death/hang-детекция — через
  `WaitForSingleObject` на handle процесса-ребёнка вместо `Weak`-сентинела текущего `RenderThreadHandle`.
- Эпитафия: при неожиданной смерти ребёнка — заменить содержимое вкладки на GDI `EDIT`-контрол
  (`ES_READONLY | ES_MULTILINE`, по эскизу из investigation-документа), текст — код/адрес исключения из
  VEH-диагностики ребёнка (тот же механизм Phase 0, `RiskyDriverCallGuard`, которым ребёнок тоже должен
  быть оснащён). Копировать текст можно, взаимодействовать с вкладкой иначе — нет, закрыть — можно.
  Соседние вкладки/окна не задеты.

### Phase C — конфиг-каскад

- `crates/config/src/frontend.rs`: `WebGpuEngine { #[default] InProcess, PerTabProcess }` — тот же
  derive-набор и стиль, что `WebGpuPowerPreference` (frontend.rs:77-82).
- `crates/config/src/config.rs`: `#[dynamic(default)] pub webgpu_engine: WebGpuEngine` рядом с
  `webgpu_power_preference` (config.rs:349) — доккомент по образцу `per_tab_process_isolation` (config.rs:380).
- `crates/config/src/start_conf.rs`: добавить `webgpu_engine: Option<WebGpuEngine>` в `StartupTab` и
  `StartupLayout`, включить в `tab_options()`/`ResolvedTabOptions` по правилу "таб побеждает"
  (`tab.webgpu_engine.or(self.webgpu_engine)`), как уже сделано для `shell`/`priority`/`admin`
  (start_conf.rs:114-123).
- Место выбора реализации — там же, где сегодня `RenderThreadHandle::spawn(...)` боксуется в
  `Box<dyn RenderBackend>` (`render_pipeline.rs`, оба call site) — переключение по резолвленному
  значению `webgpu_engine`.

### Phase D — доки/хелпы

- `docs/config/reference/config/webgpu_engine.md` — по образцу `webgpu_power_preference.md` (теги
  `gpu`, `per-tab`), с разделом про override в startup-layout файле.
- Новый `docs/per-tab-gpu-hosting-architecture.md` — по структуре `docs/per-tab-hosting-architecture.md`.
- Аддендум в `docs/per-tab-crash-isolation-investigation.md`: находка про
  `SurfaceTarget::SurfaceHandle`/DirectComposition снимает главный риск, ранее считавшийся
  неподтверждённым ("unprototyped CreateSharedHandle") — зафиксировать со ссылкой на Phase A результаты.
- `crates/onlyterm-gui-subcommands/src/lib.rs`: обновить доккомент `--start-conf` — добавить
  `webgpu_engine` в список полей, которые можно переопределить на вкладку (сейчас перечислены
  root_dir/vars/commands, нужно упомянуть новое).
- `CLAUDE.md`: одна уточняющая строка в разделе "Прочее" — новые дочерние GPU-host-процессы тоже будут
  называться `onlyterm-gui.exe` (другой режим того же бинаря); правило "никогда не завершать
  onlyterm-gui.exe" покрывает их по имени бинаря автоматически, но явно предупредить, чтобы не
  перепутать их с "тестовыми/осиротевшими" процессами, которые можно убивать.

## Верификация

- Phase A: ручной прогон спайка на реальном железе (Intel + NVIDIA), глазами — рендер виден, дочерний
  процесс убивается без побочных эффектов на родителя.
- Phase B: `cargo build -p onlyterm-gui`, `cargo clippy -p onlyterm-gui --all-targets -- -D warnings`,
  `cargo test -p onlyterm-gui` (существующие 159 тестов не должны сломаться — `PerTabProcessBackend`
  добавляет новую реализацию трейта, не меняет старую). Ручной прогон: включить `webgpu_engine:
  PerTabProcess` в тестовом `.onlyterm.ktav`, открыть вкладку, убедиться в рендере; затем убить дочерний
  GPU-host процесс **по его собственному PID, зафиксированному при спавне** (см. стоп-правило
  CLAUDE.md) — убедиться, что вкладка показывает эпитафию, а не весь процесс.
- Phase C: юнит-тесты каскада в `start_conf.rs` по образцу существующих (`per_tab_root_dir_resolves_independently...`).
- Phase D: доки читаются, ссылки не битые.

## Порядок исполнения

Каждая фаза — самостоятельная задача в TaskList с `blockedBy` на предыдущую (Phase A блокирует B, B
блокирует C, C блокирует D). Начинаем строго с Phase A; последующие фазы — по результатам gate-проверки.
