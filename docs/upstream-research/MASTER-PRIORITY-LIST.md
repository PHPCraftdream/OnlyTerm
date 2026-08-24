# Единый приоритизированный список: баги апстрима wezterm/wezterm

Синтез трёх триаж-отчётов (`triage-prs.md`, `triage-issues-part1.md`, `triage-issues-part2.md` —
1232 заголовка вручную просмотрены) и трёх отчётов кластеризации (`clusters-crashes.md`,
`clusters-rendering-perf.md`, `clusters-input-window.md` — 76 кластеров). Здесь кластеры,
повторяющиеся в разных отчётах (например "GPU teardown crash" встречался во всех трёх), сведены
в одну запись; там, где разные отчёты давали разный по полноте набор участников одного кластера,
взята более полная версия.

Правило приоритизации: **Tier 0** — есть готовый PR в апстриме, перенос дешёвый (trivial/needs
adaptation), можно делать прямо сейчас через `@sh`. **Tier 1** — механизм бага понятен и/или
подтверждён в наших исходниках, но готового фикса нет — нужна собственная инженерная работа.
**Tier 2/3** — реальные баги, но ниже по важности/дешевизне/уверенности — оставлены как бэклог
без завода отдельных тасков (могут быть заведены по запросу).

---

## Tier 0 — готовые фиксы в апстриме, переносить в первую очередь

| # | Кластер / баг | Участники | Перенос | Приоритет |
|---|---|---|---|---|
| 1 | Mux: неограниченная аллокация под PDU (OOM/stack overflow) | PR #7874 ↔ Issue #7527 | trivial, 1:1 матч, подтверждено в `codec/src/lib.rs:219,267` вручную | **highest** |
| 2 | SGR 8-битный цвет: двоеточие вместо точки-с-запятой (ломает Windows-консоль) | PR #2724 ↔ Issue #2723 | trivial, подтверждено в `onlyterm-escape-parser/src/csi.rs:~1513` | high |
| 3 | Sixel: пропущенные параметры цвета трактуются как 100 вместо 0 | PR #7345 ↔ Issue #7344 | trivial | medium-high |
| 4 | WebP lossy-декодирование не работает | PR #7694 ↔ Issue #7693 | trivial, подтверждено в `onlyterm-gui/src/glyphcache.rs` | medium |
| 5 | `LruCache::unbounded()` в `make_all_stale` → неограниченный рост памяти | PR #7704 (+ вероятно Issue #3771) | trivial, подтверждено в `onlyterm-client/src/pane/renderable.rs:426` | high |
| 6 | Windows: deadlock при смене раскладки клавиатуры (`WM_INPUTLANGCHANGEREQUEST`) | PR #7066 ↔ Issue #6786 | trivial | high |
| 7 | BlobLease: дубли-запись падает Access Denied на Windows | PR #7955 (+ Issue #5422/#7285/#6426 — тот же подузел, шире) | trivial-ish, расширить аудитом refcounting | high (#5422 — полная остановка рендера) |
| 8 | Bracketed-paste "defang" обходится вложенными маркерами (security) | PR #7859 (+ Issue #7784/#3510/#7921/#7078) | trivial, зрелый (+6 тестов) | high |
| 9 | Mux detach/reattach теряет `user_vars` | PR #7610 ↔ Issue #5832 | trivial (+4 строки) | medium-high |
| 10 | Mux detach/reattach теряет состояние mouse-reporting (tmux) | PR #6782 ↔ Issue #6764 | trivial | medium-high |
| 11 | Background-изображение: шов на стыке тайлов/градиента | PR #7030 ↔ Issue #6335 (был неверно помечен "фича" в исходном триаже) | trivial | medium |
| 12 | top_level-сплит: "призрачное" пространство после закрытия | PR #5969 ↔ Issue #4984/#4686 (был неверно помечен "фича") | trivial | medium-high |
| 13 | INTEGRATED_BUTTONS: левый статус-бар перекрывает кнопки | PR #7199 ↔ Issue #7197 (был неверно помечен "фича") | trivial | medium |
| 14 | Portable-pty (Unix): `exec()` падает `abort()` вместо `Result::Err`, диагностический pipe закрывается pre-exec хуком | PR #7743 (+ Issue #7742/#6783/#5107/#7025/#4364) | trivial для Unix-половины, тест есть | high |
| 15 | Windows: discovery gui-sock публикует только имя файла, `onlyterm cli` не подключается из другого cwd | PR #7896 + PR #7698 | trivial | medium-high |
| 16 | `--attach` не долетает до `domain_spawn_v2` при делегировании к существующему GUI | PR #7583 ↔ Issue #7582 | trivial | medium |
| 17 | Windows: `PATHEXT` с пустым `;;` → паника при `onlyterm start --` | Issue #6499 | one-line guard (нет готового PR, но тривиально) | medium-high |
| 18 | GPU-контекст teardown crash при закрытии/пересоздании окна (macOS/X11) | PR #7958, PR #7799, PR #7617 (+ Issue #5992 — Windows, без фикса) | needs adaptation, разные API на разных платформах | high |
| 19 | WebGPU: превышение лимита текстуры при экстремальном scale/тайлинге | PR #7821 ↔ Issue #7819 | needs adaptation (часть большого DPI-кластера, см. Tier 1 #29) | high |
| 20 | Wayland: DPI-calc фиксы + tiled-size + SCTK TILED-state | PR #7954, PR #7966, PR #7548 | needs adaptation, несколько точечных патчей | medium-high |
| 21 | Windows: drag-lock/resize-loop на границе мониторов с разным DPI | PR #7883 | needs adaptation (Windows-часть кластера DPI-drag, см. Tier 1 #30) | medium-high |
| 22 | Windows: conpty teardown hang при закрытии панели/таба + `procinfo` argv0 fallback + safety-check | PR #5977, PR #7226, PR #7398 (+ Issue #5882/#6190/#5496/#5432/#6094/#7847/#6198) | needs adaptation, #7398 особенно дёшев (модуль уже недавно рефакторили) | high |
| 23 | Mux client-server: deadlock при подключении с большим числом панелей + зависание capability-запросов | PR #7771, PR #7929 (+ Issue #7388/#858/#4283/#7959/#5225) | needs adaptation/substantial (наш UDS-транспорт уже переработан) | high |
| 24 | Kitty keyboard protocol: потеря Composed-клавиш (CJK/emoji-picker ввод) | PR #7944/#7915 (конкурирующие, брать одну), PR #7542 (mux-путь) (+ Issue #5224) | needs adaptation | high (полная потеря ввода CJK) |
| 25 | Wayland: дублирование клавиатурных событий с OpenGL-рендерером (гонка event loop) | PR #7435 (+ Issue #3609/#6725) | needs adaptation | high |
| 26 | Key-repeat: таймер не останавливается при закрытии окна (Wayland) | PR #7487 (+ Issue #4061/#5942/#5559 без фикса) | trivial для Wayland-части, остальное — Tier 1 | high |
| 27 | PaneFocused notification storm / feedback loop смены фокуса | PR #7871 (финальная, лучшая итерация — не #7763/#6349/#7590) (+ Issue #4484/#4390/#3994/#6885/#7096) | needs adaptation, но самый сильный сигнал во всём списке (4 конкурирующих апстрим-PR + 5 issues) | **high, топ-приоритет** |
| 28 | Выделение мышью landing в середине широкого (CJK) символа | PR #7930 ↔ Issue #6733 (+ #3494/#2910 смежно) | needs adaptation, тест есть | medium-high |
| 29 | Pane move/swap/rotate ломается в non-local mux-доменах | PR #5009 (+ Issue #6049/#6397/#4200/#5908) | substantial, но фикс есть | medium-high |
| 30 | Mux: бесконечный цикл/нулевой размер при подгонке размера панелей | PR #6062 (архитектурный переход дельты→абсолютные позиции) (+ Issue #7765/#4878/#6052) | substantial | high (#7765 — буквальный dead loop) |

