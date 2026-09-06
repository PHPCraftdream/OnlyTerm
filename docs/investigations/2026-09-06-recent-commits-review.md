# Независимое ревью коммитов на `main` за последние 24 часа (`c9b39b951..9aaa400aa`)

Дата: 2026-09-06.
Режим: только чтение — `git show`/`git diff`/grep по дереву на `HEAD 9aaa400aa`
(`v0.0.20-alpha`, тег локальный, на origin отсутствует). Ни сборок, ни тестов, ни
запусков; ни один процесс не тронут. Код не правился — только этот файл.
В рабочем дереве на момент ревью лежали чужие незакоммиченные правки
`crates/term/src/{screen.rs,terminalstate/mod.rs,test/resize.rs}`
(`discard_blank_resize_history`) и неотслеживаемый `docs/checkpoints/2026-09-06-1438.md`;
они не входят в диапазон и здесь не оцениваются.

Диапазон — 18 коммитов, все от одного автора, без PR/ревью-трейла:

```
09ae41e41 fix(input): use fresh process names for keyboard compatibility
913b9d2f8 perf(procinfo): avoid redundant process tree work
fb8a3b3f5 Merge reviewed process discovery fixes
0010f461e perf(gpu): reuse host instance buffers
3c2f6ba87 Merge reviewed host GPU buffer reuse
9aac87e8e perf(gui): make logging and image decode responsive
eb1119bae Merge reviewed logging and image responsiveness fixes
65336041d fix: finalize hot-path integration and regression coverage
61d2f56d9 test: pin stale image retry deadline regression
a93ebfd4c test: complete image fixtures and document verified integration
89b933b3a fix: stabilize startup tab titles and use repository-local build output
bb250a021 feat: make process title updates opt-in
75fa8bb19 style: satisfy nightly rustfmt import grouping
8920e90cf fix: prevent CJK atlas upload exhaustion and preserve font caches
08cb11589 fix: keep ConPTY cursor aligned after shrinking the viewport
8d24239e0 fix: match ConPTY blank-row handling when shrinking
00bff6925 perf: reduce render and terminal overhead with regression coverage
9aaa400aa docs: record alpha runtime acceptance and blank-history scrollbar issue
```

Терминология статуса: **PROVEN** — прямое следствие кода/байтов с цитатой `file:line`;
**SUSPECTED** — механизм достижим по коду, runtime-подтверждения нет;
**UNCONFIRMED** — гипотеза без достаточной опоры.

## Итог (ранжировано по тяжести × уверенности)

