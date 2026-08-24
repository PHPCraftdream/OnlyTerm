# Триаж открытых PR апстрима wezterm/wezterm (всего 280)

Методология: прочитан целиком `docs/upstream-research/open-prs-titles.txt` (280 строк), каждая
строка классифицирована. Для 74 PR, отнесённых к категории "важное" (баги/крэши/фризы/утечки/гонки/
security/хрупкий код), тело PR получено через `gh pr view <N> --repo wezterm/wezterm --json
title,body,files` (все 74 запроса выполнены успешно, без пропусков), затем применимость к нашему
дереву проверена через `git ls-files` / `git grep` / чтение исходников. Отдельно разобраны 3 POC PR
параллельной pure-Rust rendering инициативы (#7607/#7608/#7609) — см. отдельный раздел.

Основной вывод по применимости: наш форк убрал только SSH-клиент, TLS-mux, mlua/luahelper, git2 и
C-рендер/фонт-стек (cairo/freetype/harfbuzz → tiny-skia/rustybuzz/swash), но **сохранил** практически
весь остальной код в неизменном виде — mux, pty, term, window (macOS/X11/Wayland/Windows backends),
onlyterm-gui/termwindow, codec, tmux-CC. Поэтому подавляющее большинство важных апстримных багфиксов
(даже тех, что относятся к коду 2023–2024 годов) применимо к нашему дереву практически один в один.

## Важное — багфиксы/крэши/тормоза/плохой код (74 штуки)

### Крэши / undefined behavior / memory-safety

### PR #7958 — fix: macOS crash (SIGABRT) from RefCell re-entrancy panic in WindowView::draw_rect
- Что чинит: `draw_rect` в `window/src/os/macos/window.rs` держит живой `this.inner.borrow_mut()`
  guard во время повторного входа в отрисовку (rapid resize/repaint) → `RefCell` паникует →
  `SIGABRT`. Репортится как еженедельный краш.
- Применимо: файл `window/src/os/macos/window.rs` присутствует у нас без изменений в этой части.
- Сложность: trivial cherry-pick (сузить время жизни borrow guard перед вложенным вызовом).
- Приоритет: high.

### PR #7874 — fix(codec): bound PDU data allocation to avoid OOM crash (#7527)
- Что чинит: `codec/src/lib.rs` читает leb128-длину фрейма прямо с провода и аллоцирует
  `vec![0u8; data_len]` без верхней границы → повреждённый/сфабрикованный фрейм роняет mux-сервер
  или GUI-клиент аллокацией на несколько гигабайт.
- Применимо: `codec/src/lib.rs` идентичен апстриму, декодер PDU не переписывался в нашем форке.
- Сложность: trivial (добавить проверку верхней границы перед `vec![0u8; data_len]`).
- Приоритет: high (это классический OOM/DoS через сетевой протокол mux).

### PR #7799 — Fix SIGABRT on window close caused by OpenGL teardown ordering (MacOS)
- Что чинит: на закрытии окна на macOS 15.x GPU-drawable/IOSurface разрушается раньше, чем
  `RenderState`; `Drop` для `glium::RawProgram` дергает `make_current` → `CGLUpdateContext` →
  обращение к мёртвому Mach-порту → abort. Фикс: `render_state.take()` в обработчике
  `WindowEvent::Destroyed`.
- Применимо: `onlyterm-gui/src/termwindow/mod.rs` — код рендер-стейта и обработки закрытия окна не
  переписывался (у нас по-прежнему glium/OpenGL путь наравне с WebGPU).
- Сложность: needs adaptation (нужно свериться с текущей структурой `RenderState`/`Option`, но
  логика точечная — один `.take()` в правильном месте).
- Приоритет: high.

### PR #7617 — Fix X11 EGL surface use-after-free during window destruction
- Что чинит: use-after-free — event handler держит `TermWindow`, который держит
  `glium::backend::Context`, которому для корректного `Drop`/`make_current` нужен ещё живой X11
  handle. Если X11-окно уничтожается раньше glium-контекста — крэш.
- Применимо: `window/src/os/x11/window.rs` не тронут миграцией (миграция затронула только
  font/render-2D стек, не оконный backend).
- Сложность: needs adaptation (нужно поменять порядок Drop полей/явно дропнуть glium context перед
  уничтожением X11-окна).
- Приоритет: high (use-after-free — потенциально не только крэш, но и эксплуатируемая память).

### PR #7821 — webgpu: clamp surface dimensions to max_texture_dimension_2d
- Что чинит: `Surface::configure` в wgpu кидает validation error, если размер поверхности превышает
  `max_texture_dimension_2d` (16384 на Apple Silicon); внутри `did_resize` Objective-C→Rust FFI
  callback Rust не может размотать стек → abort. Возникает на macOS с тайлинг-WM (Aerospace) при
  пробуждении/ретайле на несколько Retina-мониторов.
- Применимо: `onlyterm-gui/src/termwindow/webgpu.rs` присутствует, WebGPU-бэкенд у нас активно
  используется (наравне с OpenGL).
- Сложность: trivial (clamp по обеим осям перед `SurfaceConfiguration`).
- Приоритет: high.

### PR #7704 — fix: preserve LruCache capacity in make_all_stale to prevent unbounded memory growth
- Что чинит: `RenderableInner::make_all_stale()` в `onlyterm-client/src/pane/renderable.rs` создаёт
  новый `LruCache::unbounded()`, теряя исходный лимит ёмкости. Срабатывает на каждом mux-коннекте
  клиента (initial resize) и далее на resize/zoom/palette change — построчный кэш растёт без
  ограничения, до многогигабайтного потребления за часы/дни работы.
- Применимо: `onlyterm-client/src/pane/renderable.rs` идентичен апстриму — client/mux слой не
  затрагивался нашей миграцией.
- Сложность: trivial (сохранить исходную capacity вместо `unbounded()`).
- Приоритет: high (классическая утечка памяти, легко воспроизводимая).

### PR #7771 — Fix mux client-server deadlock when connecting with many panes
- Что чинит: двусторонний deadlock сокета — `process_async` (сервер) и `client_thread_async`
  (клиент) читают/пишут на одной задаче; при подключении к серверу с ~30 табами GUI виснет
  навсегда, реконнект не помогает (сервер тоже застревает).
- Применимо: `onlyterm-client/src/client.rs`, `onlyterm-mux-server-impl/src/dispatch.rs` — не
  тронуты миграцией.
- Сложность: substantial (нужно разносить чтение/запись по разным задачам/каналам, это
  архитектурное изменение конкурентности, не однострочный патч).
- Приоритет: high.

### Зависания / фризы

### PR #7066 — fix: hang when receiving WM_INPUTLANGCHANGEREQUEST message
- Что чинит: переключение раскладки клавиатуры через `im-select` и подобные шлёт
  `WM_INPUTLANGCHANGEREQUEST` через `PostMessage`; сообщение не обрабатывается явно и падает в
  `DefWindowProcW`, что приводит к deadlock (см. известный анти-паттерн `WaitForSingleObject`).
- Применимо: `window/src/os/windows/window.rs` — Windows-backend окна не менялся миграцией.
- Сложность: trivial (добавить явный case в `do_wnd_proc`, вызвать `ActivateKeyboardLayout`,
  вернуть `Some(0)`).
- Приоритет: high.

### PR #5977 — fix(windows): closing panes sometimes freezes onlyterm
- Что чинит: закрытие панелей на Windows иногда виснет из-за особенностей работы `conpty.dll`
  (задокументированный edge-case у Microsoft: закрытие handle'ов до завершения псевдоконсольной
  сессии). Патч руками закрывает handle'ы в правильном порядке перед закрытием pty.
- Применимо: `pty/src/win/{mod.rs,conpty.rs,procthreadattr.rs,psuedocon.rs(sic)}` — Windows PTY
  слой не переписывался (только Lua/рендер/шрифты были предметом миграции).
- Сложность: needs adaptation (несколько файлов внутри `pty/src/win`, нужно сверить с текущей
  версией conpty-обёртки, но логика фикса локальная).
- Приоритет: high.

### PR #7226 — Fix: Prevent GUI freezing when closing tabs/panes after long sessions
- Что чинит: полное зависание GUI при закрытии табов/панелей после многодневных сессий с большим
  scrollback — три причины: синхронное удаление таб/панели в GUI-потоке, O(n) очистка scrollback,
  плюс третья (текст обрезан в теле PR, но обозначена).
- Применимо: `pty/src/win/mod.rs` (файл, который PR трогает) не менялся; корень (Windows PTY closing
  path) актуален для нас.
- Сложность: needs adaptation (несколько независимых причин, часть фикса может требовать
  профилирования scrollback-очистки в `term`/`mux`).
- Приоритет: high — жалоба буквально совпадает с формулировкой задачи пользователя ("зависания
  после долгих сессий").

### PR #7883 — Windows: Fix drag lock and resize loop when dragging across DPI boundary
- Что чинит: при перетаскивании окна между мониторами с разным DPI на Windows возникает
  feedback-loop в resize-коде (`live_resizing` → `scaling_changed()` → `set_inner_size()` →
  `SetWindowPos` посреди drag), окно "зацикливается" и разъезжается до экстремальных размеров.
- Применимо: `onlyterm-gui/src/termwindow/resize.rs` не тронут миграцией.
- Сложность: needs adaptation (нужно понять текущий resize/DPI pipeline, но фикс достаточно
  локальный — гасить один из путей ресайза во время drag).
- Приоритет: medium-high (не крэш, но юзабилити многомониторных Windows-конфигураций сломана до
  практической неработоспособности).

### Security / хрупкий/небезопасный код

### PR #7859 — fix(term): loop defang_paste until no embedded bracketed markers remain
- Что чинит: security-фикс — "обеззараживание" bracketed-paste (`\x1b[200~`/`\x1b[201~`) в
  `send_paste` делает один проход `str::replace`, что не ловит непересекающиеся вложенные
  конструкции: `\x1b\x1b[200~[200~` после одного прохода превращается в валидный маркер и позволяет
  вставленному (например, скопированному с вредоносного сайта) тексту внедрить свою paste-границу
  и тем самым "убежать" из paste-режима, заставив терминал выполнить произвольный ввод.
- Применимо: `term/src/terminalstate/mod.rs` идентичен апстриму — bracketed paste defanging не
  трогался.
- Сложность: trivial (обернуть текущую логику в цикл до fixed point).
- Приоритет: high — это самый настоящий injection-класс security баг (terminal paste attack), и
  фикс тривиален.

### PR #7743 — pty: mark inherited fds CLOEXEC instead of closing them
- Что чинит: pre-exec hook в `pty/src/unix.rs` закрывает все fd > 2, включая внутренний канал
  Rust stdlib для передачи exec-error родителю — из-за этого при ошибке `exec()` пользователь не
  получает реальную причину сбоя. Фикс — ставить `FD_CLOEXEC` вместо закрытия, канал доживает до
  момента, когда есть, что сообщить.
- Применимо: `pty/src/unix.rs` не менялся.
- Сложность: trivial-easy (плюс у PR есть regression test).
- Приоритет: medium-high (не крэш, но искажение диагностики ошибок spawn — реальная порча UX/DX,
  плюс попутно более аккуратная работа с fd-наследованием — "хрупкий код").

### PR #7053 — Fix: Don't use client environment to execute command on server
- Что чинит: при построении команды для исполнения на mux-сервере использовалось базовое окружение
  клиента вместо серверного — потенциальная утечка/подмена переменных окружения между
  клиентом и сервером, если клиент явно его очистил.
- Применимо: `mux/src/domain.rs`, `pty/src/cmdbuilder.rs`, `onlyterm-client/src/domain.rs` — не
  менялись.
- Сложность: needs adaptation (проверить текущие fixup-пути CommandBuilder).
- Приоритет: medium (граница доверия client/server в mux-протоколе, стоит того, чтобы разобраться).

### Кластер "PaneFocused notification storm / feedback loop" (4 связанных PR — рекомендуется решать вместе)

Четыре PR решают вариации одного и того же корневого бага: `PaneFocused`-уведомления, которые
обрабатываются в `mux/src/tab.rs`, `onlyterm-client/src/pane/clientpane.rs`,
`onlyterm-gui/src/frontend.rs`, `onlyterm-mux-server-impl/src/sessionhandler.rs`, могут порождать
новые `PaneFocused`-события в ответ на себя же → зацикливание/шторм при массовом уничтожении
панелей, множественных mux-клиентах или задержках домена. Все затронутые файлы у нас присутствуют
без изменений.

### PR #7871 — Fix PaneFocused notification feedback loop - take 3
- Что чинит: третья, наиболее вычищенная итерация фикса storm'а `PaneFocused` при массовом
  уничтожении панелей и mux reconciliation (продолжение #7763, те же файлы). Тестировалось вручную
  и через cli на предмет регрессии #4390/#4737.
- Применимо: да, тот же набор файлов (`mux/src/lib.rs`, `mux/src/tab.rs`, `mux/src/tmux_commands.rs`,
  `onlyterm-client/src/pane/clientpane.rs`, `onlyterm-gui/src/frontend.rs`,
  `onlyterm-mux-server-impl/src/sessionhandler.rs`) присутствует без изменений.
- Сложность: substantial (много скоординированных изменений в разных крейтах, но это самая зрелая
  версия фикса в цепочке — рекомендуется взять именно её, а не #7763 отдельно).
- Приоритет: high.

### PR #7763 — Fix PaneFocused notification storm on pane destruction and mux reconciliation
- Что чинит: то же самое, что #7871, но это предыдущая (закрытая через #7871) итерация —
  оставлена для истории решения. Не портировать отдельно от #7871.
- Применимо: superseded — см. #7871.
- Сложность: n/a (не портировать).
- Приоритет: n/a — использовать #7871 как основной кандидат.

### PR #6349 — Introduce sequencing for 'pane focus' related events
- Что чинит: альтернативный (более старый, архитектурно иной) подход к тому же классу бага —
  вводит serial/sequence number для GUI-инициированных смен фокуса панели, чтобы GUI мог
  игнорировать устаревшие ответы сервера. Затрагивает 14 файлов, включая `codec/src/lib.rs`,
  `onlyterm/src/cli/activate_{pane,tab}.rs`.
- Применимо: файлы существуют, `mux/src/serial.rs` из PR у нас отсутствует (т.е. концепции serial
  ещё нет вообще).
- Сложность: substantial — это принципиально другое (более общее) решение, чем #7871/#7763.
  Рекомендация: не портировать оба подхода параллельно — выбрать один. Практический совет: сначала
  оценить, покрывает ли #7871 наши реальные сценарии (detach/reattach, множественные mux-клиенты);
  если нет — рассмотреть #6349 как более фундаментальное решение.
- Приоритет: medium (конкурирует с #7871, не делать оба).

### PR #7590 — Fix: flickering across multiple mux clients
- Что чинит: тот же класс проблемы под другим углом — при нескольких клиентах, подключённых к
  одному mux-серверу, смена фокуса панели создаёт broadcast-петлю (клиент→сервер→broadcast всем
  клиентам→получатель считает это новой сменой фокуса→снова сообщает серверу), что вызывает мигание
  экрана. Фикс вводит флаг `NotifyMux` в `set_active_pane`, чтобы серверные изменения фокуса не
  ретранслировались обратно.
- Применимо: файлы (`mux/src/lib.rs`, `mux/src/tab.rs`, `mux/src/tmux_commands.rs`,
  `onlyterm-client/src/pane/clientpane.rs`, `onlyterm-mux-server-impl/src/sessionhandler.rs`)
  пересекаются с #7871/#7763 почти один в один — весьма вероятно, что фикс #7871 уже закрывает и
  этот случай, либо оба патча нужно свести в один заход, а не переносить раздельно.
- Сложность: substantial (нужно свести с #7871 в единую реализацию, не дублировать логику).
- Приоритет: medium — часть того же кластера, оценивать вместе с #7871/#6349.

### PR #7929 — mux: answer capability queries and bound the hold while a synchronized update is open
- Что чинит: во время открытого synchronized-output "hold" (DECSET 2026) запросы возможностей
  терминала (DA1/DA2/DA3, XTVERSION, DSR, kitty keyboard query) раньше застревали в hold-буфере
  вместе с состоянием экрана — приложение, ждущее ответа на capability-query, могло зависнуть до
  окончания synchronized-update; плюс добавляется таймаут, ограничивающий время удержания.
- Применимо: `mux/src/lib.rs` содержит `parse_buffered_data` и обработку
  `DecPrivateModeCode::SynchronizedOutput` без изменений; конфиг-поля
  `mux_synchronized_output_timeout_ms` в нашем дереве ещё нет — это будет добавлено вместе с фиксом.
- Сложность: needs adaptation (нужно ввести отдельный путь ответа на capability-queries помимо
  hold-буфера — умеренный объём работы, но инфраструктура synchronized-update уже на месте).
- Приоритет: medium (предотвращает зависание клиентских приложений, ожидающих ответа терминала).

### Хрупкий/плохой код (соответствует нашему же стилю недавних правок, напр. `procinfo`/`iterative not recursive`)

### PR #7398 — fix(procinfo): return argv0 when handling priviledged programs with `can_close_without_prompting`
- Что чинит: чтение `/proc/<pid>/exe` для процесса с повышенными привилегиями (например, под
  `sudo`) без повышенных прав самого onlyterm всегда возвращает пустой путь → в списке процессов
  остаётся только шелл, из-за чего `can_close_without_prompting` неверно считает, что "опасного"
  дочернего процесса нет, и таб закрывается без подтверждения, хотя внутри всё ещё работает,
  например, `sudo apt install`.
- Применимо: **прямое попадание** — `procinfo/src/lib.rs` мы только что рефакторили (коммит
  `0894be2c4`, "make LocalProcessInfo tree ops iterative, not recursive"); это тот же модуль,
  логика привилегированных процессов не менялась и баг актуален.
- Сложность: trivial (fallback-чтение argv0, локальный, некрупный патч).
- Приоритет: medium-high — риск незаметного закрытия таба с активным привилегированным процессом,
  и попадает прямо в код, который мы недавно трогали (стоит перенести заодно, раз мы там).

### PR #7177 — Align with_phys_lines()'s implementation with with_phys_lines_mut()
- Что чинит: `Screen::with_phys_lines()` в `term/src/screen.rs` неверно считает офсет второго
  диапазона `VecDeque` (в отличие от корректной `with_phys_lines_mut()`, которая вычитает длину
  первого диапазона) — воспроизводится при обрезании scrollback-части `.lines`, потенциальный
  выход за границы/некорректный доступ.
- Применимо: `term/src/screen.rs` присутствует, не менялся.
- Сложность: trivial (привести немутабельную реализацию в соответствие с мутабельной).
- Приоритет: medium-high (рассинхронизация двух параллельных реализаций одной и той же логики —
  ровно тот "хрупкий код", который просил найти пользователь; риск некорректного чтения
  scrollback).

### PR #7181 — Replace socket with channel for internal pane communication
- Что чинит: автор указывает, что не понимает, зачем внутрикоммуникация между панелями в одном
  процессе (`mux/src/lib.rs`) реализована через loopback-сокет вместо in-process канала — это ломает
  сценарии с firewall, блокирующим весь трафик включая loopback. Автор прямо пишет, что патч
  "быстрый исследовательский", не финальный.
- Применимо: `mux/src/lib.rs` содержит этот механизм без изменений.
- Сложность: substantial (нужен собственный дизайн, а не прямой перенос черновика — но сама идея
  "не гонять внутрипроцессные данные через сокет" правильная и решает реальный класс проблем
  firewall/loopback-политик, актуальный и для нас на Windows).
- Приоритет: medium (хрупкая архитектура, но не горящий баг).

### Windows-специфичные баги

### PR #7955 — Fix duplicate blob storage on Windows
- Что чинит: `SimpleTempDir::store` в `onlyterm-blob-leases/src/simple_tempdir.rs` перезаписывает
  файл по content-hash пути при каждом сохранении; если у него ещё открыт читатель — Windows
  возвращает "Access is denied" (error 5), что ломает повторную загрузку одинакового контента
  (например, анимированных фонов в дополнительных окнах). Фикс — переиспользовать существующий blob
  и инкрементить refcount.
- Применимо: `onlyterm-blob-leases/src/simple_tempdir.rs` присутствует без изменений.
- Сложность: trivial-easy (плюс регресс-тест в PR).
- Приоритет: medium.

### PR #7896 — windows: publish absolute gui sock path so `onlyterm cli` connects from any cwd
### PR #7698 — windows: resolve gui socket path from runtime dir
- Что чинит (обе вместе): на Windows discovery GUI-сокета (`onlyterm-client/src/discovery.rs`)
  публикует только имя файла в shared memory; при резолве это имя ошибочно трактуется как полный
  путь → `onlyterm cli` не может подключиться к работающему GUI, если cwd клиента не совпадает с
  runtime dir. #7896 чинит сторону публикации, #7698 — сторону резолва (join с
  `config::RUNTIME_DIR`); это дополняющие друг друга половины одного и того же исправления одного
  файла.
- Применимо: `onlyterm-client/src/discovery.rs` присутствует, не менялся.
- Сложность: trivial (оба патча небольшие; переносить вместе, а не по отдельности).
- Приоритет: medium-high (базовая функциональность `onlyterm cli` на Windows).

### PR #7879 — fix: allow navigation to non-admin symlinks on Windows
- Что чинит: Windows-инсталлятор (Inno Setup) ломается при симлинке `~/.config/onlyterm`
  ("The path cannot be traversed because it contains an untrusted mount point", os error 448) —
  актуально для пользователей, симлинкающих versioned dotfiles.
- Применимо: `ci/windows-installer.iss` присутствует.
- Сложность: trivial (правка Inno Setup скрипта/манифеста).
- Приоритет: low-medium.

### PR #7583 — FIX: propagate --attach flag when delegating to existing GUI
- Что чинит: при делегировании spawn к уже работающему GUI, флаг `--attach` не долетал до
  `domain_spawn_v2` — новая панель могла спауниться не так, как просил пользователь (без attach к
  уже detached домену). Требует бампа версии codec (`SpawnV2`, 45→46).
- Применимо: `codec/src/lib.rs`, `onlyterm-client/src/domain.rs`,
  `onlyterm-mux-server-impl/src/sessionhandler.rs`, `onlyterm/src/cli/spawn_command.rs` — все на
  месте.
- Сложность: needs adaptation (нужно свериться с текущим codec version counter, но сама правка
  небольшая).
- Приоритет: medium.

### PR #6062 — don't resize pane with delta
- Что чинит: при быстрой серии mouse-событий ресайза (100→99→98→90 колонок) относительные дельты
  применяются к уже устаревшему состоянию (из-за гонки resize/resync), в результате конечный размер
  панели хаотично отличается от желаемого — на видео проиллюстрирован "дребезг" сплита при быстром
  перетаскивании границы.
- Применимо: `mux/src/tab.rs`, `onlyterm-gui/src/termwindow/mouseevent.rs` не менялись.
- Сложность: needs adaptation (нужно перейти на абсолютные позиции вместо дельт в resize-протоколе
  сплита).
- Приоритет: medium (UX-баг при интерактивном ресайзе сплитов, не крэш, но раздражающий и
  воспроизводимый).

### PR #6881 — Fix build fails when the target dir doesn't exist
- Что чинит: `onlyterm-gui/build.rs` копирует `OpenConsole.exe` и т.п. в дефолтный `target/` на
  Windows; если используется кастомный `CARGO_TARGET_DIR`, копирование падает и сборка прерывается.
  Фикс — читать `CARGO_TARGET_DIR` перед копированием.
- Применимо: `onlyterm-gui/build.rs` присутствует.
- Сложность: trivial.
- Приоритет: low-medium (влияет на воспроизводимость сборки в нестандартных окружениях, что
  релевантно для форка с собственной CI).

### Wayland/X11/macOS — платформенные баги рендеринга/окон

### PR #7967 — Fix: Window still dragging with hidden window decorations
- Что чинит: на macOS клик по верхней строке окна с `window_decorations = "RESIZE"/"NONE"`
  запускает нативный drag окна вместо выделения текста — потому что `NSWindowStyleMaskTitled`
  остаётся в style mask (чтобы сохранить скруглённые углы), а `NSWindow.isMovable` по умолчанию
  `true` и AppKit сам решает считать верхнюю полосу зоной드래그а независимо от видимости титлбара.
- Применимо: `window/src/os/macos/window.rs`.
- Сложность: trivial.
- Приоритет: medium.

### PR #7966 — wayland: honor compositor size for tiled windows (eg: sway)
- Что чинит: на тайлинг-композиторах (sway) onlyterm рендерится в узкую область вместо выделенного
  тайла; причина — спекулятивный resize (`apply_dimensions()`/`set_inner_size`) при первом
  определении scale factor перезаписывает compositor-заданный tiled size собственным configure.
- Применимо: `window/src/lib.rs`, `window/src/os/wayland/window.rs`.
- Сложность: needs adaptation (нужно не пересчитывать size при первом scale-событии, если окно
  тайловое).
- Приоритет: medium-high (полностью ломает использование на sway/аналогах).

### PR #7954 — wayland: DPI calculation fixes for different desktop scale factors
- Что чинит: на COSMIC desktop и т.п. неверный начальный расчёт DPI приводит к неправильному
  размеру и проблемам с dragging у только что созданных окон; пропадает после ручного ресайза
  (который триггерит пересчёт DPI).
- Применимо: `window/src/os/wayland/{connection.rs,window.rs}`.
- Сложность: needs adaptation.
- Приоритет: medium.

### PR #7548 — Treat SCTKWindowState::TILED as a maximized state
- Что чинит: SCTK ввёл `TILED` состояние вместо/вместе с `MAXIMIZED`; onlyterm его не обрабатывает
  и считает тайловое окно resizable, что рендерит в неверном размере.
- Применимо: `window/src/os/wayland/window.rs`.
- Сложность: trivial.
- Приоритет: medium.

### PR #7735 — Fix wgpu/Vulkan rendering on Wayland compositors (like Niri)
- Что чинит: без этого фикса окна onlyterm зависают на старте или становятся неотзывчивыми при
  `front_end = "WebGpu"` на Wayland+Vulkan; на tiling-компоузерах (Niri) фриз происходит сразу и не
  лечится ресайзом. Причина — конфликт между `wl_surface.frame()` callback throttling через SCTK
  (расчитан на EGL/`eglSwapBuffers`) и собственным управлением commit'ами surface в Vulkan WSI
  (`vkQueuePresentKHR`).
- Применимо: `onlyterm-gui/src/renderstate.rs`, `onlyterm-gui/src/termwindow/{webgpu.rs,render/draw.rs}`,
  `window/src/os/wayland/window.rs` — все присутствуют, WebGPU-бэкенд у нас в строю.
- Сложность: needs adaptation (два независимых фикса: frame-callback bypass + что-то ещё, детали
  обрезаны в теле PR — требуется прочитать диф целиком перед портированием).
- Приоритет: high (полный фриз при старте на Wayland+Vulkan — это прямое попадание в категорию
  "фризы", особенно если у нас есть пользователи с WebGPU на Linux/Wayland).

### PR #7601 — fix(wayland): prevent title bar with window_decorations = "NONE"
- Что чинит: при явном отключении декораций на Wayland всё равно показывался титлбар.
- Применимо: `window/src/os/wayland/window.rs`.
- Сложность: trivial.
- Приоритет: low-medium.

### PR #7487 — wayland: terminate key repetition after window is closed
- Что чинит: key-repeat таймер не останавливается после закрытия окна на Wayland; попутно автор
  отмечает утечку — хэшмап окон растёт, но записи из него не удаляются (не критично для короткой
  сессии, но релевантно при долгой работе).
- Применимо: `window/src/os/wayland/window.rs`.
- Сложность: trivial (плюс отдельная задача на очистку хэшмапа окон, если делать полностью).
- Приоритет: medium.

### Kitty keyboard protocol / клавиатурный ввод — кластер

### PR #7944 — Fix: Kitty keyboard protocol drops single-char Composed key events
### PR #7915 — fix: encode Composed keys properly in Kitty keyboard protocol
- Что чинят (дублирующие фиксы одного бага): `encode_kitty()` в `onlyterm-input-types/src/lib.rs` не
  имеет ветки для `KeyCode::Composed` (символ от IME/emoji picker без "сырого" hardware-события) —
  падает в catch-all, который требует `self.raw`, а он всегда `None` → функция молча возвращает
  пустую строку, нажатие теряется без ошибки. Особенно заметно при CJK-вводе (китайский/корейский)
  под Kitty-протоколом на Windows.
- Применимо: `onlyterm-input-types/src/lib.rs` присутствует без изменений.
- Сложность: trivial (добавить match-ветку для `Composed`); #7944 и #7915 — по сути конкурирующие
  реализации одного и того же фикса, переносить одну (любую, суть идентична).
- Приоритет: high (полная потеря ввода CJK-текста при активном kitty-протоколе — серьёзная
  регрессия функциональности, не просто косметика).

### PR #7936 — macos: fix CMD chords losing modifiers (IME forwarding + command-layer chars)
- Что чинит: на macOS `CMD+SHIFT+D` и подобные chord'ы не долетают до приложений с kitty keyboard
  protocol — либо IME-форвардинг съедает модификаторы (когда `use_ime=true` и модификаторы
  пересекаются с `macos_forward_to_ime_modifier_mask`), либо (для non-US раскладок) сама раскладка
  съедает CMD-модификатор.
- Применимо: `window/src/os/macos/window.rs`.
- Сложность: needs adaptation (два независимых бага в одном файле).
- Приоритет: medium-high.

### PR #7877 — fix(kitty): emit SS3 cursor keys when DECCKM is active
- Что чинит: kitty-кодирование клавиш-стрелок игнорирует DECCKM (cursor key application mode) —
  приложения вроде `less`/`git show`, которые ставят DECCKM и ждут `ESC O {key}`, ломаются (стрелки
  не скроллят), если включен kitty keyboard protocol.
- Применимо: `mux/src/{localpane.rs,pane.rs}`, `term/src/terminalstate/mod.rs`,
  `onlyterm-gui/src/termwindow/keyevent.rs`, `onlyterm-input-types/src/lib.rs` — все на месте.
- Сложность: needs adaptation (несколько файлов, но логика точечная — учитывать DECCKM в
  kitty-кодере).
- Приоритет: medium-high (ломает базовую навигацию в `less`/pagers при включённом kitty-протоколе).

### PR #7804 — Fix Shift-AltGr dead key probing
- Что чинит: на Windows-раскладках, где dead keys доступны через Shift+AltGr (например, Czech
  Programmer — Ř, Š, Č, Ž, ň), `KeyboardLayoutInfo::probe_dead_keys()` не пробует комбинацию
  `Shift | RIGHT_ALT`, поэтому такие dead keys никогда не распознаются и не работают.
- Применимо: `window/src/os/windows/window.rs`.
- Сложность: trivial (добавить комбинацию в перебираемый набор `shift_states`).
- Приоритет: medium (ломает ввод диакритики для нескольких раскладок целиком).

### PR #7145 — fix: key-repeat detection for kitty input protocol
- Что чинит: `repeat_count` в kitty-протоколе на всех платформах захардкожен в 1 — автор явно
  пишет, что реализовал только для macOS и "не имеет ресурсов" доделать остальные платформы; PR —
  отправная точка, а не готовый фикс.
- Применимо: `onlyterm-input-types/src/lib.rs`, `onlyterm-gui/src/termwindow/keyevent.rs`,
  `window/src/os/macos/window.rs` — на месте.
- Сложность: substantial (реализация не завершена самим автором; чтобы закрыть баг полностью,
  нужно доделать Linux/Windows).
- Приоритет: medium (несоответствие спецификации kitty-протокола, но не крэш).

### PR #6849 — Remove `mode` from `PushKittyState` CSI representation
- Что чинит: термвиз (у нас — `onlyterm-escape-parser`) излишне эмитит/принимает необязательный
  параметр `mode` в push-последовательности kitty keyboard protocol, чего нет ни в спецификации,
  ни в референсной реализации kitty — расхождение со спецификацией протокола.
- Применимо: `term/src/terminalstate/performer.rs`, `onlyterm-escape-parser/src/csi.rs` (было
  `termwiz/src/escape/csi.rs` в апстриме — у нас этот код просто переехал в отдельный крейт
  `onlyterm-escape-parser`, логика идентична).
- Сложность: trivial.
- Приоритет: medium (протокольная корректность для приложений, строго проверяющих кодирование).

### PR #7542 — fix kitty keyboard protocol not work in mux mode
- Что чинит: локальные панели кодируют kitty keyboard protocol корректно, но панели через mux идут
  другим кодовым путём и не кодируют его вовсе.
- Применимо: `termwiz/src/input.rs` присутствует (обратите внимание: несмотря на переезд
  escape-парсера в `onlyterm-escape-parser`, `termwiz` как крейт у нас тоже остаётся — см.
  Cargo.toml workspace members).
- Сложность: needs adaptation (нужно свести локальный и mux code path).
- Приоритет: medium-high (полная потеря kitty-протокола в самом частом сценарии — мультиплексовании).

### PR #7435 — Fix keyboard event duplication on Wayland with OpenGL renderer
- Что чинит: гонка в event loop — фиксированный порядок обработки (сначала `SPAWN_QUEUE`, затем
  Wayland dispatch) при быстрых key press/release (~200мс) приводит к дублированию клавиатурных
  событий (Ctrl+U скроллит дважды, Enter срабатывает дважды). Воспроизводится только с OpenGL, не с
  WebGPU.
- Применимо: `window/src/os/wayland/{keyboard.rs,window.rs}`.
- Сложность: needs adaptation (нужно проанализировать порядок event loop на предмет гонки).
- Приоритет: high (гонка данных, дублирующая пользовательский ввод — прямое попадание в критерий
  "гонки данных" из задачи).

### PR #4991 — Fix Emits of Additional ANSI Characters
- Что чинит: X11-обработчик клавиатуры эмитит ANSI-символы при нажатии нестандартных модификаторов,
  хотя не должен.
- Применимо: `window/src/os/x11/keyboard.rs`.
- Сложность: trivial.
- Приоритет: medium.

### Terminal state / protocol correctness

### PR #7930 — Fix selection landing in the middle of wide characters (#6733)
- Что чинит: при выделении CJK/широких символов выделение может начаться/закончиться посередине
  широкого символа; двойной клик по слову оставляет "хвост" широкого символа невыделенным. Две
  независимые причины — в `compute_double_click_range` (Line::compute_double_click_range) не
  учитывается ширина ячейки, и в обработке click/drag выделения.
- Применимо: `onlyterm-gui/src/termwindow/mouseevent.rs`, `onlyterm-surface/src/line/line.rs` — на
  месте (термин "surface" — наш аналог/потомок структуры line из термвиза, не тронут миграцией).
- Сложность: needs adaptation (несколько файлов, есть новый тест).
- Приоритет: medium (заметный, регулярно воспроизводимый баг для CJK-пользователей).

### PR #7722 — fix: Use current attrs for cells inserted with ICH command
- Что чинит: атрибуты ячеек, вставляемых командой ICH (Insert Character), берутся некорректно.
- Применимо: `term/src/terminalstate/mod.rs`. **Внимание**: сам автор в PR пишет "Actually this fix
  seems to be wrong... it doesn't work as expected... Need to be reworked" — то есть фикс сам
  признан нерабочим.
- Сложность: не портировать как есть; if реализовывать — заново, с нуля, по описанию проблемы
  (#7715), а не по диффу этого PR.
- Приоритет: low (баг реален, но готового рабочего фикса апстрим ещё не имеет).

### PR #7626 — fix: suppress duplicate DA responses for tmux control mode panes
### PR #7292 — tmux-CC: suppress capability handshakes in control mode
- Что чинят (пересекающийся кластер): под `tmux -CC` и tmux, и onlyterm отвечают на DA/capability
  запросы приложения (например, neovim) — приложение получает ответ от tmux сразу, а ответ onlyterm
  приходит с опозданием через control-протокол уже после выхода приложения → в шелле остаётся
  "мусорная" escape-последовательность на экране. #7292 — более общее решение (перехват и
  DA/OSC/CSI/DCS handshake, буферизация по панели), #7626 — более узкое (только DA1/2/3).
- Применимо: `mux/src/{tmux.rs,tmux_commands.rs,tmux_pty.rs}`, `term/src/terminalstate/mod.rs` — на
  месте, tmux control mode не переписывался.
- Сложность: needs adaptation (рекомендуется взять #7292 как более полное решение, #7626 как
  референс/доп. тест-кейс).
- Приоритет: medium.

### PR #7148 — fix: tmux -CC "Unrecognized tmux cc line error for %unlinked-window-renamed"
- Что чинит: парсер control-mode строк (`tmux_cc/tmux.pest`) не знает событие
  `%unlinked-window-renamed`, что приводит к ошибке парсинга.
- Применимо: `onlyterm-escape-parser/src/tmux_cc/{mod.rs,tmux.pest}` — présent (переехало вместе с
  остальным escape-parser кодом, логика та же).
- Сложность: trivial (добавить правило в pest-грамматику).
- Приоритет: low-medium.

### PR #6782 — fix: tmux -CC mouse reporting issue
- Что чинит: при detach/attach tmux control-сессии теряется состояние mouse reporting
  (`set mouse=a` в vim) — нужно вычитывать состояние у tmux и восстанавливать на локальном
  терминале при повторном attach.
- Применимо: `mux/src/tmux_commands.rs`.
- Сложность: trivial.
- Приоритет: low-medium.

### PR #7345 — fix omitted color params in sixel parser
- Что чинит: пропущенные параметры в sixel color-определениях (`#1;2;;50;`) неверно трактуются как
  100 из-за -1 sentinel-значений, переполняющихся при приведении типов; по спецификации DEC Sixel
  пропущенный параметр должен по умолчанию быть 0.
- Применимо: `onlyterm-escape-parser/src/parser/sixel.rs`.
- Сложность: trivial.
- Приоритет: medium (протокольная корректность рендеринга sixel-изображений).

### PR #2724 — termwiz: revert to using semicolon when encoding 8 bit SGR colors
- Что чинит: Windows-консоль не поддерживает 8-битные (256-цветные) SGR-последовательности с
  двоеточием-разделителем (только true-color нужно двоеточие), regression от более раннего коммита.
- Применимо: **подтверждено чтением исходников** — в `onlyterm-escape-parser/src/csi.rs`, макрос
  `ansi_color!` (строка ~1513), у нас до сих пор `write!(f, "{}:5:{}m", ...)` для 8-битного индекса
  палитры — то есть баг из PR **буквально всё ещё присутствует** в нашем коде один в один.
- Сложность: trivial (поменять формат-строку `"{}:5:{}m"` → `"{};5;{}m"`, аналогично для
  background/underline color веток).
- Приоритет: medium-high — подтверждённый, легко тестируемый, тривиальный к переносу баг
  совместимости с Windows-консолью/некоторыми терминалами, не принимающими двоеточие в 256-цветном
  SGR.

### PR #7610 — fix: sync user_vars on mux reconnect (Issue #5832)
- Что чинит: при detach→reattach к mux-серверу пользовательские переменные (`user_vars`) теряются,
  потому что `GetPaneRenderChangesResponse` их не включает — добавляется поле `user_vars` в
  response.
- Применимо: `codec/src/lib.rs`, `onlyterm-client/src/pane/clientpane.rs`,
  `onlyterm-mux-server-impl/src/sessionhandler.rs` — на месте.
- Сложность: trivial (+4 строки в апстримном диффе).
- Приоритет: low-medium.

### Разное — функциональные баги

### PR #6913 — fix: onlyterm cli set-window-title #4899
- Что чинит: `onlyterm cli set-window-title foo` не применяет заголовок, если нет
  `format-window-title` Lua/rhai-обработчика — фикс переставляет приоритет источников заголовка
  (заголовок объекта окна > дефолт), если нет обработчика форматирования.
- Применимо: `onlyterm-gui/src/termwindow/mod.rs`.
- Сложность: trivial.
- Приоритет: medium (базовая CLI-функциональность, которая просто не работает).

### PR #7091 — Prevent shell integration from interfering with other terminal programs
### PR #6957 — fix: disable shell integration inside a Neovim terminal
- Что чинят: `assets/shell-integration/onlyterm.sh` подключается через
  `/etc/profile.d/onlyterm.sh` без проверки, что текущий терминал — именно onlyterm, из-за чего
  ломает поведение других терминалов, и отдельно — печатает мусор перед промптом внутри
  `:terminal` в Neovim.
- Применимо: `assets/shell-integration/onlyterm.sh` — присутствует без изменений (shell-интеграция
  не зависит от Lua/rhai/рендера).
- Сложность: trivial (shell-скрипт, без Rust-кода).
- Приоритет: medium (влияет на всех пользователей неосознанно, если у них установлен
  `/etc/profile.d/onlyterm.sh`, даже когда они пользуются другим терминалом).

### PR #7694 — Fix error when decode webp images
- Что чинит: встроенный в крейт `image` WebP-декодер поддерживает только lossless-кодирование —
  lossy webp (частый в реальных файлах) не декодируется, ошибка. Фикс — переезд на выделенный крейт
  `image-webp` того же вендора.
- Применимо: **подтверждено** — `onlyterm-gui/src/glyphcache.rs` действительно использует
  `image::codecs::webp::WebPDecoder` (строка ~297), т.е. подвержен той же проблеме.
- Сложность: trivial (замена зависимости/декодера в Cargo.toml + glyphcache.rs).
- Приоритет: medium (background/inline images в lossy webp сейчас не грузятся вовсе).

### PR #4995 — Fix kitty image leaving behind images after removing image placements
- Что чинит: удаление image placement (kitty graphics protocol) может не убрать изображение из
  scrollback, если `StableRowIndex` устарел (из-за ресайза/wrap строк) — "призрачные" изображения
  остаются. Сам автор помечает PR как DRAFT и предупреждает, что предложенный фикс "будет иметь
  серьёзные проблемы с производительностью для больших scrollback и множества обновлений
  изображений".
- Применимо: `term/src/terminalstate/kitty.rs` присутствует.
- Сложность: substantial (готового качественного фикса у автора нет; нужно реализовать иначе, чем в
  диффе).
- Приоритет: low-medium (визуальный баг с известным performance-компромиссом у предложенного
  решения — не переносить бездумно).

### PR #5009 — Fix interaction between pane swapping / rotating and client domains
- Что чинит: свап/ротация панелей работает только на локальных доменах — состояние не
  распространяется на удалённый mux-сервер; попутно чинится опечатка в Lua API (rotate всегда
  крутил в одну сторону).
- Применимо: широкий охват файлов (`mux/src/{domain.rs,lib.rs,tab.rs}`,
  `onlyterm-client/src/{client.rs,domain.rs}`, `onlyterm-gui/src/{frontend.rs,termwindow/mod.rs,
  termwindow/paneselect.rs}`, `onlyterm-mux-server-impl/*`, `lua-api-crates/mux/src/tab.rs`) — все
  присутствуют.
- Сложность: substantial (добавление RPC для ротации/свапа панелей через mux-домен — не тривиальный
  перенос, но код-база позволяет).
- Приоритет: medium (корректность мультиплексирования между клиентами, плюс сам факт "opposite
  direction" бага в текущем Lua/rhai API стоит проверить отдельно).

### PR #7434 — Detect mouse over the wrong panel during URL highlighting
- Что чинит: URL-хайлайтинг под курсором использует номер строки, посчитанный относительно активной
  панели, но это не всегда верно (мультипанельные раскладки) — подсветка URL может сработать не в
  той панели.
- Применимо: `onlyterm-gui/src/termwindow/mouseevent.rs`.
- Сложность: trivial (добавить проверку, что курсор реально над активной панелью).
- Приоритет: medium.

### PR #7491 — Fix open hyperlink bindings with mouse reporting
- Что чинит: `OpenLinkAtMouseCursor` / `CompleteSelectionOrOpenLinkAtMouseCursor` не работают в
  режиме mouse reporting (когда приложение внутри перехватывает мышь, например vim/tmux) для
  дефолтных и пользовательских биндингов.
- Применимо: `onlyterm-gui/src/termwindow/mouseevent.rs`.
- Сложность: trivial.
- Приоритет: low-medium.

### PR #7494 — fix(macos): improve IME preedit handling
- Что чинит: несколько проблем preedit-состояния сторонних (особенно корейских) IME на macOS:
  preedit-текст не коммитится перед движением курсора/кликом мыши, дисплей preedit не обновляется
  при активации IME вне клавиатурных событий.
- Применимо: `window/src/os/macos/window.rs`.
- Сложность: needs adaptation (несколько независимых мест в одном файле).
- Приоритет: medium (некорректный ввод текста для целого класса языков/IME).

### PR #7556 — fix(gui): handle IME committed text in prompt overlays
- Что чинит: оверлеи ввода текста (`PromptInputLine`, например, переименование таба) теряют
  IME-скоммиченный текст (`KeyCode::Composed`) — добавляется `Pane::send_composed_text` для
  правильной маршрутизации.
- Применимо: `mux/src/{pane.rs,termwiztermtab.rs}`, `onlyterm-gui/src/termwindow/keyevent.rs`.
- Сложность: needs adaptation.
- Приоритет: medium (та же категория проблем с CJK/IME вводом, что #7944/#7915/#7494, но в другом
  UI-контексте — оверлеи, а не основной терминал).

### PR #4875 — WIP fix for i3 terminals sending mouse press before focus event
- Что чинит: под i3 (и, вероятно, некоторыми другими WM) клик мышью может прийти раньше события
  фокуса окна; сейчас клик по неактивному окну не долетает до приложения внутри. Автор сам называет
  PR черновым и не уверен в правильности места фикса.
- Применимо: `onlyterm-gui/src/termwindow/mouseevent.rs`.
- Сложность: trivial-ish идея, но требует валидации на реальном i3/подобных WM перед принятием
  (гонка порядка событий, а не гарантированно детерминированный фикс).
- Приоритет: low-medium.

### PR #7730 — Fix IPv6 rule in quick select parsing
- Что чинит: регулярное выражение для quick-select IPv6-паттерна ошибочно: `A-f` (диапазон символов)
  не равно `A-Fa-f` (буквы A-F в обоих регистрах) и вдобавок захватывает лишние символы `\~]^`.
- Применимо: `onlyterm-gui/src/overlay/quickselect.rs`.
- Сложность: trivial (однострочная правка regex).
- Приоритет: low.

### PR #6229 — Fixed mail-to regex allowing email addresses to have dots & hyphen as part of the user segment
- Что чинит: regex для авто-детекции email-адресов в тексте терминала не допускает точки/дефис в
  user-части адреса (`имя.фамилия@...`), хотя это валидный email.
- Применимо: `config/src/config.rs`.
- Сложность: trivial (правка regex).
- Приоритет: low.

### N/A — подсистема удалена в нашем форке (SSH-клиент / freetype-internals)

### PR #7812 — fix(onlyterm-ssh): support tilde expansion and multiple files for include directive
- N/A — SSH-клиент (`onlyterm-ssh`) в нашем форке удалён целиком; ssh_config-парсинг отсутствует.

### PR #7745 — ssh: IdentitiesOnly=yes should filter agent keys, not skip agent auth
- N/A — то же самое, SSH-клиент удалён.

### PR #7739 — fix: IdentitiesOnly and IdentityFile ssh_config(5) / openssh compliance
- N/A — SSH-клиент удалён.

### PR #7378 — RemoteSshDomain: Use `sh -c`, not `$SHELL -c`
- N/A — `RemoteSshDomain` как часть удалённого SSH-клиента отсутствует в дереве.

### PR #7557 — fix(mux): ensure runtime directory exists before creating ssh agent symlink
- N/A — форвардинг SSH-агента является частью удалённой SSH-подсистемы.

### PR #7736 — Fix builds on MSVC aarch64 environments by removing GNU assembly NEON stub
- N/A — правка находится в `deps/freetype/build.rs`; каталог `deps/freetype` в нашем форке
  отсутствует полностью (freetype заменён на swash/rustybuzz), файл `git ls-files` не находит.

## Сравнение с параллельной pure-Rust rendering инициативой апстрима (#7607/#7608/#7609)

### Что предлагают апстримные POC

- **#7607 "POC: pure-Rust font stack"** заменяет весь C-стек шрифтов/рендеринга:
  FreeType → **swash** (skrifa + zeno) для растеризации глифов (контуры, subpixel LCD, цветной
  emoji, bitmap-шрифты); HarfBuzz → **harfrust** для шейпинга текста; fontconfig →
  **fontdb + fontconfig-parser** (чистый Rust, парсит те же XML-конфиги fontconfig, но без
  линковки на саму C-библиотеку) для обнаружения шрифтов на Linux; Cairo → **tiny-skia** для
  рендеринга COLR цветных шрифтов. Диф в основном состоит из удаления ~131 МБ / ~269K строк
  вендоренного C-кода (весь `deps/cairo/cairo/src/cairo-*.c` и аналоги для freetype/harfbuzz).
  Явно помечен автором как "not production-ready", "for discussion".
- **#7608 "POC: PureCpu software renderer"** (стек поверх #7607) добавляет вариант
  `front_end = "PureCpu"`, который полностью обходит GPU: композитинг глифов из
  in-memory texture atlas напрямую во framebuffer и презентация через `XPutImage` на X11.
  Целевой сценарий — безGPU-окружения (VNC, SSH X11 forwarding, remote desktop).
- **#7609 "POC: dirty-region tracking and idle skip for PureCpu renderer"** (ещё один слой поверх
  #7608) добавляет отслеживание "грязных" строк по seqno панели/движению курсора/скроллу
  вьюпорта/изменению выделения/конфига и ранний выход из `paint_impl`, когда ничего не изменилось —
  снижая простойное CPU-потребление почти до нуля.

### Наш подход (уже сделан, не POC, в проде)

- **Шейпинг**: `onlyterm-font/src/shaper/rustybuzz.rs` — используем **rustybuzz** (чистый Rust порт
  HarfBuzz), а не harfrust (более новый, отдельный от HarfBuzz-семейства проект). rustybuzz —
  консервативный выбор: he максимально совместим по API/поведению с оригинальным HarfBuzz (это
  прямой построчный порт C++ кода), что снижает риск регрессий в сложных скриптах (арабский,
  индийские письменности, лигатуры). harfrust — более новая, написанная "с нуля" на Rust реализация
  от той же команды rustybuzz (те же авторы мигрировали проект), заявлена как более быстрая и
  идиоматичная, но менее "проверена боем" на момент разбора. **Вывод**: наш выбор rustybuzz
  консервативнее и безопаснее; апстримный harfrust потенциально быстрее — стоит держать это в поле
  зрения как будущую опцию для замены, но не как немедленную необходимость.
- **Растеризация**: `onlyterm-font/src/rasterizer/swash.rs` + `swash_metrics.rs` — тоже **swash**,
  то есть в этой части наш выбор идентичен POC #7607. Значит, для растеризации мы уже пришли к тому
  же решению, которое апстрим только предлагает как эксперимент — мы здесь не "отстаём", а по факту
  уже в проде используем то же самое решение, которое там всё ещё experimental/POC.
- **2D-рисование/композитинг (замена Cairo)**: `onlyterm-font/src/rasterizer/paint.rs` — наш
  собственный `Painter`, реализующий cairo-совместимый API (save/restore, transform, path-builder,
  push_group/pop_group, clip-маски) поверх **tiny-skia**, с явным протоколом "dry-run для расчёта
  ink extents → второй проход в реальный Pixmap" (комментарий в файле подробно объясняет, почему
  это нужно — у tiny-skia нет аналога `cairo::RecordingSurface::ink_extents()`). Это значительно
  более глубокая и продуманная работа, чем то, что описано в теле #7607 (которое просто перечисляет
  замену библиотек без объяснения, как воспроизведён API cairo). **Мы прошли этот путь дальше и
  тщательнее**, чем показывает POC.
- **COLR цветные шрифты**: у нас есть отдельные `rasterizer/colr.rs` и `rasterizer/colr_paint.rs` —
  специализированная обработка COLR (colored font) глифов, аналогично тому, что упоминает POC
  ("Cairo for COLR color font rendering"), но реализовано отдельным модулем, а не как часть общего
  Painter.
- **Fontconfig**: здесь у нас **не полное соответствие POC** — `deps/fontconfig/` в нашем форке
  по-прежнему **линкуется с системной C-библиотекой fontconfig** через `pkg-config` (см.
  `deps/fontconfig/build.rs`, `onlyterm-font/src/fcwrap.rs` — это FFI-обёртка над `FcFontSet` и
  т.п., не переписана на чистый Rust). Мы удалили freetype/harfbuzz/cairo как C-зависимости, но
  **fontconfig как единственная оставшаяся C-библиотека всё ещё используется** (только для
  Linux-поиска шрифтов на диске). **Это единственная конкретная идея из POC #7607, которую мы
  реально упустили**: переход на `fontdb + fontconfig-parser` избавил бы нас от последней
  оставшейся линковки с C-кодом в шрифтовом стеке. Если цель форка — "убрать все C/C++/asm"
  полностью (как заявлено в описании задачи), то этот участок ещё не завершён и стоит взять на
  заметку как отдельную будущую задачу (не багфикс, а хвост уже начатой миграции).
- **Software/CPU-рендеринг без GPU (сравнение с #7608/#7609)**: у нас **уже существует** третий
  вариант в `config::FrontEndSelection` — `Software` (наравне с `OpenGL` и `WebGpu`), используемый,
  в частности, автоматически при обнаружении RDP-сессии на Windows
  (`window/src/configuration.rs::prefer_swrast()`, при `is_running_in_rdp_session()`). Однако это
  **не то же самое**, что предлагает #7608/#7609: наш `Software` — это, по всей видимости, glium
  software-rasterization fallback (OpenGL software rendering через существующий glium/OpenGL путь,
  без GPU-драйвера), а не полностью отдельный "no-GPU-at-all" пайплайн с прямой презентацией через
  `XPutImage`, который строит #7608. Идея #7608/#7609 (полностью развязать from GPU/EGL/Vulkan
  вообще, с ручной презентацией framebuffer через `XPutImage`, плюс dirty-region tracking) —
  **потенциально применимая, но не реализованная у нас идея**, актуальная в первую очередь для
  сценариев VNC / SSH X11 forwarding / remote desktop без GPU, где даже glium software rasterizer
  всё ещё зависит от наличия OpenGL-контекста (пусть и software-реализованного через Mesa llvmpipe
  и т.п., что не всегда доступно в headless/remote окружениях). Идея dirty-region tracking + idle
  skip из #7609 отдельно интересна сама по себе (снижение простойного CPU) независимо от PureCpu —
  стоит проверить, есть ли у нас уже подобный early-exit в текущем `paint_impl` для
  OpenGL/WebGpu-путей; если нет, это самостоятельная performance-идея, которую можно перенять вне
  контекста PureCpu-рендерера.

### Итоговая оценка

Мы **опережаем** апстримную инициативу в части font-стека (rustybuzz+swash+tiny-skia уже в проде,
не POC) и предметно проработали cairo→tiny-skia API-совместимость глубже, чем показано в диффе POC.
Единственный реальный технический долг, который POC высвечивает и который у нас не закрыт —
**fontconfig как последняя C-зависимость** (можно закрыть переходом на `fontdb` +
`fontconfig-parser`, по образцу #7607). Идеи #7608/#7609 (PureCpu без GPU вообще + dirty-region
tracking) у нас не реализованы и являются самостоятельными фичами/оптимизациями, а не багфиксами —
не подпадают под критерий "нужно исправить баги", но могут быть интересны отдельно как будущая
фича (особенно dirty-region idle-skip — это чистый perf-выигрыш, применимый независимо от бэкенда).

## Неважное — фичи/косметика/доки/deps (206 штук: 203 обычных + 3 POC, разобранных выше отдельно)

#7976 cargo: revert edition to 2021
#7975 chore(deps): update spin to version 0.9.9
#7973 chore(deps): upgrade thiserror to 2.0
#7971 Add directional pane swapping
#7968 Add pane_padding: a configurable gutter between split panes
#7964 Add a Command background source: run any command as a live window background
#7962 Fix window_frame border color missing at rounded macOS corners
#7956 Install desktop entry and icons in Linuxbrew formula
#7948 macOS: compute non-native fullscreen notch inset from the window's own screen, not mainScreen
#7946 macos: bounce dock icon on bell to request user attention
#7941 macos: re-apply non-native fullscreen frame on display/resolution changes
#7938 build(deps): bump serde_with from 2.3.3 to 3.21.0
#7933 Add win32_system_backdrop_keep_inactive: keep the active backdrop material on unfocused windows
#7931 ALT Linux build initial support
#7926 fix: repaint tab bar during drag reorder + floating drag preview (builds on #6527)
#7925 feat(search): defer search updates to navigation and preserve viewport
#7924 feat(term): support kitty graphics protocol unicode placeholders
#7923 docs: document escape sequences that are handled but were undocumented
#7903 feat(lua): expose link/mouse position under cursor; OpenLinkAtMouseCursor fallback
#7878 feat: add `ReconnectDomain` key assignment for reconnecting remote domains
#7855 feat: add actions to change font size by specified unit
#7853 clear-screen persistence on resize
#7792 Fix the titlebar on macos after exiting fullscreen
#7850 build(deps): bump `harfbuzz` to 14.2.1
#7849 don't move the pane to a new window if it's already the only pane in the window
#7848 windows: add win32_keep_system_backdrop_when_inactive option
#7839 build(deps): bump actions/checkout from 3 to 6.0.2
#7838 build(deps): bump stefanzweifel/git-auto-commit-action from 5 to 7
#7837 build(deps): bump actions/upload-pages-artifact from 3 to 5
#7836 build(deps): bump actions/cache from 4 to 5
#7834 build(deps): bump openssl from 0.10.75 to 0.10.80
#7828 Fix hyperlink hover underline for identical text with different links
#7826 x11: handle horizontal scroll wheel (buttons 6 and 7)
#7823 Add opt-in forwarding for macOS marked text
#7820 Add Wine build instructions for onlyterm-gui
#7816 Fix stale mouse move restoring hidden cursor and URL hover
#7809 tab bar: add tab_bar_width config to stretch tabs across the bar
#7789 filedescriptor: Add `into_stdio` conversion
#7786 launcher: select the active workspace/tab by default
#7781 refactor: Update unicode constants and tables
#7778 feat(ssh): support Match exec, GlobalKnownHostsFile, and two-phase parsing
#7775 Update bundled Windows ConPTY to 1.24.260402001
#7767 flatpak: bump runtime to 24.08
#7766 docs: example for adjusting window opacity incrementally
#7764 docs: clarify chocolatey install requires admin shell
#7762 fix: update Se terminfo entry to reset cursor to configured default
#7760 build(deps): bump rustls-webpki from 0.103.8 to 0.103.13
#7757 build(deps): bump rand from 0.8.5 to 0.8.6
#7754 Add NocturnalZone color scheme
#7749 Fix tab:set_title flashing in unix domain mode
#7741 feat: ranking for InputSelector items
#7737 feat: add opt-in cursor trail and smear effects (closes #7387)
#7734 Upgrade wgpu 25.0.2 to 27.0.1 for FreeBSD support
#7723 feat: implement mode CSI 2031 (color appearance reporting)
#7714 ci: update nightly winget manifest after each nightly build
#7712 Improve braille test data 😉
#7699 feat: allow matching windows paths in Quick Select Mode
#7690 feat(termwiz): add dynamic color probing
#7687 clear-screen persistence on resize; add broadcast input to tab panes
#7683 fonts: add configurable font thickening
#7682 Vim-like cursor navigations
#7679 Add vertical tab bar with drag-to-resize and left/right positioning
#7674 Fix text_min_contrast_ratio
#7673 feat: add per-pane title bars
#7669 feat: add AVIF image support via dav1d decoder
#7658 Fix transparent window flash on Windows during initial rendering
#7654 fix: restore tab size after top-level split
#7649 Add WebGPU post-processing shader pipeline
#7643 Focus the originating pane/tab when a toast notification is clicked
#7637 Add window-tab-switched event for Lua config
#7633 build(deps): bump getrandom from 0.3 to 0.4
#7624 Add clipboard image paste support with Ctrl+V smart paste for all platforms
#7622 Use NSVisualEffectView for Window background blur on macOS
#7618 Implement tab:move_to_window method to allow moving tabs between windows using Lua
#7606 feat: add `Child::main_thread_handle`
#7605 feat: add option to disable usage of `PSUEDOCONSOLE_INHERIT_CURSOR`
#7604 feat: add `CommandBuilder::creation_flags`
#7596 generate 256 palette
#7592 windows: add native ARM64 build, packaging, and runtime support
#7591 feat: stretch fancy tab bar tabs to fill available width
#7585 docs: typo on 8-bit escape sequence recognition
#7580 chore: use `LazyLock` instead of `lazy_static`
#7574 Add vertical tab bar support (Left/Right positioning)
#7566 fancy tab bar: fix layout issues with macOS native traffic light buttons
#7563 build(deps): bump time from 0.3.44 to 0.3.47
#7559 build(deps): bump git2 from 0.20.2 to 0.20.4
#7558 kitty: improve animated image handling
#7555 fix: set DBus notification priority to normal
#7554 build(deps): bump bytes from 1.10.1 to 1.11.1
#7539 Change display order of `key` and `mode` in `show-keys --lua`
#7526 fix(onlyterm-gui): max tab title length
#7510 termwindow: emit pane-focus-changed window event
#7500 Add new window_close_mux_behavior config option
#7493 feat(macos): add services for opening folders
#7472 feat: add custom post-processing shader support for WebGPU backend
#7464 Add Quic transport to mux
#7453 fix macOS dictation support with new color options to customize the preview text
#7449 Add config option for debounce when searching
#7440 docs(config/launch): use vswhere for Visual Studio launch_menu example (works with VS 2026 / 18.x layout)
#7432 Update linux brew install instructions to use formula rather than cask
#7420 Cursor trail
#7397 Linux install guide: Better place to save APT key
#7390 Alpine: Add static libraries for OpenSSL and zlib
#7385 feat: smart case search
#7381 docs: replace literal `\n` with `<br/>` in flowchart labels
#7379 onlyterm ssh: add `--assume-shell` argument
#7360 wayland: Show client-side decorations when compositor doesn't provide server-side
#7357 Implement hide window operation for x11 windows
#7352 Support background image blurring
#7349 Add bell_urgency_hint config option and window:request_attention() API
#7338 Removed "start --cwd ." from Exec=onlyterm in onlyterm/assets/onlyterm.desktop
#7312 Abandon kitty's legacy key event encoding
#7296 Add PowerShell integration script
#7284 docs: Improve get_progress example circle rendering
#7282 Added lua function get_logical_lines_as_escapes
#7216 Implement get_semantic_zones() for ClientPane
#7202 fix(units): allow GuiPosition to parse pt and cell units
#7200 feat(terminal): Implement dynamic DECRQSS and DECRQM cursor queries
#7199 Fix: left status bar overlaps with window buttons (#7197)
#7195 Fix mermaid newlines in what-is-a-terminal.md
#7193 feat(macos): new MACOS_DISABLE_TITLEBAR_DRAG window decoration and refactor macOS window decoration logic
#7191 Improve scroll handling: add gesture fan-out and precise scroll scale
#7186 build(deps): bump actions/upload-pages-artifact from 3 to 4
#7185 build(deps): bump actions/checkout from 3 to 5
#7184 build(deps): bump actions/download-artifact from 4 to 5
#7179 added configurable glow effect
#7178 pty!: Fix typo: psuedo => pseudo
#7170 build(deps): bump xcb from 1.5.0 to 1.6.0
#7161 Add CopyMode overlay to TermwizTerminalPane overlays
#7160 onlyterm-ssh: support ProxyJump in ssh config
#7151 build(deps): bump slab from 0.4.10 to 0.4.11
#7146 FEATURE: Add metadata for cargo-deb build
#7140 copy_mode: implement `MoveToBlankLine`
#7131 Add function to percent encode string
#7124 docs: fixed some of the Lua documentation
#7095 adopt sctk-adwaita as our frame on wayland
#7093 Open in new tab setting in Nautilus integration
#7065 build(deps): bump stefanzweifel/git-auto-commit-action from 5 to 6
#7059 Disable '*_continuous' workflows for forks
#7035 Add copy mode cursor configuration options
#7032 chore: correct mermaid mistakes in "what is a terminal"
#7030 Fix: Added logic to correct seams in repeating backgrounds. (Issue #6335)
#7020 termwiz: add links on types in main doc page
#7005 Add JumpToMatchingBracket CopyMode KeyAssignment
#6999 Add SearchForwardRelativeToCursor, SearchBackwardRelativeToCursor keyassignments to CopyMode
#6973 Proposal: alternative minimum contrast algorithm
#6937 WIP: Makefile `servedocs` target with live reload; reorder subsections in "Config" and "Colors & Appearance"
#6878 feat: support left/right mods for keybindings
#6876 Introduce `win32_window_appearance` to override Windows setting
#6856 Support SGR DECRQSS
#6821 lua-api-crates/mux: expose swap_active_with_index
#6756 OnlyTerm Shell Context Menu for Windows 11
#6657 Improve dragging and double-clicking for maximize on the tab bar
#6610 onlyterm-ssh: make SshPty fields public
#6533 Improve Wayland scroll behavior
#6527 feat: reorder tabs via left mouse drag
#6511 Fixed cursor blinking on transparent backgrounds
#6445 adds movement for start of word, regardless of stop characters
#6324 add support to align tabs to the right
#6292 Add support for OSC 22 control sequence to change mouse cursor shape
#6239 feat: implement opt-in OSC52 clipboard querying
#6185 adds HideOtherApplications (for macOS)
#5997 Enhanced vi-like word motions in copy mode
#5995 Attempt to implement AccessKit inside the window crate
#5972 Add command to check config values
#5969 Fix ghost space left by SplitPane with top_level=true
#5941 WIP: add logging to help investigate incorrect terminal size on macOS
#5894 Support custom cargo build dirs on MacOS.
#5850 feat: Identify modifiers on Wayland/X11 (new)
#5820 Add plugin aliases
#5780 Simplify wayland configure event
#5779 onlyterm-ssh: add support for comments in `Match` statements
#5573 replace all 'psuedo' by 'pseudo'
#5567 Hotfix/search overlay move to end of line
#5452 doc: Use more stable format when first describing SGR sequence
#5390 Decorations rework
#5373 Remove nix bits from repo and CI
#5229 equivalent tab bar styling example with current api
#5184 feat(ssh/config): handle absolute path includes in ssh config
#5104 Add Wayland Scroll Factor
#5096 Line wrapping
#5035 [feature] tab min width
#5013 [Feature] Resurrection: saving and restoring terminal layout/contents/commands
#5002 fix(flatpak): Remove cwd from desktop entry
#4951 termwiz: Kitty keyboard enhancements support
#4767 Expose as_command API on CommandBuilder
#4727 Improve integrated titlebar buttons on macos
#4635 Windows rs
#4493 Path and MetaData objects + `onlyterm.<functions>`
#4413 Fix Gnome window buttons DPI scaling
#4393 Start line selection at 0th cell
#4336 Adding some functions to work with Lua tables.
#4334 Scroll viewport to bottom after erase scrollback
#4277 Added spawn_command_as_user to allow user impersonation when spawning processes into the Pty
#4248 Plugin package path
#4221 Add move_pane_id parameter to pane:split lua api
#4093 Add force_reverse_video_selection config
#4043 Fixes #4035: Make hyperlink rule match links that contain emojis
#3737 packaging: Add Snap build
#3579 Add config option for mouse cursor when grabbing
#3006 Add get_position lua method for window
#2608 Add a State parameter to Ui that's passed to all widget calls
#2056 IME Selected String
