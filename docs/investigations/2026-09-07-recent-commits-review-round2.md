# Независимое ревью, раунд 2: проверка `6e1e6a934` против находок раунда 1 и свежее ревью `975d86635`

Дата: 2026-09-07. `HEAD = 6e1e6a934` (`main`).
Режим: чтение диффов (`git show`), grep по дереву, точечные прогоны тестов (см. §4).
Код не правился; единственный новый файл — этот отчёт.

Хронология (один и тот же git-автор во всех трёх коммитах):

```
975d86635  2026-09-06 22:10  fix: avoid blank-only scrollback after shrinking fresh ConPTY panes
b9ba6c18e  2026-09-06 22:16  docs: independent review of the last 24h of commits on main   (отчёт раунда 1)
6e1e6a934  2026-09-06 23:08  fix: address review findings with targeted regression tests
```

`975d86635` лёг за 6 минут до коммита отчёта раунда 1 и им не покрыт; `6e1e6a934` —
ответ на отчёт раунда 1 с собственным резюме `docs/investigations/2026-09-06-review-follow-up.md`.
Резюме рассматривалось как набор утверждений для проверки, а не как факт.

В рабочем дереве на момент ревью лежали чужие незакоммиченные правки — `M crates/term/src/test/mod.rs`
(одна строка `mod conpty_startup;`), `?? crates/term/src/test/conpty_startup.rs`,
`?? docs/checkpoints/2026-09-06-1438.md`, `?? docs/investigations/2026-09-06-native-prompt-row.md`.
Они не тронуты и здесь не оцениваются; см. оговорку в §4 о том, что тестовый прогон
крейта `onlyterm-term` компилировал именно это дерево.

Статусы, как в раунде 1: **PROVEN** — прямое следствие кода с цитатой; **SUSPECTED** —
механизм достижим, runtime-подтверждения нет; **UNCONFIRMED** — гипотеза.

## 1. Вердикты по находкам раунда 1 против `6e1e6a934`