| # | Находка | Коммит | Статус | Тяжесть |
|---|---|---|---|---|
| A | Порча UTF-8 в пользовательских строках: `⚠️`/`👍` в баннерах завершения процесса превратились в `âš ï¸`/`ðŸ‘` (двойное UTF-8-кодирование). `crates/mux/src/localpane/pane_impl.rs:351,362,365`. Байты сверены с родителем `8d24239e0` | `00bff6925` | PROVEN | Высокая (видимая регрессия текста, ExitBehavior Hold/CloseOnCleanExit) |
| B | «Merge reviewed X» — пустые `--no-ff` слияния однокоммитных веток без собственного диффа и без текста ревью; единственное свидетельство реального ревью — правки в `65336041d` (откат `should_flush`, contiguity-check пула). Последние 5 коммитов (`8920e90cf..9aaa400aa`) не запушены (`origin/main = 75fa8bb19`), CI по ним не запускался; `fmt` падал на `bb250a021` и `a93ebfd4c`; CI вообще не выполняет `cargo test` — «regression coverage» нигде автоматически не прогоняется | fb8a3b3f5, 3c2f6ba87, eb1119bae; весь диапазон | PROVEN | Средняя (процесс) |
| C | Сняты `#[ignore]` с тестов, требующих GPU-адаптера: `production_pool_reuses_distinct_buffers_and_releases_unused_slots` и новый `mirrored_atlas_writes_do_not_touch_the_parent_gpu_texture` делают `request_adapter(..).expect(..)` — `cargo test -p onlyterm-gpu-render` на машине/раннере без адаптера падает. Сейчас замаскировано тем, что CI тесты не гоняет (B) | `65336041d`, `8920e90cf` | PROVEN | Средняя-низкая |
| D | Асинхронный логгер GUI: Debug/Trace уходят в очередь на 256 записей, сбрасываемую воркером (idle-flush 50 мс). Warn/Error и panic-hook делают flush-барьер, поэтому Rust-паника хвост сохраняет; но AV/SEH-abort/stack overflow/внешний `TerminateProcess` теряют до 256 последних trace/debug-строк — ровно того per-PID лога, который CLAUDE.md объявляет авторитетным при разборе падений. Плюс переупорядочивание строк: некритичная запись при полной очереди пишется напрямую в обход ещё не слитых более ранних. Заметно, что `65336041d` уже откатывал более слабую форму этой же уступки («keep flushing every record»), а `00bff6925` вернул её в другой форме | `00bff6925` | SUSPECTED (механизм PROVEN, инцидента нет) | Средняя (диагностика) |
| E | `fresh_process_tree_exe_names` намеренно обходит `SNAPSHOT_CACHE`: один `CreateToolhelp32Snapshot` на каждый Ctrl+буква и Shift+Enter. Не «шторм» H из отчёта 2026-08-25 (там — каждые 300 мс на панель при рендере), вызов строго гейтится в `encode_win32_input`, но это новый синхронный системный вызов на пути ввода с копированием `[u16; MAX_PATH]` (520 байт) на каждый процесс машины | `09ae41e41`, `913b9d2f8` | PROVEN (по дизайну) | Низкая |
| F | ConPTY-курсор после сжатия: `new_cursor_y = cursor_y - (lines.len() - physical_rows)` теперь и в ConPTY-ветке; при непустых строках ниже курсора, превышающих новую высоту (курсор в 5-й строке заполненного 53-строчного экрана → 40 строк) значение отрицательное, `set_cursor_pos` зажимает в 0 — курсор в строке 0, строка промпта в scrollback до перерисовки ConPTY. Тесты покрывают только строки 13 и 39 | `08cb11589` | SUSPECTED | Низкая-средняя (транзиентно) |
| G | `allow_process_title_updates`: при `true` `automatic_title` возвращает `process_title` и полностью игнорирует cwd (даже если OSC-заголовок никогда не приходил) — включение опции отключает cwd-заголовки, а `default_tab_title.md` п.3 по-прежнему описывает cwd-basename безусловно. `onlyterm cli set-tab-title` (явное действие пользователя) заперт той же опцией и читает конфиг клиента, а не mux. В `docs/changelog.md` записи нет — как и для ConPTY-, atlas- и keyboard-фиксов | `bb250a021` | SUSPECTED (семантика), PROVEN (changelog) | Низкая |
| H | Поиск: `capture_logical_batch` делает `ensure!(snapshot.stable_start == next_cursor)` — если scrollback обрезался, пока захватывалась перенесённая строка (панель печатает), весь поиск завершается ошибкой, а не best-effort; `try_lock_terminal_for` с таймаутом вместо блокирующего `lock()` — «timed out acquiring terminal for search» под нагрузкой | `00bff6925` | SUSPECTED | Низкая |
| I | Refill кадров: `FrameState::accept_frame` проглатывает кадр декодера, чей content-hash совпадает с ожидающим refill-ключом (одинаковые кадры анимации при ожидающем refill) — анимация теряет кадр. Узкий случай | `00bff6925` | SUSPECTED | Низкая |
| J | Fallback-fingerprint строки: `line.as_str()` (аллокация) + HashMap-lookup на каждый символ при каждом пересчёте shape-hash (смена seqno / bump эпохи) — новая стоимость на горячем пути в «perf»-коммите; ограничена числом изменившихся строк за кадр | `00bff6925` | PROVEN | Низкая |

Остальное — чисто или с оговорками, см. раздел 3.

## 1. Детали ключевых находок

### 1.1. A — порча эмодзи в `pane_impl.rs` (PROVEN)

`git show 00bff6925 -- crates/mux/src/localpane/pane_impl.rs`:

