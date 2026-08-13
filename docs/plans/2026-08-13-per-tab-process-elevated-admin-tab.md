# План: per-tab процессная изоляция + админ-вкладка в том же окне

Дата: 2026-08-13. Задачи: **#550–#552** (дизайн → @oh-ревью → таски по ревью),
дальнейшие фазы реализации заводятся отдельными `/babygoal`-раундами по мере
готовности, каждая — со своим номером задачи.

## 0. Формулировка цели и что в ней требует уточнения

Пользователь попросил: «доведи задачу до конца "на каждую вкладку свой
процесс", потом реализуй, чтобы новая админская вкладка открывалась в том же
окне».

Первая часть требует поправки по факту, зафиксированной здесь письменно (не
только в диалоге), чтобы не потерять при `/resume`/чекпоинте:

**Шелл каждой вкладки уже сегодня — отдельный процесс.** `cmd.exe` вкладки A
и `powershell.exe` вкладки B — разные PID, так было всегда; это не
недоделанная часть. НЕ отдельным процессом является сторона
терминала-эмулятора (хостинг PTY, рендеринг, вкладки) — все окна/вкладки
OnlyTerm живут в одном `onlyterm-gui.exe`. Именно это в
`docs/plans/2026-07-29-execution-decoupling.md:152-157` названо «Уровень E»
и никогда не было реализовано (трекалось как #224/#258) — но там это
масштабировано на **окно**, а не на вкладку.

Вторая часть — реальная, конкретная цель: админ-вкладка должна открываться
**в существующем окне пользователя**, а не в отдельном окне (как сейчас,
`crates/wezterm-gui/src/elevate.rs`, task #547).

## 1. Жёсткий платформенный факт (не пересматривается)

`CreateProcess` — единственный API, принимающий хэндл ConPTY через
`PROC_THREAD_ATTRIBUTE_HANDLE_LIST` — не умеет запрашивать элевацию.
`ShellExecuteExW("runas")` — единственный штатный способ показать UAC — не
принимает произвольные хэндлы для наследования. Значит невозможно **напрямую**
создать elevated-процесс, который сразу владел бы ConPTY, унаследованным от
текущего medium-integrity `onlyterm-gui.exe`. Это подтверждено в task #545 и
использовано в #547 (`elevate.rs`) — там же зафиксировано, что Windows
Terminal с `elevate: true` решает это ровно так же, как мы: отдельное окно.

Значит, чтобы elevated-шелл оказался вкладкой в существующем окне, ConPTY
должен принадлежать **отдельному elevated-процессу**, а исходное окно — лишь
**отображать** его пейны. Это не обходной путь вокруг факта из абзаца выше, а
его прямое следствие.

## 2. Рекомендуемая архитектура: переиспользовать существующий mux-client/server, не изобретать заново

Параллельное исследование (агент, 2026-08-13) подтвердило: `crates/wezterm-mux-server`,
`wezterm-mux-server-impl`, `wezterm-client`, `codec` — не мёртвый код с апстрима,
а **реально используемая сегодня** машинерия. GUI уже на каждом старте
регистрирует `ClientDomain` для домена `"unix"`
(`crates/wezterm-mux-server-impl/src/lib.rs:19-25`, вызывается из
`wezterm-gui/src/main.rs:252,564`), команда `onlyterm-gui.exe connect` и
`KeyAssignment::AttachDomain` уже реально работают
(`main.rs:124-125`, `termwindow/actions.rs:1484-1499`). `ClientDomain::spawn`
(`crates/wezterm-client/src/domain/mod.rs:647-693`) шлёт `SpawnV2` по проводу
и заворачивает ответ в обычный `Tab`/`ClientPane`
(`crates/wezterm-client/src/pane/clientpane.rs:34-52`) через ТЕ ЖЕ
`mux.add_tab_and_active_pane`/`add_tab_to_window`, что и у `LocalDomain` — то
есть с точки зрения окна и вкладочной панели это неотличимо от обычной
локальной вкладки уже сегодня.

Это значит: тяжёлая, нетривиальная часть задачи («показать пейн чужого
процесса как обычную вкладку в этом окне») уже решена и обкатана в этом форке.
Не нужно изобретать протокол/рендеринг с нуля.

**Решение пользователя (2026-08-13, подтверждено явно через уточняющий
вопрос):** полная изоляция для ВСЕХ вкладок, не только admin. Мотивация —
не производительность, а **blast radius краша**: если хостинг вкладки
падает (паника, UB, любой процессный фолт), это не должно ронять ничего,
кроме самой вкладки. Это меняет масштаб раздела относительно первоначальной
рекомендации (которая ограничивала изоляцию только elevated-случаем по
соображениям производительности) — записываю почему это меняет картину:

Уровни A–D из `docs/plans/2026-07-29-execution-decoupling.md` изолируют
**зависания GPU-вызовов** через отдельный рендер-поток на окно — это НЕ
изолирует крах самого процесса `onlyterm-gui.exe` (паника в любом месте,
UB, segfault и т.п. по-прежнему роняет весь процесс, все окна, все вкладки).
Требование пользователя — про этот, более широкий класс отказов, который
уровни A–D не покрывают в принципе (они про конкретно GPU present/submit,
не про весь процесс). Значит полная процессная изоляция — не избыточность
поверх уже сделанного, а закрытие реального пробела.

**Важное архитектурное следствие этого решения:** «каждая вкладка — свой
процесс» для настоящей изоляции крахов означает буквально **один хостящий
процесс на вкладку/пейн**, а не один общий «local mux-server» для всех
обычных вкладок разом (общий процесс воспроизвёл бы сегодняшний blast radius
один в один, просто в другом процессе). Сегодняшний `wezterm-mux-server`
(`--daemonize`) спроектирован как один долгоживущий демон, мультиплексирующий
произвольное число пейнов — это модель, ПРОТИВОПОЛОЖНАЯ требуемой. Фаза A
(раздел 4) поэтому теперь включает разработку "single-pane" режима хостинг-
процесса: один инстанс = один PTY/пейн, завершается вместе с ним. Это
касается equally обычных и elevated вкладок; elevation остаётся лишь
частным случаем ("тот же per-pane процесс, но заспавненный через
`ShellExecuteExW(\"runas\")` вместо обычного `CreateProcess`").

Цена по производительности (сериализация ввода/вывода по IPC для КАЖДОЙ
вкладки, не только admin) признаётся сознательно и не пересматривается —
это прямой компромисс за изоляцию крахов, который пользователь запросил
explicitly.

## 3. Что нужно достроить (новое, не переиспользование)

Честно, по находкам исследования — вот что реально отсутствует:

1. **Elevation-триггер спавна mux-server.** Сегодняшний путь подключения
   (`crates/wezterm-client/src/client/conn.rs:153-220`, `unix_connect` →
   при неудаче `unix_dom.serve_command()` → retry) спавнит
   `onlyterm-mux-server.exe --daemonize` обычным `Command::spawn`, что не
   умеет UAC. Нужен elevation-вариант этого пути: `ShellExecuteExW("runas")`
   вместо `Command::spawn`, с той же обработкой `ERROR_CANCELLED`, что уже
   есть в `elevate.rs:162-176`.
2. **ACL сокета — на сегодня отсутствует полностью.** Транспорт —
   unix-domain-socket-эмуляция на Windows через `uds_windows`
   (`crates/wezterm-uds`), путь по умолчанию — файл в `RUNTIME_DIR`.
   `create_user_owned_dirs` (`crates/config/src/lib.rs:134-139`) и
   `set_sticky_bit` (`crates/config/src/daemon.rs:17-21`) на Windows —
   **no-op**. Значит подключиться к сокету сегодня может ЛЮБОЙ процесс
   текущего пользователя. Для elevated mux-server это неприемлемо as-is:
   любой процесс от имени пользователя получил бы admin-шелл без UAC.
   Обязательно закрыть ДО того, как это уйдёт в прод-путь: явный DACL на
   файл сокета/каталог через `SetNamedSecurityInfoW`, ограничивающий
   доступ текущим SID (плюс, возможно, admin-группой, раз сам процесс
   elevated).
3. **Аутентификация — отсутствует.** Апстримовые PKI/TLS-механизмы были
   удалены из этого форка (`docs/plans/2026-07-23-remove-ssh-tls-feature.md`).
   Сегодня «кто подключился — тот и доверенный клиент». Решить: достаточно
   ли жёсткого ACL на сокет, или нужен ещё minimal-handshake (например,
   общий секрет, сгенерированный при спавне и переданный только через
   защищённый канал — например, unix-сокет уже сам по себе даёт то же самое,
   что и файловый ACL, так что при полностью корректном ACL отдельный
   хендшейк может быть избыточен — но это должно быть явным решением, а не
   умолчанием по недосмотру).
4. **Lifecycle.** Сегодняшний `--daemonize`/`serve_command` предполагает
   «запустить один раз и жить бесконечно», а не «запустить при первой
   admin-вкладке, завершиться, когда закрылась последняя». Нужен: триггер
   остановки (mux-server отслеживает количество живых пейнов в своём домене и
   завершается сам, либо GUI явно шлёт shutdown-PDU / убивает по PID,
   зафиксированному в момент собственного спавна — здесь напрямую действует
   правило проекта из CLAUDE.md про безопасность процессов: элевейтед
   mux-server — это процесс, который запускает СЕССИЯ/ПОЛЬЗОВАТЕЛЬ через этот
   же путь, поэтому его закрытие по точному PID, зафиксированному при
   спавне, — штатно и разрешено; трогать любой ДРУГОЙ `onlyterm*.exe` —
   всё так же запрещено).
5. **Проводка UI.** Выбор "Admin" в New Tab Options
   (`crates/wezterm-gui/src/termwindow/newtab_options.rs`, task #548) должен
   не звать `elevate::spawn_elevated_window` (текущий путь — целое отдельное
   окно), а attach-or-spawn elevated mux-домен и затем `SpawnV2` в нём.

## 4. Фазы реализации (каждая — отдельный будущий `/babygoal`)

- **Фаза A.** Single-pane режим хостинг-процесса: новый режим запуска
  `onlyterm-mux-server.exe` (или отдельный компактный бинарь на общем коде
  `wezterm-mux-server-impl`) — один инстанс = один PTY/пейн, никакого
  мультиплексирования нескольких вкладок внутри одного процесса; процесс
  завершается вместе со своим пейном. ACL'd сокет (или, как менее
  ACL-чувствительная альтернатива — `proxy_command`/`socketpair()`-транспорт,
  `crates/config/src/unix.rs:36-39`, `conn.rs:36-83`, который вообще не
  требует файлового ACL, раз хэндлы наследуются напрямую при спавне — оценить
  оба варианта на этой фазе, не решать заранее). Критерий приёмки: обычная
  (не admin) вкладка cmd.exe хостится в собственном процессе, видна как
  обычная вкладка в окне, и целенаправленный краш этого хостинг-процесса
  (например, тестовый abort) не роняет ни окно, ни другие вкладки.
- **Фаза B.** Генерализация: ВСЕ новые вкладки (обычный `SpawnTab`, не
  только New Tab Options) идут через путь фазы A вместо прямого
  `LocalDomain`. Обновить/удалить прямой `LocalPane`-путь для интерактивных
  вкладок пользователя (не трогать служебные TermWizTerminal-оверлеи вроде
  Ctrl+I/Command Palette — они не являются "вкладками пользователя" и не
  подпадают под требование).
- **Фаза C.** Elevation поверх фазы A: тот же single-pane процесс, но
  заспавненный через `ShellExecuteExW("runas")` вместо `CreateProcess`, с
  той же обработкой отказа/ошибки UAC, что уже есть в `elevate.rs`. Проводка
  "Admin" в New Tab Options на этот путь вместо сегодняшнего
  `spawn_elevated_window` (whole-window). Whole-window путь не удаляется
  сразу — остаётся fallback на случай сбоя attach/spawn.
- **Фаза D.** Lifecycle и восстановление после краха: что видит GUI и что
  показывает пользователю, если single-pane хостинг-процесс любой (не
  только admin) вкладки упал — вкладка должна показать понятное состояние
  "процесс завершился"/ошибку, а не зависшую пустую вкладку; PID каждого
  спавненного хостинг-процесса фиксируется в момент спавна для безопасного
  закрытия по точному правилу из CLAUDE.md.
- **Фаза E.** Security hardening pass: ACL или обоснованный выбор
  `proxy_command`-транспорта без ACL, минимальность командной поверхности,
  отдельно — для elevated-случая (тот же риск privilege escalation, что и
  раньше, теперь актуален для КАЖДОЙ вкладки, если выбран socket-транспорт).
- **Фаза F.** Замеры производительности (сериализация ввода/вывода по IPC
  для каждой вкладки — это осознанная цена, но её размер надо измерить, не
  предполагать) + документация + расширение QA-матрицы задачи #549.

## 5. Честная оценка масштаба и риска

Это уже не «elevation-only доделка», а **полноценный перевод модели хостинга
вкладок** с in-process (`LocalDomain`/`LocalPane`) на per-pane процессы через
существующую mux-client/server машинерию, применённый ко всем вкладкам, а не
только admin. Многодневная-многонедельная работа, не рефакторинг с нуля —
показ чужого процесса как вкладки уже реализован и обкатан в этом форке,
но нужно: новый lifecycle-режим хостинг-процесса (single-pane, не
today's long-lived daemon), генерализация точки спавна вкладок, elevation
поверх того же пути, обработка краха/восстановления на КАЖДОЙ вкладке (не
только admin), транспортная безопасность, и честный замер цены по
задержке/CPU на каждый keystroke/repaint по сравнению с сегодняшним
in-process путём. Ревью `@oh` (task #551) обязано отдельно проверить и
безопасность транспорта, и то, что генерализация не создала регресс
отзывчивости для обычного (некогда самого частого) сценария использования.

**Честная оговорка:** упоминавшееся ранее в диалоге решение «gsudo не
нужен» не нашлось ни в одном чекпоинте/плане/коммите этого репозитория —
видимо, разговор не был зафиксирован письменно. Переоценка тут не требуется:
предложенная архитектура (переиспользование уже существующего
mux-client/server) не пересекается с gsudo и не требует стороннего
инструмента.

## 6. Решение пользователя по scope (закрыто)

Полная процессная изоляция для ВСЕХ вкладок подтверждена явно (см. раздел 2)
— причина: краш хостинга вкладки не должен затрагивать ничего, кроме самой
вкладки. Цена по производительности принята сознательно. Вопрос закрыт,
дальнейшие фазы реализации исходят из этого без дальнейших уточнений scope.

## 7. Phase A findings (crush, 2026-08-13)

### Transport Mechanism Comparison

#### Option 1: ACL'd Unix Domain Socket

**Security properties:**
- Currently **INSECURE on Windows**: any process running as the same user can connect (no ACL enforcement)
- Would require implementing real Windows DACL via `SetNamedSecurityInfoW` to restrict socket file to current user's SID
- Filesystem object that could be discovered and tampered with by malicious processes
- Socket file path is discoverable in the filesystem (both a feature for debugging and a risk)

**Implementation complexity:**
- Need to implement Windows DACL setting (non-trivial — requires understanding Windows security descriptors, SIDs, ACLs)
- Need to manage socket file lifecycle (creation, cleanup on exit, handling crashes, stale file cleanup)
- Need to handle concurrent access (only one GUI should connect to a single-pane process)
- Existing code in `crates/wezterm-uds/src/lib.rs` wraps `uds_windows` but has no ACL enforcement
- Client connection path already exists via `UnixStream::connect`

**Pros:**
- Reuses existing UDS infrastructure
- Familiar pattern from Unix/Linux world
- Socket file is discoverable via filesystem (useful for debugging/monitoring)

**Cons:**
- Windows DACL implementation is complex and error-prone
- Security-sensitive code — bugs could allow unauthorized access from other user processes
- Filesystem object management adds complexity (cleanup, permissions, concurrent access)
- Socket file could be left behind on crashes (stale endpoints)
- Authentication is filesystem-path-based, not process-based

#### Option 2: proxy_command / socketpair() Transport

**Security properties:**
- **INHERENTLY SECURE**: no filesystem object, so no ACL surface to get wrong
- Socket handle is inherited ONLY by the specific child process at spawn time via `socketpair()`
- No other process can connect to the socket (handle is not exposed to any other process)
- Process-based security rather than filesystem-based — the handle is exclusively owned

**Implementation complexity:**
- Already fully implemented in `crates/wezterm-client/src/client/conn.rs` lines 36-83
- Uses `filedescriptor::socketpair()` to create paired socket handles
- Child process gets one handle via `cmd.stdin(b.as_stdio()?)` and `cmd.stdout(b.as_stdio()?)`
- Parent keeps the other handle and returns it as `UnixStream::from_raw_socket(a.into_raw_socket())`
- No filesystem object to manage, no permissions to set, no cleanup code needed
- Connection is implicit at spawn time — no retry logic, no "does socket exist" checks

**Pros:**
- Zero ACL work — inherit-only handles provide strong security guarantees by construction
- No filesystem management (no socket file, no path, no permissions)
- Existing code path already works and is production-tested
- Simpler lifecycle: when child exits, socket closes automatically (no cleanup code)
- No stale endpoints (socket dies with the processes)

**Cons:**
- Requires using `proxy_command` field on `UnixDomain` (already exists in config)
- Need to implement a `--single-pane` mode flag in the hosting process (trivial)
- Connection is only possible at spawn time (no post-start discovery — this is a feature, not a bug)

### Recommendation: Use proxy_command / socketpair() Transport

**Decision**: Use Option 2 (proxy_command / socketpair() transport) for Phase A.

**Reasoning**:

1. **Security by construction**: Socket handles are inherit-only and exclusively owned. No other process can connect — not just "no ACL bug," but "no ACL surface at all." This is fundamentally stronger than any DACL-based approach, where implementation bugs could allow unauthorized access.

2. **Implementation simplicity**: The code path already exists and is fully functional. No Windows DACL code needed. No filesystem management. No cleanup logic. The hosting process only needs a `--single-pane` flag to distinguish its lifecycle (exit when pane exits vs long-lived daemon).

3. **Lifecycle simplicity**: When the child process exits, the socket closes automatically. No stale file cleanup. No "is this socket file still valid?" checks. The model maps directly to the single-pane use case: process life = pane life.

4. **No hidden complexity**: Option 1 would require:
   - Windows DACL implementation (complex, error-prone)
   - Socket file lifecycle management (creation, permissions, cleanup, crashes)
   - Concurrent access protection (only one GUI should attach)
   - All of these are security-sensitive code paths

   Option 2 requires:
   - Add a `--single-pane` flag to `onlyterm-mux-server`
   - Modify spawn logic to exit when the sole pane exits (instead of daemonizing)
   - Configure `UnixDomain.proxy_command = ["onlyterm-mux-server", "--single-pane"]`

5. **Pattern alignment**: This is exactly how `UnixDomain.proxy_command` is designed to work — spawning a command and using stdin/stdout as the transport. The only difference is lifecycle management (single-pane vs long-lived), which is a small behavioral flag.

### Why Option 1 is the wrong choice for Phase A

While UDS is the familiar pattern from Unix/Linux, on Windows it introduces unnecessary complexity and security risk:

- **Windows DACL is unfamiliar territory**: This codebase is Windows-only, but DACL APIs are complex and error-prone. A bug here would be a security vulnerability.
- **Filesystem objects are unnecessary**: For a process that spawns once, exits once, and has exactly one client, a filesystem socket adds complexity without value.
- **Phased approach**: Phase A should prove the concept with the simplest possible mechanism. If UDS is ever needed (e.g., for post-start discovery in Phase C), it can be added later with proven working code to validate against.

### Conclusion

For Phase A, use the existing `proxy_command` / `socketpair()` transport. It provides:
- Strong security guarantees by construction (inherit-only handles)
- Minimal implementation effort (add a flag and change lifecycle)
- Simple lifecycle (process exit = socket close)
- Production-tested code path

## 8. Phase A: real end-to-end verification (orchestrator, 2026-08-13)

The crush session above honestly reported it could not complete the full
manual end-to-end attach-and-render-as-tab check ("I could not safely test
without potentially disrupting user processes"). The orchestrating session
did complete it, in an isolated `CARGO_TARGET_DIR`, using self-launched
processes whose exact PIDs were confirmed by path before touching anything
(per this repo's process-safety rule) -- and found two real bugs the crush
session's own build/clippy/fmt/test pass did not catch, because neither a
compiler nor a linter can see "this future is constructed but never
polled" as anything worse than a style nit, and neither runs the actual
IPC path:

1. **The mux protocol dispatcher was never actually driven.**
   `spawn_stdio_listener()`'s original form did
   `let _ = wezterm_mux_server_impl::dispatch::process(stream);` inside a
   bare `thread::spawn` closure. `dispatch::process` is `async fn` --
   calling it only constructs a `Future`; nothing runs until something
   polls it. `let _ = <future>;` drops that future immediately, unpolled.
   The clippy lint that exists specifically to catch this
   (`let_underscore_future`) fired correctly and was suppressed with
   `#[allow(...)]` instead of fixed. Net effect: the single-pane process
   would accept its inherited connection and then do nothing with it --
   silently. Fixed by using `promise::spawn::spawn_into_main_thread(async
   move { dispatch::process(stream).await... }).detach()`, the same
   primitive `LocalListener::run` already uses for daemon-mode
   connections, scheduled onto the same `SimpleExecutor` that
   `run_single_pane_mode`'s `executor.tick()` loop is already polling --
   no separate OS thread needed.
2. **Winsock was never initialized in the child process.** Confirmed by
   an actual manual run: `onlyterm_mux_server > Either the application
   has not called WSAStartup, or WSAStartup failed. (os error 10093)`.
   Daemon mode gets `WSAStartup` "for free" because
   `filedescriptor::socketpair()` (called on the parent/GUI side) calls
   it -- but Winsock initialization is per-process, not inherited across
   a process boundary along with a socket handle. Single-pane mode's
   child never calls `socketpair()` itself (it only inherits an
   already-created handle as stdin), so it needs its own explicit
   `WSAStartup`, added as `init_winsock()` (same `Once` + `WSAStartup`
   pattern as `filedescriptor::windows::socketpair::init_winsock`,
   which is private to that crate and not reusable directly), called
   before `spawn_stdio_listener()` wraps the inherited handle.

After both fixes: `cargo build -p wezterm-mux-server`, `cargo clippy -p
wezterm-mux-server --all-targets -- -D warnings` (no `#[allow(...)]`
needed this time) both clean. Manual end-to-end verification, actually
performed:

1. Wrote a throwaway `.ktav` test config with a `unix_domains` entry:
   `proxy_command: [<path>/onlyterm-mux-server.exe, --single-pane]`,
   `no_serve_automatically: true`, `connect_automatically: false`.
2. Launched the real GUI (`onlyterm-gui.exe --config-file
   test-single-pane.ktav connect single-pane-test`) from the isolated
   build -- a self-launched, PID-confirmed instance, never one of the
   6 real production windows.
3. Before the fixes: the GUI logged the WSAStartup error, then after a
   1-minute timeout logged `Timed out while parsing the response from
   the server` and exited on its own.
4. After the fixes: no errors at all. The window stayed open (did not
   hit the timeout path this time -- itself a meaningful signal the
   handshake succeeded). A screenshot of the live window confirms a
   real `cmd.exe` prompt (`C:\Users\Computer>`, tab titled "1: Computer")
   rendered as an ordinary tab -- the single-pane hosting process's PTY,
   attached over the proxy_command/socketpair transport, indistinguishable
   in the UI from a normal local tab. This is the acceptance criterion
   from section 4 (Phase A), genuinely met.
5. Both self-launched test processes (the GUI instance and the spawned
   `onlyterm-mux-server.exe --single-pane` child) were closed by their
   exact confirmed PIDs after verification; the 6 real production
   `onlyterm-gui.exe` instances were untouched throughout (verified by
   path before and after).

**Conclusion:** Phase A's core mechanism is now genuinely proven end to
end, not just compiled and linted. Phase B (generalizing ordinary
`SpawnTab` to this path) can proceed on this foundation.

## 9. Phase B: implementation, review findings, and an honest verification gap (2026-08-13)

Adds `spawn_single_pane_tab` (`crates/wezterm-gui/src/spawn.rs`): builds a
`UnixDomain` with `proxy_command` pointed at
`onlyterm-mux-server.exe --single-pane` (resolved next to the running
GUI's own exe path), registers it as a `ClientDomain` via `mux.add_domain`,
attaches, and spawns the requested command there via
`SpawnTabDomain::DomainName(...)`. `spawn_command_impl` now branches on a
new `per_tab_process_isolation` config flag to choose this path over the
existing direct `LocalDomain` one. `SplitPane` is explicitly excluded
(bails with an error) -- splitting into a single-pane-hosted tab is not
supported yet.

Two things the orchestrating session changed after independent review,
not present in the version crush reported "complete":

1. **Config flag default changed from `true` to `false`.** Crush's own
   reasoning (isolation is the user's requested end state, so default it
   on) is philosophically defensible, but practically this flag gates the
   code path every single new tab goes through for every user, and it
   shipped with zero live interactive verification that opening a tab
   still works at all end-to-end (see point 2). Flipping the default on
   before that gap closes would risk turning "every tab you open" into
   the untested path for anyone who updates. Left as an opt-in rollout
   lever; flipping it to `true` by default is appropriate once Phase D
   lands (see point 3) and/or a live QA pass (ideally by the user, since
   this session could not complete one -- see below) confirms the golden
   path.
2. **A real resource leak, not yet fixed:** `mux.add_domain(&domain)` runs
   once per spawned tab, with no matching removal when the tab/pane
   closes. Over a real working session (many tabs opened and closed
   across a day) this accumulates dead domain registrations in the Mux
   for the lifetime of the GUI process. This is squarely a Phase D
   ("crash/lifecycle handling") concern and is deliberately NOT patched
   here under time pressure -- flagging it explicitly rather than leaving
   it silently undocumented, and Phase D's task description should
   treat this as part of its scope, not just process-crash handling.
3. **Verification gap, disclosed honestly:** static code review confirms
   `SpawnTabDomain::DomainName(name)` resolves via
   `Mux::resolve_spawn_tab_domain` -> `get_domain_by_name` against exactly
   the name `spawn_single_pane_tab` registers, so the wiring is correct
   by inspection. But the actual interactive acceptance criterion --
   press the real "new tab" keybinding (Ctrl+T) in a live window with
   `per_tab_process_isolation = true`, confirm a new isolated tab opens
   and works, then kill that tab's hosting process and confirm the
   window and other tabs survive -- was attempted and NOT achieved this
   session. `SendKeys` and an `AttachThreadInput`-based foreground-force
   attempt both failed to make a self-launched isolated test window
   accept synthetic keyboard input (Windows' foreground-lock protections
   blocked focus-stealing from a non-interactive script context). Crush's
   own session reported the identical inability for a different reason
   (the HARD CONSTRAINT against touching real windows). This is the
   actual point of the whole initiative and it remains unverified by a
   live interactive test -- flagging this prominently rather than
   treating the code-review-level confidence as equivalent to having
   watched it work. A human manually testing this live (as has happened
   for every other UI-facing change this session) would close this gap
   directly.

Verified (mechanical/automated only, given the gap above):
`cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo fmt --check`, `cargo test -p wezterm-gui -p
wezterm-mux-server-impl -p wezterm-client -p wezterm-mux-server -p mux -p
config`, all clean.