| # | Находка раунда 1 | Вердикт | Доказательство (файл/hunk) |
|---|---|---|---|
| A | Порча UTF-8 в баннерах завершения процесса | **FIXED** | `pane_impl.rs`: новая `process_exit_brief(cmd, success)` с `\u{1f44d}` (U+1F44D THUMBS UP SIGN) и `\u{26a0}\u{fe0f}` (U+26A0 WARNING SIGN + U+FE0F VS-16); двойной пробел после ⚠️ сохранён как в родителе `8d24239e0`. Литералы в тесте `exit_message_tests` побайтно верны: `360 237 221 215` и `342 232 240 357 270 217` (файл сам не пострадал от той же перекодировки). `git grep -P` по паттернам двойного кодирования (`Ã`, `â€`, `ðŸ`, `âš`, `Ï¸`, `ï¸`) по всему дереву — единственные вхождения находятся внутри цитат отчёта раунда 1; других повреждённых файлов нет |
| B | «Merge reviewed» без диффа; «CI не гоняет `cargo test`»; 5 коммитов не запушены | **ОШИБКА РАУНДА 1 (часть про CI) — исправлена в follow-up; остальное устарело** | `.github/workflows/gen_windows{,_continuous,_tag}.yml` содержат `cargo nextest run --all --no-fail-fast` (генерируется `ci/generate-workflows.py:401`); то же в `git show 9aaa400aa:.github/workflows/gen_windows_continuous.yml`, т.е. и на HEAD раунда 1. Утверждение раунда 1 «`cargo test` в CI отсутствует» было неверным — grep искал буквальную строку `cargo test`. Следствие: тесты C (GPU без адаптера) в CI действительно выполнялись бы, так что C была серьёзнее, чем оценил раунд 1. На момент этого ревью `origin/main = 6e1e6a934` (всё запушено), `fmt` зелёный, `windows_continuous` и `windows_tag` для тега `v0.0.21-alpha` — `in_progress` (см. §3.1). Пустые `--no-ff` слияния как факт процесса остаются, но это не дефект кода |
| C | GPU-тесты без `#[ignore]` падают на машине без адаптера | **FIXED** | Новый `onlyterm-gpu-render/src/test_gpu.rs`: `adapter()` → `request_adapter` (wgpu 25.0.2, возвращает `Result`) → при `Err` печатает `SKIP ...` и возвращает `None`; оба теста (`instance_buffer_pool.rs`, `atlas_upload_test.rs`) делают `let Some(adapter) = ... else { return; }`. `ONLYTERM_REQUIRE_GPU_TESTS=1` превращает отсутствие адаптера в `assert!`-панику. Два юнит-теста с `Backends::empty()` реальны (`headless_runner_can_report_missing_adapter`, `required_gpu_run_does_not_silently_skip`; payload `assert!` с аргументами — `String`, `downcast_ref::<String>` корректен). Оговорка: `SKIP` идёт в `eprintln!`, под nextest по умолчанию не виден — на раннере без GPU оба теста «passed» бесследно; README это документирует. `request_device(..).expect(..)` по-прежнему падает при наличии адаптера, но невозможности создать устройство — намеренно |
| D | Асинхронный GUI-логгер: потеря хвоста trace/debug при аварийном завершении; переупорядочивание при полной очереди | **PARTIALLY FIXED** (ровно по рекомендации №4 раунда 1) | `ringlog.rs:22-25` `use_background_output(is_gui, max_level) = is_gui && max_level == LevelFilter::Info`; дефолт GUI — `filter_level(LevelFilter::Info)` (`ringlog.rs:507`), т.е. **по умолчанию лог остаётся асинхронным** (очередь 256, idle-flush 50 мс). Явный `ONLYTERM_LOG=debug`/`trace` (любая директива выше Info поднимает `filter.filter()`) → полностью синхронная запись. Переупорядочивание устранено: `flush_before_direct` при `Full`/`Disconnected` сначала ставит барьер; проверено, что `flush()` (`ringlog.rs:187-192`) использует блокирующий `tx.send(Flush)` + `rx.recv()`, т.е. реально ждёт слива всей очереди; при мёртвом воркере `send` падает и запись идёт напрямую без зависания. Воркер (`ringlog.rs:205-260`) сам не логирует — самодедлока через барьер нет. Остаток: per-PID лог на уровне Info по-прежнему теряет ≤256 записей + ≤50 мс при TerminateProcess/AV/stack overflow; follow-up честно это фиксирует («neither mode promises power-loss durability»). Тесты: предикат + барьер на канале ёмкости 1 — реальные |
| E | Свежий Toolhelp-снимок на каждый Ctrl-аккорд; `[u16; MAX_PATH]` на процесс; `info!` в `pane.rs:443` | **IMPROVED** (механизм сохранён намеренно, как и принял раунд 1) | `procinfo/src/windows.rs`: `SnapshotExeEntries { entries: Vec<{pid,ppid,exe: Range<usize>}>, names: Vec<u16> }`, `push` копирует до первого NUL; `entry_exe_name(&[u16])` (`windows.rs:587-594`) корректно обрабатывает срез без NUL (`position(0).unwrap_or(len)`). Семантика ошибок `fresh_snapshot_exe_entries` не изменилась (`?` на каждой записи). `pane.rs:443` `info!` → `debug!`. Тест `packed_names_copy_only_live_utf16_and_keep_unicode_names`: проверка содержимого имён (CJK + emoji) реальна; неравенство по байтам (24 против 520 на запись) тривиально истинно — не защищает ни от чего, но и не вредит |
| F | Отрицательный `new_cursor_y` в ConPTY-ветке при непустых строках ниже курсора | **NOT FIXED — поведение закреплено тестом как принятое** | `term/src/test/resize.rs::conpty_resize_clamps_cursor_when_its_old_line_leaves_the_viewport` (53 строки заполнены, курсор в строке 5, сжатие до 40) утверждает ровно то состояние, которое описал раунд 1: `cursor_pos().y == 0`, `all_lines()[5]` = «old prompt» (в scrollback), `visible_lines()[0]` = «row 13». Follow-up прямо отказывается менять поведение («F is not represented as a fixed regression»): единственная альтернатива — выкидывать непустые строки ниже курсора, что деструктивно; ConPTY после ресайза перерисовывает экран. Оценка раунда 1 (низкая-средняя, транзиентно) остаётся; рекомендация «дописать тест» выполнена. `discard_blank_resize_history` из `975d86635` здесь no-op (строка 0 непустая) |
| G | `default_tab_title.md` п.3 противоречит `automatic_title(.., true)`; нет записей в changelog | **FIXED (доки/changelog)**, семантика не менялась намеренно | `default_tab_title.md` п.4 теперь условный; сверено с `tabbar.rs:281-294`: `allow=true` → `process_title`; `allow=false` → basename(cwd), при пустом cwd — `process_title` (в доке: «If cwd is unavailable, the process-derived title is used as a fallback» — верно). CLI-гейт описан как «convenience policy, not an authorization boundary». `docs/changelog.md`: добавлены записи для `allow_process_title_updates`, CJK/atlas, ConPTY-ресайза (включая `975d86635`), keyboard-детекции, логгера, анимации |
| H | Поиск: `ensure!(stable_start == next_cursor)` валит весь поиск; `try_lock_terminal_for` с таймаутом → «timed out acquiring terminal for search» | **FIXED** (с осознанным компромиссом) | `search.rs:193-225`: `snapshot_physical_batch` стал `async`, цикл `try_lock()` → `smol::Timer::after(2ms)`; блокировка не удерживается через `await` (cancellation-safe), флаг `unresponsive` не трогается. Прежний код блокировал GUI-поток (`promise::spawn::spawn` → `spawn_local`) на `TERMINAL_ACCESSOR_LOCK_TIMEOUT = 8 мс` (`localpane.rs:175`) на каждый чанк и при превышении ронял поиск. Eviction: `search.rs:268-273` вместо `ensure!` сбрасывает только накопитель `pending` (перенесённую строку, чей префикс вытеснен) и продолжает с `snapshot.stable_start`; `complete` не затрагивается; `stable_start` может быть только `>= next_cursor` (`start = cursor.max(phys_to_stable(0))`), так что `!=` означает именно вытеснение. Оба теста реальны и детерминированы (прослежено: первый `poll` доходит до `yield_now()` без таймера; после `erase_scrollback()` `earliest = 277 > 256`; `parking_lot::try_lock` из того же потока при удерживаемом `lock()` даёт `None`, не дедлок). Компромисс: ожидание не ограничено; поисковые футуры `.detach()`-нуты (`overlay/copy/render.rs`), закрытие оверлея их не отменяет — панель с намертво зажатым терминальным `Mutex` будет крутить `try_lock` каждые 2 мс и держать один из `SEARCH_WORKER_COUNT = 2` глобальных пермитов, пока блокировка не отпустится. Раньше такой поиск завершался ошибкой. Низкая тяжесть: это только «мёртвая» панель |
| I | `accept_frame` проглатывает кадр декодера с тем же content-hash при ожидающем refill | **FIXED** | `image_decode.rs:529-537`: ветка дедупликации удалена, каждый кадр декодера добавляется в timeline. Проверено, что ветка была мёртвой/вредной: у канала декодера единственный продюсер `run_decoder_thread(tx: SyncSender<DecodedFrame>)` (`image_decode.rs:288`), результаты refill приходят по отдельному `refill_receiver` и обрабатываются в `poll_refills` (`image_decode.rs:539-560`) — значит через `accept_frame` могли проходить только настоящие кадры. Тест `repeated_animation_pixels_keep_distinct_timing_during_refill` реален: два кадра с одним lease и разными `duration`, `pending_refills` содержит ключ — оба кадра сохраняются, цикл `FrameIndex` проверен |
| J | Fallback-fingerprint строки: `as_str()`-аллокация + HashMap-lookup на каждый символ | **FIXED** | `render/mod.rs:763-793` `fallback_fragments_generation`: ранний `return 0` при пустой карте; стриминг `line.visible_cells()`/`cell.str()` без `as_str()`; хэшируются только символы, присутствующие в карте. Эквивалентность инвалидации: текст уже входит в `compute_shape_hash()`, поколения начинаются с 1 (`render_pipeline.rs:1474-1477`, `or_insert(0)` + `wrapping_add(1)`), поэтому «символ отсутствует» и «символ с поколением 0» неразличимы и раньше; любое изменение состава/поколения по-прежнему меняет ключ. `fallback_text_generation` остался для кластеров (`render/mod.rs:422-424`, `render/pane.rs:598`) и тоже получил ранний выход. Тест с паникующим итератором доказывает отсутствие сканирования при пустой карте — реальный |