```
-                            brief = format!("⚠️  Process {cmd} didn't exit cleanly");
+                            brief = format!("âš ï¸  Process {cmd} didn't exit cleanly");
...
-                                brief = format!("👍 Process {cmd} completed.");
+                                brief = format!("ðŸ‘ Process {cmd} completed.");
```

Байты в `HEAD`: `303 242 305 241 302 240 303 257 302 270 302 217` (каждый байт исходного UTF-8
`342 232 240 357 270 217` = U+26A0 U+FE0F перекодирован как отдельный символ Latin-1 → UTF-8).
В родителе `8d24239e0` байты правильные. Остальные файлы диапазона (включая
`search.rs` с `ΟΣ`/`界` и `ringlog.rs` с `界`) не пострадали — `grep -rn "Ã\|â€\|ðŸ\|âš" crates/`
находит только эти три строки. Это не смысловая правка, а артефакт инструмента,
сохранившего файл в неверной кодировке; строки видны пользователю в баннере
`ExitBehavior::Hold`/`CloseOnCleanExit`. Исправление тривиально: вернуть литералы.

### 1.2. B — «Merge reviewed» и CI (PROVEN)

`git log --graph`: `fb8a3b3f5`, `3c2f6ba87`, `eb1119bae` — двухродительские слияния, у каждого
второй родитель — ровно один perf-коммит; `git show <merge>` даёт тот же stat, что и сам
perf-коммит (слияние ничего не меняет). Тела сообщений пустые. Следы ревью есть только
в последующем `65336041d`: откат `should_flush` из `9aac87e8e` (с комментарием, почему
Debug-хвост нельзя не сбрасывать), добавление проверки контигуозности слотов в
`InstanceBufferPool::buffer_for`, удаление неиспользуемого поля `capacity`, расширение
GPU-теста. То есть ревью, вероятно, было — но метка «reviewed» на слиянии сама по себе
ничего не удостоверяет.

`gh run list`: `windows_continuous` зелёный на всех запушенных коммитах до `75fa8bb19`
включительно; `fmt` красный на `bb250a021` (`gpu_tab_host.rs:37`, import grouping) и на
`a93ebfd4c`, починен `75fa8bb19`. Тег `v0.0.19-alpha` = `75fa8bb19`, прошёл `windows_tag`.
`origin/main = 75fa8bb19`; `8920e90cf`, `08cb11589`, `8d24239e0`, `00bff6925`, `9aaa400aa`
(и локальный тег `v0.0.20-alpha` → `9aaa400aa`) не запушены — CI их не видел.
Workflow-ы (`.github/workflows/gen_windows*.yml`, `fmt.yml`) содержат только `cargo build
--release` и `cargo fmt --check`; `cargo test` в CI отсутствует. Все цифры «N passed» в
`2026-09-06-remediation-validation.md` — локальные прогоны автора.

### 1.3. D — асинхронный логгер (`crates/env-bootstrap/src/ringlog.rs`)

`00bff6925`: для процессов с `gui` в имени создаётся `AsyncOutput` (`sync_channel(256)`,
воркер `onlyterm-diagnostic-log`, `recv_timeout(50ms)` → `flush_outputs`). В `Logger::log`:
`Warn|Error` → `async_output.flush()` (барьер) + `write_direct`; прочие уровни →
`try_send`, при `Full` — `write_direct` без барьера (`ringlog.rs`, ветка
`async_output.send(limit_queued_record(output)).err()`). `LogGuard` из `bootstrap()`
делает `log::logger().flush()` при выходе из `run()`; panic-hook в `env-bootstrap/src/lib.rs`
пишет `log::error!` → барьер срабатывает. Что теряется: хвост trace/debug при аварийном
завершении без паники (именно эти сценарии — WM_PAINT-зависание, TDR, OOM-abort — и
разбирались по per-PID логам в предыдущих отчётах). Компромисс осознанный, но он
противоречит и записи в CLAUDE.md, и комментарию, оставленному в `65336041d`.
Рекомендация: хотя бы для `RUST_LOG`/`ONLYTERM_LOG` с trace-уровнем оставлять синхронную
запись, либо снизить idle-flush до единиц миллисекунд.