---

## Tier 1 — высокий приоритет, механизм понятен, готового фикса в апстриме нет

| # | Кластер | Участники (репрезентативные) | Что делать |
|---|---|---|---|
| 31 | Крэш при нецелочисленном/экстремальном scale factor (независимо от WM) | Issue #2445/#3687/#6233/#4857 | аудит арифметики деления на scale-фактор в `window/`, `onlyterm-gui/src/termwindow/resize.rs` |
| 32 | Multi-monitor DPI drag feedback loop — macOS/X11-часть (Windows уже в Tier 0 #21) | Issue #1983/#3396/#2907/#6567/#7281/#3956 | адаптировать guard-паттерн из PR #7883 под macOS/X11 `window/src/os/*` |
| 33 | macOS: сон/пробуждение/screen-lock ломают размер/позицию окна на внешнем мониторе | Issue #4633/#6309/#2958/#6555 | аудит обработки display-reconfiguration notification на macOS |
| 34 | Фундаментальная порча рендера при ресайзе во время активного вывода (давняя, топ по частоте) | Issue #922/#1265/#2659/#3368/#3033/#6869/#4323/#2987/#4265/#7630/#3224 | начать с PR #7177 (`with_phys_lines` sync) как вероятного частичного кандидата, затем аудит reflow↔redraw порядка |
| 35 | Неверная позиция курсора после ресайза+reflow в alt-screen (DECSC/DECRC) | Issue #6669/#5100/#6623 | аудит save/restore курсора относительно reflow в alt-screen |
| 36 | Mux: ресайз панелей рассинхронизирован через remote/mux-домен (10 независимых репортов) | Issue #5142/#6666/#5117/#4723/#3694/#7331/#6884/#6052/#3671/#5011 | архитектурный аудит mux resize-протокола (абсолютные vs относительные размеры) |
| 37 | `window_decorations`: флаг RESIZE трактуется как RESIZE\|TITLE | Issue #3936/#6920/#6578/#6105/#5419 | аудит парсинга bitflags `WindowDecorations` в `window`/`config` |
| 38 | IME-композиция (preedit/committed text) — хрупкий конвейер в нескольких точках | PR #7494/#7556 (частично) + Issue #3411/#7358/#6433/#6222/#7173/#6228/#7695 | по каждой точке конвейера отдельно; #7358 (паника) — начать с неё |
| 39 | Idle/высокая загрузка CPU — общий event-loop/render cycle | Issue #7416 (конкретный hotspot `VTParser::action`) + широкий список | профилировать конкретно #7416; рассмотреть идею dirty-region/idle-skip из апстримного POC #7609 |
| 40 | Тормоза скролла в TUI (vim/tmux/pagers) | Issue #817/#6371/#5234/#7275/#7645 | тот же incremental-scroll/dirty-region подход, что и #39 |
| 41 | Медленный старт на Windows | Issue #7782 (точный root cause: `portable_pty::cmdbuilder` + gui-sock setup, 15-50с) + #7753/#6254/#6197/#4644/#7724 | профилировать и исправить конкретно локализованный участок |
| 42 | Фриз/подтормаживание GUI при устойчивом потоке крупного вывода (в т.ч. AI coding-инструменты) | Issue #7531/#7309/#7275/#5485 | профилировать backpressure/yield в parser/render loop под высокой пропускной способностью stdout |
| 43 | Фокус/мышь: рассинхронизация порядка событий на границе смены фокуса окна | PR #4875 (черновой, неуверенный) + Issue #5212/#2414/#5309/#3885/#3883 | #3883 (потеря ввода) и #5309 (потеря clipboard) — начать с них |
| 44 | Спонтанное дублирование окон/вкладок при spawn через workspace/domain | Issue #4527/#2984/#4901/#4408/#7096 | аудит гонки в spawn/reconciliation логике |
| 45 | Clipboard-операции асинхронны, гоняются с dispatch действий | Issue #3302/#5793 | синхронизировать чтение clipboard с action dispatch |
| 46 | Живая перезагрузка темы/appearance применяется не ко всем окнам одновременно | Issue #3328/#5451/#5982/#6607/#2446/#4437 | аудит hot-reload пути применения цветов/appearance |
| 47 | Windows/Unix portable-pty — оставшиеся крэши (не покрытые PR #7743) | Issue #6783/#5107/#7025/#4364 | разбор по каждому репорту отдельно, единого корня нет |
| 48 | Wayland: жёсткие крэши по протокольной ошибке (не покрыты PR #7735) | Issue #7969/#7725 | аудит обработки protocol errors/oversized-сообщений в Wayland dispatch |
| 49 | Необработанные ошибки уровня ОС валят процесс целиком | Issue #5263/#3107/#1839 | гигиенический аудит fallible OS-вызовов → `Result` + graceful degradation |
| 50 | Verification-тест: паника на исчезнувшем во время работы fallback-шрифте | Issue #6157 (вероятно уже закрыт нашим commit `5752050b8`, но не подтверждено тестом) | добавить regression-тест на сценарий "шрифт удалён посреди сессии" |

---

## Tier 2 — средний приоритет (бэклог, без отдельных тасков)

Записано в кластерных файлах, не дублируется здесь построчно — см.:
- `clusters-rendering-perf.md`: цвет/гамма-расхождение backend'ов, мерцание при переходах состояния окна, эмодзи/COLR метрики (требует переверификации на нашем стеке), курсор-рендер в разных состояниях, line_height искажения, AA/хинтинг-артефакты, оконные углы/рамки, WebGPU-прозрачность, macOS Acrylic/backdrop артефакты, tab bar layout при большом числе вкладок.
- `clusters-input-window.md`: INTEGRATED_BUTTONS остальные симптомы (#6086/#6351/#4076), fullscreen-нестабильность при смене фокуса (перепроверить PR #7941/#7948 на релевантность), OSC1337 leak, cursor-style после фокуса, dead-key/модификаторы edge cases, hyperlink hover на неактивной панели.
- `clusters-crashes.md`: утечки памяти в image/GPU-кэшах при долгой сессии (3 разных места, не единая причина), wgpu VRAM leak.

## Tier 3 — низкий приоритет / нишевое (бэклог)

NixOS/X11/специфичные WM-баги без общего механизма, старые low-engagement одиночные репорты без
подтверждённого repro. Полный список — в исходных `triage-issues-part1.md`/`triage-issues-part2.md`
разделах "Неважное".

---

## Открытый вопрос

Кластер "GPU-контекст teardown" (#18) и кластер DPI/scale (#19-21, #31-32) частично пересекаются
по механизму (оба — про невалидное состояние GPU-поверхности на границе платформенных событий) —
при реализации стоит рассмотреть, не закрывает ли общий фикс сразу оба.