Итого: A, C, G, H, I, J — исправлены; D — частично (по минимальной рекомендации, дефолт
остаётся буферизованным); E — улучшение без смены механизма; F — не исправлено, осознанно
закреплено тестом; B — раунд 1 ошибся в части про CI, follow-up корректен.
Ни одна из правок не ухудшила исходную находку («MADE WORSE» не выявлено).

### 1.1. Проверка соответствия `6e1e6a934` CLAUDE.md
- `panic!("...{var}")` без аргументов в диффе нет (`panic!("empty fallback map must not scan text")` без интерполяции; `assert!(.., "{}", reason)` явный).
- `.onlyterm.ktav` не затронут; версии в `Cargo.toml` не поднимались (тег `v0.0.21-alpha` — отдельный объект, см. §3.1).
- Новые зависимости, `unsafe`, изменения wire-формата — не появились (сверено по диффу).

## 2. Свежее ревью `975d86635` — «avoid blank-only scrollback after shrinking fresh ConPTY panes»

Изменение: `Screen::discard_blank_resize_history(&mut self, palette)` (`screen.rs:103-124`) —
пока `lines.len() > physical_rows`, снимает с головы строки, которые `is_whitespace()` и у
всех видимых ячеек нет `reverse`/`underline`/`overline`/`strikethrough`/`hyperlink`/`images`,
а `palette.resolve_bg(bg) == palette.background`; за каждую снятую строку
`stable_row_index_offset += 1`. Вызывается из `TerminalState::resize`
(`terminalstate/mod.rs:937-940, 972-975`) только при
`enable_conpty_quirks && !alt_screen_is_active && rows.max(1) < physical_rows && scrollback_rows() == physical_rows`,
где условие вычисляется **до** `Screen::resize`, т.е. «панель без истории вообще».