### 1.4. F — знак `new_cursor_y` в ConPTY-ветке (`crates/term/src/screen.rs`)

До `08cb11589`: `new_cursor_y = preserved_cursor_y` (видимый номер строки сохранялся).
После: единая формула `cursor_y - (lines.len() - physical_rows)`. Дополнение
`required_num_rows_after_cursor` гарантирует `new_cursor_y <= preserved_cursor_y`, но не
`>= 0`: при `lines.len() - cursor_y > physical_rows` (непустых строк ниже курсора больше,
чем новая высота) результат отрицателен; `TerminalState::resize` → `set_cursor_pos(Absolute)`
→ `.max(0)` (`terminalstate/mod.rs:1092`). Для non-ConPTY эта формула была и раньше — то есть
ConPTY-ветка теперь разделяет давний edge-case, а не получает новый. Сценарий «курсор в
верхней части заполненного экрана + сжатие» не покрыт тестами `resize.rs`.

### 1.5. E — свежий Toolhelp-снимок на каждый Ctrl-аккорд

`crates/mux/src/localpane/process_info.rs:17-58` → `LocalProcessInfo::fresh_process_tree_exe_names`
(`procinfo/src/windows.rs`) → `fresh_snapshot_exe_entries()` → `Snapshot::new()` на каждый вызов.
Гейт: `keyevent.rs:94-96` (`KeyCode::Char` ASCII-буква + `CTRL`) и `shift_enter_esc_cr_for`.
Обоснование в `2026-09-05-keyboard-process-detection-hardening.md` (устаревший кэш ломал
Ctrl-J для Codex) корректно: кэш `SNAPSHOT_CACHE`/`divine_process_list` остаётся для
заголовков/cwd, keyboard-путь его не трогает и не ломает. Механизм H (300 мс × панель при
рендере) не переоткрыт. Замечания: `SnapshotExeEntry` с inline `[u16; MAX_PATH]` даёт
~150-300 КБ временного `Vec` на аккорд при 300-600 процессах (заявленная «экономия PathBuf»
сомнительна); `log::info!("diag: key-compat ...")` из `09ae41e41` (3 места + default-impl
`Pane::get_process_tree_exe_names` в `mux/src/pane.rs:443`) понижены до `debug` в
`9aac87e8e`/`00bff6925` — на `HEAD` шума нет, кроме `pane.rs:443`, где остался `info!`
для не-Local панелей (срабатывает на каждый Ctrl-аккорд в `ClientPane`).

## 2. Проверка «хрупких зон» из отчёта 2026-08-25

- **Lost-wakeup / handshake `HostProcessBackend`** — `0010f461e`/`65336041d` трогают только
  `gpu_tab_host.rs` (дочерний процесс) и новый `instance_buffer_pool.rs`; `in_flight`/
  `repaint_pending`, ack-путь, `render_thread_is_hung` не изменены. Регрессии A/C/D
  прошлого отчёта не переоткрыты.
- **Переиспользование буферов и stale-frame** — `queue.write_buffer` в WebGPU упорядочен
  после ранее отправленных submit-ов на той же очереди, поэтому перезапись буфера кадра N
  для кадра N+1 не гонится с чтением GPU (комментарий в `build_gpu_frame` верен).
  `GpuFrame` передаётся в `submit_frame(frame: GpuFrame)` по значению и нигде не
  удерживается для повторного показа (`grep GpuFrame`), значит перезаписанный буфер не
  может «пере-представиться». При `Lost|Outdated` добавлен `queue.submit(empty)`, чтобы
  staged-записи не копились. Пул обнуляется на `AttachSurface`. Чисто.
- **Порванный snapshot (`Pane::get_render_snapshot`)** — не затронут. COW-`Line`
  (`Arc<VecStorage>`/`Arc<ClusteredLine>`, `00bff6925`) делает клон снимка дешёвым и
  проверен тестами `cow_test.rs`; wire-формат serde не меняется (`Arc` прозрачен,
  `serde/rc`), тест `serde_roundtrip_keeps_legacy_line_shape` это фиксирует.
- **Process-snapshot storm (H)** — `913b9d2f8`/`00bff6925` улучшают `shared_snapshot_entries`
  (`Arc<[ProcessEntry]>` вместо `Vec::clone`, тест на сохранение поколения при ошибке),
  `current_working_dir` больше не читает командную строку (`get_params_impl(false)`).
  Keyboard-путь обходит кэш намеренно (E).
- **WM_PAINT busy-spin, утечка per-pane** — не затронуты.
- **Сигнатура кадра vs `take_instances_for_wire`** — проверено: `compute_frame_signature`
  (`draw.rs:340`) вызывается до `build_wire_frame` (`draw.rs:362`), аккумулятор ещё полон;
  перенос без копирования не обнуляет сигнатуру.

## 3. По коммитам

### `09ae41e41` fix(input) — в целом корректно
- `Snapshot::new`: проверка `INVALID_HANDLE_VALUE` вместо `is_null()` — реальный баг-фикс
  (`CreateToolhelp32Snapshot` возвращает `INVALID_HANDLE_VALUE`). `ProcIter` различает
  `ERROR_NO_MORE_FILES` и ошибку перечисления; частичный список не выдаётся за полный.
- `exe_names_from_entries`: итеративный DFS по индексу ppid с `visited` — тесты
  (`key_compat_tests.rs`) реальные: порядок не parent-first, цикл, 10 000-звенная цепочка,
  отсутствующий корень → `NotFound`, соседняя панель не протекает.
- Разбиение `localpane.rs` → `pane_impl.rs`/`process_info.rs`, `keyevent.rs` → `key_table.rs`
  — перенос; `KeyTableState::current_expiration()` заменил прямой доступ к `stack`.
- См. E.

### `913b9d2f8` perf(procinfo) — корректно
- `build_tree_iterative`: индекс `children_by_parent` (O(N) вместо O(N·K)); порядок обхода
  сохранён (push в обратном порядке), тест `indexed_tree_preserves_depth_first_source_order`
  и счётчик `ppid_of` — реальные.
- `read_process_wchar`: нечётная `Length` теперь `None` (раньше молча усекалась) — на практике
  недостижимо (`UNICODE_STRING::Length` чётный).
- `clone_without_children` устраняет клон всего поддерева для `foreground`.

### `0010f461e` perf(gpu) + `65336041d` — корректно (см. §2), но C
- `required_capacity` → степень двойки с `MIN_BUFFER_SIZE=4`, ограничение `max_buffer_size`;
  `target_capacity` растёт при нехватке и ужимается при 4× запасе — тесты
  `BufferPoolCore` детерминированы. `begin_frame` усекает слоты. Сценарий «`create_buffer`
  упал на слоте k» оставляет `capacities` длиннее `slots`, но процесс всё равно выходит
  с fatal 4 — не проблема.

### `9aac87e8e` perf(gui) + `65336041d` — корректно, с оговоркой D (в её ранней форме откачена)
- `LevelRing`: до правки при заполненном кольце `first == last` и `len()` возвращал 0, а
  `append_to_vec` — пустой срез; явное `len` чинит это. Тест
  `level_ring_keeps_exactly_sixteen_entries_before_wrapping` без фикса падает.
- Декодер изображений: снято ожидание до 125 мс на GUI-потоке (`wait_for_first_frame`
  удалён), опрос через `IMAGE_DECODE_POLL_INTERVAL=25ms` и `schedule_budget_repaint`
  (независимо от фокуса). `Disconnected` → `FrameIndex(current_index)` вместо `0`
  (раньше после конца декодирования кадр 0 пропускался/дублировался) — тест
  `disconnected_decoder_resumes_at_frame_zero_without_skipping_it` реальный.
- Диагностические `info!` понижены до `debug!`.

### `61d2f56d9`, `a93ebfd4c` — тестовые, чисто
- Второй правит fixture так, чтобы устаревший `frame_start` делал старую формулу
  «unambiguously past»; иначе тест мог проходить по таймингу.

### `89b933b3a` — корректно
- `.cargo/config.toml` `[build] target-dir = "target"`: переменная окружения
  `CARGO_TARGET_DIR` всё равно имеет приоритет над config — CLAUDE.md это отражает.
  `ci/dev-install.ps1` берёт `target_directory` из `cargo metadata` — правильно.