### 2.1. Находки (ранжировано)

| # | Находка | Статус | Тяжесть |
|---|---|---|---|
| P1 | Узость гейта `scrollback_rows() == physical_rows`: пруннинг применяется только при полном отсутствии истории. Как только в истории есть хотя бы одна реальная строка (баннер `cmd.exe`, ушедший вверх при первом сжатии), последующие сжатия снова кладут пустой префикс (`\r\n` перед промптом) в scrollback. Это ровно то поведение, что было до коммита, и оно невидимо, когда скроллбар уже показан из-за настоящей истории — поэтому фикс прагматичен, но название «fresh panes» надо понимать буквально | PROVEN (по коду) | Низкая (ограничение объёма, не дефект) |
| P2 | Строки-пробелы с невидимыми, но семантически значимыми атрибутами удаляются: `wrapped` (пустая физическая строка-голова перенесённой логической) и семантические зоны OSC 133 (`semantic_type` на ячейках) — навигация «по промптам» может потерять границу зоны. Только для пустых строк в голове scrollback у панели без истории | SUSPECTED | Очень низкая |
| P3 | `SKIP`-подобная незаметность в тестах: все пять тестов используют `std::assert_eq!` — это стиль файла (`resize.rs` уже так написан), не проблема | — | — |
| P4 | Changelog-запись для этого коммита появилась только в следующем `6e1e6a934` («…avoids creating empty-only scrollback when shrinking a fresh pane»); investigation-док обновлён в самом коммите. Документация в порядке на HEAD | PROVEN | Инфо |

Дефектов корректности не найдено. Подробности проверки:

- **Инварианты `Screen`.** Цикл останавливается на `lines.len() == physical_rows` — видимая
  область всегда последние `physical_rows` строк, поэтому позиция курсора (относительная к
  viewport) и результат `Screen::resize` не требуют коррекции; `stable_row_index_offset += 1`
  на каждую снятую строку совпадает с паттерном пруннинга в `screen/scroll.rs:208,272` —
  стабильные индексы оставшихся строк сохраняются (тест проверяет
  `visible_row_to_stable_row(cursor.y)` до/после).