- `get_current_working_dir`: при известном `last_known_good.cwd` — `terminal.try_lock()`
  вместо `try_lock_terminal_for` (таймаут + пометка unresponsive). Тест
  `known_cwd_does_not_wait_on_the_terminal_or_mark_it_unresponsive` держит `terminal.lock()`
  из того же потока — со старым кодом ждал бы таймаут и выставил `unresponsive`; реальный.

### `bb250a021` feat — реализация корректна, семантика/доки — G
- `osc.rs`: guard-ветка на `SetIconName*`/`SetWindowTitle*` при `!allow_process_title_updates()`
  — no-op, `Alert::TitleChanged` не шлётся, инвалидаций tab bar нет.
- Тесты `term/src/test/title.rs` (`ignored`/`enabled`/tmux-title) и конфиг-тесты — реальные.
  Существующие mux-тесты переключены на `allow = true`, чтобы не менять их ожидания.
- Доки: страница `allow_process_title_updates.md` есть, включена в `SUMMARY.md` и
  `config/index.md`; `set-tab-title.md`/`set-window-title.md`/`passing-data.md`/`get_title.md`
  обновлены. Нет записи в `docs/changelog.md`.

### `75fa8bb19` — только импорт, чисто (починил `fmt` CI).

### `8920e90cf` fix CJK atlas — корректно
- `WebGpuTexture::write` при `mirroring_enabled` не делает локальный `write_texture`.
  Безопасно: единственный переход на in-process рендер — при неудаче
  `HostProcessBackend::spawn` в момент создания пайплайна (`render_pipeline.rs:389-412`),
  когда `mirror_atlas=false` и mirroring никогда не включается; смена бэкенда «на лету»
  отсутствует. `enable_mirroring` теперь сбрасывает начальную очистку атласа пустым
  submit-ом (`Atlas::new` очищал текстуру до включения зеркала — иначе staging копился).
- `atlas_retry_size`: при `pass == 0` и атласе < 2048 — сразу рост вместо очистки;
  ≥ 2048 — очистка на текущем размере, рост только на pass 1. Тесты на границах.
- Шейпер: удалён `shaped_any`/выгрузка шрифта после fallback-only прогона — причина
  повторного парсинга CJK-фоллбэков; тест `fallback_only_runs_keep_parsed_primary_font`
  проверяет идентичность `_data.as_ptr()` до/после.
- Тест `mirrored_atlas_writes_do_not_touch_the_parent_gpu_texture` — см. C.

### `08cb11589`, `8d24239e0` ConPTY — логика соответствует сообщениям; F
- `8d24239e0`: `prune_limit` при ConPTY-сжатии = `len - (shrink - min(shrink, cursor.y))`
  — оставляет ровно столько хвостовых пустых строк, чтобы промпт поднялся на `shift`,
  как в нативной консоли; тест `conpty_shrink_moves_prompt_over_leading_blank_like_native_console`
  проходит по трассировке вручную (24→23→22→18).
- `test/mod.rs`: уровень `env_logger` в тестах снижен с `Trace` до `warn` по умолчанию —
  разумно.

### `00bff6925` perf (61 файл, +3334/−750) — крупный смешанный коммит; A, D, H, I, J
Помимо перечисленного:
- `env-bootstrap`: `bootstrap()` возвращает `LogGuard`; все три `main` его держат.
- `onlyterm-blob-leases/simple_tempdir.rs`: `lease_by_content` держал `refs` и вызывал
  `add_ref`, который берёт тот же `std::Mutex` — реальный дедлок; исправлено инкрементом
  под уже взятой блокировкой, тест есть.
- `onlyterm-font`: `SharedFontData` (`&'static [u8]` / `Arc<Box<[u8]>>`) с weak-кэшем по
  `(path, len, mtime)` — один owned-read на файл для шейпера/растеризатора/метрик;
  `OwnedRbFace` сохраняет инвариант стабильного адреса. `FontShaper::append_handles`
  добавляет fallback-шрифты без пересоздания шейпера; `shape_impl` больше не возвращает
  `ClearShapeCache` — инвалидация теперь точечная через
  `TermWindowNotif::InvalidateShapeCache(Vec<char>)` → `fallback_generations` →
  `fallback_generation` в ключах `ShapeCacheKey`/`LineQuadCacheKey`/`LineToEleShapeCacheKey`/
  `RetainedRow`. Цепочка сходится: `FallbackResolveInfo::process` кладёт handles в
  `pending` и затем зовёт `completion(requested_glyphs)`; на следующем `shape_impl`
  handles вставляются. Пустой `chars` (смена палитры) сохраняет старую полную очистку.
- `glyphcache`: GUI-поток больше не читает blob с диска — `DECODED_PIXEL_CACHE`
  (64 МиБ / 1024 записей, LRU) + `decoded_refill` (2 воркера, 16 заданий, 256 МиБ
  транзиентного бюджета); при отказе refill кадр навсегда становится 1×1 плейсхолдером
  (`refill_failed`). `WebpFrames` — ленивый итератор; попутно исправлен старый баг: первый
  WebP-кадр собирался из ещё не заполненного буфера (`from_raw(raw_buf.clone())` до
  `read_frame`), тест теперь сравнивает пиксели. `DecodedPixelsHandle::pixel_data_mut`
  паникует — путь недостижим: `Atlas::allocate_with_padding` копирует через `draw_image`
  по `&dyn BitmapImage` (только чтение).
- `spawn.rs`/`main.rs`: изолированные startup-вкладки готовятся группами по 4
  (`prepare_attach` конкурентно, `commit_prepared_attach` по порядку через `StartupOrder`
  с `Drop`-сигналом — тест на упавшего предшественника реальный); UAC/неизолированные
  остаются последовательными; последняя вкладка активируется явно.
- `window/os/windows/wheel.rs`: умножение `delta * WHEEL_SCROLL_LINES` перенесено в i32
  с clamp — устранён wrap i16 (кандидат на «редкий прыжок колеса»). `normalize_viewport`
  — clamp до проверки низа.
- `procinfo`: см. §2. `perform_actions_reusing` — перенос владения вместо `chunk.to_vec()`.
- Документы `2026-09-06-remediation-validation.md`/`review-remediation-plan.md` честно
  фиксируют отклонённый эксперимент (фоновая очередь глифов → «дождь» CJK) и pending-статусы.

### `9aaa400aa` docs — только документы, чисто.

## 4. Соответствие CLAUDE.md
- `panic!("...{var}")` без аргументов в диапазоне не появляется (grep по диффу);
  `bail!`/`format!` с inline-аргументами допустимы в edition 2018.
- `.onlyterm.ktav` не затрагивается; версии не поднимались (теги — отдельные объекты).
- Каталог сборки: `89b933b3a` фиксирует `[build] target-dir` и обновляет заметку в CLAUDE.md.
- Процессы OnlyTerm: `remediation-validation.md` явно утверждает, что ни один не завершался;
  по коду подтвердить/опровергнуть нельзя.

## 5. Рекомендации (по убыванию срочности)
1. Вернуть литералы `⚠️`/`👍` в `pane_impl.rs:351,362,365` (A) — и проверить кодировку
   инструмента, который писал файл.
2. Запушить `8920e90cf..9aaa400aa` (или хотя бы прогнать `cargo fmt --check` и
   `cargo build --release` локально) до того, как считать `v0.0.20-alpha` проверенным CI;
   добавить `cargo test` для scoped-крейтов в `windows_continuous` — иначе «regression
   coverage» не защищает ни от чего (B).
3. Вернуть `#[ignore]` (или `skip if no adapter`) на два GPU-теста (C).
4. Для trace/debug-уровня в GUI оставить синхронную запись или сократить idle-flush (D).
5. Дописать тест на «курсор в верхней части заполненного экрана + сжатие» (F) и
   согласовать `default_tab_title.md` п.3 с поведением `automatic_title(.., true)` (G);
   добавить записи в `docs/changelog.md` для `allow_process_title_updates`, ConPTY-фиксов,
   CJK-atlas и keyboard-детекции.