- **Трассировка репродьюсера вручную.** 24 строки, `\r\nprompt> ` (курсор y=1) → 23:
  ConPTY-ветка `8d24239e0` даёт `shift = min(1,1) = 1`, `prune_limit = len`, хвост не
  режется → 24 строки при 23 видимых, строка 0 пустая уходит в историю → `new_cursor_y = 0`;
  без фикса `scrollback_rows() = 24` (скроллбар из одной пустой строки), с фиксом — 23.
  Дальше 23→18 (`shift = 0`, срезаются 5 хвостовых пустых), 18→30 (гейт выключен ростом,
  паддинг в хвост), 30→24 (гейт снова истинен, хвостовые пустые срезаны до 24, голова
  непустая — ничего не снимается). Ожидания теста совпадают с трассировкой.
- **Цвет фона.** `resolve_bg(Default) == palette.background`; явный фон, равный фону палитры,
  визуально неотличим от дефолтного — удаление корректно. Тест
  `conpty_resize_uses_active_palette_for_blank_background` — настоящий двусторонний
  дискриминатор (переопределение `colors[15]` меняет исход 23↔24).
- **Изображения.** Ячейки Kitty-плейсмента имеют текст `" "`, т.е. `is_whitespace()` истинно,
  и строку спасает только проверка `attrs.images().is_none()` — тест в `image.rs` реален.
- **Гиперссылки/подчёркивание/фон** — три префикса теста
  `conpty_resize_preserves_text_and_visible_blank_row_decoration` покрывают `hyperlink`,
  `underline`, явный фон; `overline`/`strikethrough`/`reverse` в коде есть, тестами не
  покрыты (некритично).
- **Существующая история** — `conpty_resize_does_not_discard_preexisting_blank_history`
  (25 переводов строки → 2 строки истории → гейт ложен, `phys_to_stable_row_index(0)` и
  `scrollback_rows()` неизменны) реален.
- **Alt-screen и рост** исключены гейтом; `size.rows.max(1)` согласовано с
  `physical_rows = size.rows.max(1)` в `Screen::resize`.
- **Взаимодействие с F.** Для заполненного экрана (`conpty_resize_clamps_cursor_...`) строка 0
  непустая → цикл выходит сразу; фикс не маскирует и не усугубляет F.
- **CLAUDE.md.** `format!("{}", prefix)` с явным аргументом; `panic!` нет; пользовательский
  конфиг и версии не тронуты.
- **Соответствие нативной консоли** (утверждение follow-up-дока «like native console») здесь
  не перепроверялось — это runtime-наблюдение, недоступное чтением кода; оба документа
  (`2026-09-06-conpty-cursor-after-shrink.md` §«Unreleased follow-up» и
  `review-follow-up.md`) честно требуют «GUI runtime acceptance», которого на момент ревью нет.

## 3. Новые наблюдения

### 3.1. Тег `v0.0.21-alpha` поставлен и запушен до runtime-приёмки (PROVEN)
`git tag --points-at HEAD` → `v0.0.21-alpha`; `git ls-remote --tags origin` показывает
аннотированный `dd85e1427` → `6e1e6a934` на origin; `origin/main = 6e1e6a934`.
`gh run list`: `fmt` — success; `windows_continuous` (main) и `windows_tag` (`v0.0.21-alpha`)
— `in_progress` на момент ревью (стартовали 2026-09-06T22:00:25Z). Оба документа автора
одновременно утверждают, что `975d86635` и `6e1e6a934` «still require GUI runtime
acceptance» и «not in the existing v0.0.20-alpha tag» — при этом уже выпущен следующий
alpha-тег с этими же изменениями. Не дефект кода, но противоречие между заявленным
статусом приёмки и фактом тегирования; версии в `Cargo.toml` при этом не поднимались.

### 3.2. Параллельная незакоммиченная работа в той же области (UNCONFIRMED)
Незакоммиченные `crates/term/src/test/conpty_startup.rs` и
`docs/investigations/2026-09-06-native-prompt-row.md` указывают на продолжающуюся итерацию
над ConPTY-ресайзом/«native prompt row». Содержимое не рецензировалось; но любой следующий
коммит в `screen.rs`/`terminalstate/mod.rs` стоит проверять на совместимость с гейтом
`discard_new_blank_history` (он читает `physical_rows`/`scrollback_rows()` до ресайза).

### 3.3. Мелочи по `6e1e6a934`, не тянущие на находки
- `test_gpu.rs::required_gpu_run_does_not_silently_skip` печатает панику дефолтным hook-ом в
  захваченный stderr — шум только при `--nocapture`.
- `flush_before_direct` при переполнении блокирует логирующий поток до слива всех 256
  записей — это и есть барьер; при burst-логировании на уровне Info GUI-поток на это время
  ведёт себя как при синхронном логгере. Ожидаемо.
- `history_eviction_during_wrapped_capture_keeps_surviving_suffix` полагается на то, что
  `TestConfig` панели держит ≥ 277 строк scrollback (иначе `erase_scrollback()` не меняет
  `earliest` относительно 256). Проходит; при ужесточении `TestConfig::scrollback_size`
  тест станет вакуумным — стоит помнить.

## 4. Точечные прогоны

Хост: 48.9 ГБ ОЗУ, ~23 ГБ свободно на момент прогонов; `RUSTC_WRAPPER=sccache`;
`CARGO_TARGET_DIR` снят из окружения команды (по CLAUDE.md), сборка в `target/`.

Первые две попытки (`cargo test` без `-j` и с `-j 4`) упали на стороне хоста, а не кода:
`rustc` через `sccache` завершался с `STATUS_STACK_BUFFER_OVERRUN` после
`memory allocation of N bytes failed`, затем `E0786 ... The paging file is too small for this
operation to complete. (os error 1455)` при mmap `libtest-*.rlib` — исчерпание commit charge
при параллельно идущих чужих сборках (два посторонних `cargo`-процесса на хосте, не мои,
не тронуты). Третья попытка с `-j 2` прошла целиком:

```
env -u CARGO_TARGET_DIR cargo test -j 2 -p onlyterm-term -p mux -p env-bootstrap -p procinfo -p onlyterm-gpu-render
env_bootstrap        10 passed   (в т.ч. explicit_diagnostics_are_synchronous, queue_overflow_flushes_earlier_records_before_direct_fallback)
mux                  78 passed   (в т.ч. exit_message_tests, оба новых search-теста; 47.97 s)
onlyterm_gpu_render  41 passed   (headless_runner_can_report_missing_adapter, required_gpu_run_does_not_silently_skip; прошли ли два адаптерных теста по-настоящему или через SKIP — под захватом stderr не видно, см. оговорку к C)
onlyterm_term        92 passed   (все пять тестов 975d86635 + conpty_resize_clamps_cursor_... из 6e1e6a934)
procinfo             17 passed   (packed_names_copy_only_live_utf16_and_keep_unicode_names)
```

Оговорка: `onlyterm-term` компилировался с чужими незакоммиченными
`test/mod.rs` + `test/conpty_startup.rs` — отсюда 92 теста против 91, заявленных в follow-up;
на результаты рецензируемых тестов это не влияет (они не зависят от `conpty_startup`).
`cargo test -p onlyterm-gui` (170 тестов по заявлению follow-up, включая
`fallback_fingerprint_*`, `repeated_animation_pixels_*`) локально не прогонялся из-за той
же нехватки памяти на хосте; для I и J вердикт основан на чтении кода и на ожидаемом
результате CI (`cargo nextest run --all`, `in_progress` на момент ревью).
`cargo clippy`/`cargo fmt --check` локально не запускались; `fmt` в CI зелёный.

## 5. Рекомендации (по убыванию срочности)
1. Дождаться зелёных `windows_continuous`/`windows_tag` для `6e1e6a934`/`v0.0.21-alpha` и
   провести заявленную GUI runtime-приёмку ConPTY-сжатия (в т.ч. сценарий P1 — панель с уже
   существующей историей) до того, как считать `v0.0.21-alpha` проверенным (§3.1).
2. D: если per-PID лог должен оставаться «авторитетным» при аварийных завершениях без паники
   (CLAUDE.md), либо снизить idle-flush для Info, либо документировать в CLAUDE.md, что при
   разборе таких падений нужен `ONLYTERM_LOG=debug`.
3. H: рассмотреть верхнюю границу ожидания в `snapshot_physical_batch` (например, десятки
   секунд с возвратом ошибки), чтобы «мёртвая» панель не могла удерживать поисковый пермит
   бесконечно.
4. Исправить в отчёте раунда 1 (или в его follow-up) формулировку про отсутствие тестов в CI —
   уже сделано в `review-follow-up.md`; отдельной правки `2026-09-06-recent-commits-review.md`
   не требуется, этот раунд фиксирует поправку.
