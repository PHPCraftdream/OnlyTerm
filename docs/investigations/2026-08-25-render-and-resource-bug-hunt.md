# Рендер и ресурсы: второй триггер «курсор не на строке промпта», прочие глюки кадра, лишняя работа и утечки

Дата анализа: 2026-09-01 (файл заведён под датой задачи, 2026-08-25).
Режим: read-only — код на `HEAD 8f867381d` (`v0.0.17-alpha-5-g8f867381d`), git-история,
существующие отчёты и per-PID логи в `C:\Users\Computer\.local\share\onlyterm\`. Ни одного
запуска, ни одной правки кода, ни один процесс не тронут.
Логи: 89 файлов `onlyterm-gui.exe-log-*.txt` (6.4 МБ, 27–29 августа … 1 сентября), все от
одной сборки `v0.0.17-alpha-4-ge0f07e50a-dirty` (commit `e0f07e50a`, built 2026-08-25). Эта
сборка уже содержит `0c7fbab37` (cursor-aware retained fix), `58097a015` (trace-логирование),
`19ab37316` (`HostProcessBackend` — безусловный GPU-хост-процесс на окно) и `2e227e77b`.

Исходные документы: `2026-08-21-ghost-cursor-after-refocus-tab-switch.md`,
`2026-08-21-ghost-cursor-retained-stamp-refutation.md`,
`2026-08-21-ghost-cursor-consolidated-analysis.md`, `docs/plans/2026-08-21-ghost-cursor-fix-plan.md`.

Терминология статуса: **PROVEN** — прямое следствие кода с цитатой `file:line`; **SUSPECTED** —
механизм достижим по коду, но нет runtime-подтверждения, что именно он сработал у пользователя;
**UNCONFIRMED** — гипотеза без достаточной опоры.

## Итог (ранжировано по уверенности, что объясняет повторяющийся симптом)

| # | Находка | Статус | Тяжесть |
|---|---|---|---|
| A | `HostProcessBackend` заново реализовал handshake `in_flight`/`repaint_pending` с `Release/Acquire` — тот самый lost-wakeup, который для in-process render thread был найден и закрыт в `0e9807e0d`/`aaf1f8f58` (SeqCst). Потерянный wakeup = свежепостроенный кадр отброшен, экран **застывает на предыдущем** до постороннего события. С дефолтным `SteadyBlock`-курсором периодических перерисовок нет, поэтому «предыдущий» кадр висит, пока пользователь не нажмёт клавишу | PROVEN как регрессия кода; SUSPECTED как причина инцидента (окно гонки узкое, следа в логах нет) | Высокая |
| B | Рассинхронизированный render-snapshot: `paint_pane` берёт `terminal.lock()` трижды раздельно, а `perform_actions_chunked` отпускает lock каждые 256 действий — в том числе внутри одного DEC 2026 synchronized-кадра. Даёт кадр «курсор из t0, строки из t2» = ровно «курсор на одной строке, промпт на другой». Сам по себе транзиентен (следующий `PaneOutput` перерисует); **устойчивым его делают только A/C/D** | PROVEN достижимость; транзиентность PROVEN | Средняя сама по себе, высокая в связке с A |
| C | Дочерний `gpu-tab-host`: `submit_frame` вернул `Err` → ack не отправлен, процесс продолжает жить → у родителя `in_flight` залипает `true` навсегда, `render_thread_is_hung()` всегда `false` → окно **перестаёт обновляться навсегда** | PROVEN латентный дефект; 0 срабатываний в логах | Критическая при срабатывании |
| D | `last_frame_signature` фиксируется до того, как кадр реально ушёл ребёнку; при смерти ребёнка / провале `send` потерянный кадр не пересылается — после respawn первый repaint отбрасывается по совпадению сигнатуры, новая генерация не получает кадра и не делает `swap_visual_content` → экран висит на кадре *до* потерянного, пока содержимое не изменится | PROVEN латентный; 0 respawn'ов в логах | Средняя |
| E | Retained rows: закрытый в `0c7fbab37` дефект действительно закрыт; `RetainedStamp` по-прежнему не содержит origin viewport'а, поэтому после прокрутки `EmitRetained` пере-эмитит **прошлое содержимое слота** (дублированная строка текста) на кадр-два. Курсор защищён `contains_cursor`. Не объясняет симптом | PROVEN | Низкая (текст, транзиентно) |
| F | Busy-spin GUI-потока: `wm_paint` в throttled-режиме возвращается без `BeginPaint`/`ValidateRect`, Windows немедленно синтезирует следующий `WM_PAINT`, `run_message_loop` крутится до истечения throttle (до 1000/`max_fps` = 16 мс) после каждого кадра, за которым пришёл invalidate | PROVEN | Средняя (CPU) |
| G | Кадр строится целиком (`paint_pass`) и **только потом** отбрасывается по `is_in_flight()` — при host-process backend `in_flight` длится всё время pipe+GPU+ack, доля выброшенной работы под потоком вывода велика | PROVEN | Средняя (CPU) |
| H | Системные снапшоты процессов (`CreateToolhelp32Snapshot` + `OpenProcess` на каждый процесс машины) каждые 300 мс на каждую рисуемую панель (`bidi_disabled_by_foreground_process`, дефолтный список `claude.exe`) и на активную панель каждой вкладки при обновлении заголовка | PROVEN | Средняя (CPU в фоне) |
| I | `retained_rows`/`pane_state`/`semantic_zones` никогда не удаляют записи закрытых панелей — медленная утечка пропорционально числу когда-либо открытых панелей (до сотен КБ на панель в `retained_rows`) | PROVEN | Низкая–средняя |
| J | Ветка `fix/hostprocess-oom` (`cbd71a9ff`) полностью поглощена `32b92900a` на `main` (тот же diff под тем же заголовком, в тот же день) и с тех пор улучшена (`8cbb27818`, `2e227e77b`). Rebase не нужен; ветку можно удалить | PROVEN | — |

Главный вывод: единственный найденный в текущем коде путь, который превращает транзиентный
«порванный» кадр (B) в **устойчивый**, — это отброс уже построенного кадра без последующей
перерисовки. Таких путей три, все три — в `HostProcessBackend` (A, C, D), который стал
безусловным для каждого окна в `19ab37316` (`render_pipeline.rs:378-395`) — то есть уже после
сборки `7cd27f37e`, на которой был первый инцидент, но до сборки `e0f07e50a`, на которой симптом
повторяется сейчас. Для in-process пути аналог A был закрыт SeqCst-ом за десять дней до
появления host-process кода, и `HostProcessBackend` эту гарантию не унаследовал.

## 1. Второй триггер: где кадр с неверным курсором может *застыть*

### 1.1. Что уже закрыто и что говорят логи о retained rows

`0c7fbab37` добавил `RetainedRow.contains_cursor` (`render/mod.rs:145-150`), заполняемый в обеих
ветках записи (`render/pane.rs:660-664`, `773-777`) и форсирующий `Build` в
`RowSweep::decide` (`render/budget.rs:104-107`). Путь «`EmitRetained` слота со старым курсором»
недостижим: `must_build` включает `retained_contains_cursor` без учёта дедлайна и `work_start`.
Это перепроверено; регрессии нет.

Логи подтверждают, что retained-путь при переключении вкладок жив и потому фикс не был холостым:
из 750 событий `activate_tab:` за 604 в пределах трёх строк следует
`retained rows reset on stamp mismatch` (539 — с изменившимся `quad_generation`, т.е. между
последним рендером вкладки и возвратом был focus-bump; 65 — с тем же поколением, т.е. штамп
разошёлся по другому полю: у всех таких случаев в окне только что появилась/пропала вкладка,
что меняет `top_pixel_y` через высоту tab bar). У оставшихся ~146 переключений сброса нет —
штамп совпал, retained-строки переиспользовались, и защищал их ровно `contains_cursor`.

Отдельно проверено, что «курсор выше viewport'а» (`cursor.y < viewport_top`, невозможно при
согласованном снимке, т.к. `set_viewport` ограничивает позицию `< physical_top`,
`actions.rs:2000-2010`) не встречается ни разу в 12 797 строках `retained rows reset`
(777 — курсор ниже viewport'а, т.е. пользователь прокрутил назад; 12 020 — внутри). Это
отрицательный результат ограниченной силы: сброс штампа — редкий и смещённый момент выборки,
и раса B в нём видна только в самом грубом своём проявлении.

### 1.2. Находка A — lost wakeup в `HostProcessBackend` (PROVEN как регрессия, SUSPECTED как причина)

GUI-поток, `render/draw.rs:271-289`:

```rust
if rt.is_in_flight() {                 // in_flight.load(Acquire)   host_process_backend.rs:701-703
    rt.set_repaint_pending();          // repaint_pending.store(true, Release)          :705-707
    ...
    if !rt.is_in_flight() {            // in_flight.load(Acquire)
        win.invalidate();
    }
    return Ok(());
}
```

Reader-поток ребёнка, `host_process_backend.rs:518-556` (`on_presented`):

```rust
shared.in_flight.store(false, Ordering::Release);      // :530
...два mutex lock/unlock, возможно swap_visual_content...
if shared.repaint_pending.swap(false, Ordering::AcqRel) { (shared.invalidate)(); }  // :553-555
```

Пара «store в `repaint_pending`, затем load `in_flight`» на GUI-потоке — это store→load разных
адресов. `Release/Acquire` такую пару не упорядочивают; на x86 это единственное разрешённое
переупорядочивание (store buffer): load `in_flight` может исполниться до того, как store
`repaint_pending` станет виден. Тогда возможна последовательность: GUI читает
`in_flight == true` (оба раза), reader публикует `in_flight = false` и делает
`swap(repaint_pending)` → видит ещё не выехавший из буфера `false` → не инвалидирует; затем
store GUI-потока становится виден. Итог: `in_flight == false`, `repaint_pending == true`,
никто ничего не перерисовывает. Кадр, который только что был построен, отброшен; экран
показывает *предыдущий* отправленный кадр.

Это дословно тот дефект, который уже был найден и закрыт для in-process пути:

- `0e9807e0d` (2026-08-11, «fix lost-wakeup race in the WebGPU backpressure check»): «freshly
  built content could sit unpainted until an unrelated event (input, new pane output) happened
  to invalidate the window» — описание симптома совпадает с наблюдением пользователя;
- `aaf1f8f58` (2026-08-11, «use SeqCst for the in_flight/repaint_pending handshake»): «with
  Release/Acquire on both sides this pairing has no StoreLoad ordering … formally the original
  lost-wakeup is still reachable». Все четыре операции in-process handshake переведены на
  SeqCst: `onlyterm-gui-render-thread/src/lib.rs:470-472`, `479-481`, `489-492` — с
  комментарием, объясняющим именно этот случай (`lib.rs:461-469`).

`HostProcessBackend` (`19ab37316`, 2026-08-21) не переиспользовал эти helper'ы и написал
handshake заново с `Release/Acquire` (`host_process_backend.rs:530`, `553`, `680`, `702`, `706`).
`call_draw_webgpu` обращается к обоим backend'ам через один trait, поэтому «двойная проверка»
из `0e9807e0d` формально осталась, но без SeqCst она снова вероятностная.

Почему это кандидат №1 именно на *устойчивый* призрак:

1. `blinking` в `compute_cell_fg_bg` требует `cursor_shape.is_blinking()`
   (`render/mod.rs:882-886`); дефолт `default_cursor_style` — `SteadyBlock`
   (`config_types.rs:6-14`), а `effective_shape` (`config_types.rs:17-23`) подменяет им
   `CursorShape::Default`. Без мигания `has_animation` не взводится, таймер в `paint_impl`
   (`render/paint.rs:221-253`) не планируется: у бездействующего окна **нет ни одной
   периодической перерисовки**. Что было показано последним — то и остаётся.
2. Единственное, что снимет застывший кадр, — новый `invalidate()`: клавиша, вывод панели,
   смена фокуса, alert заголовка. Пользователь замечает «поле ввода не на строке промпта» именно
   когда возвращается к окну *до* нажатия клавиши; первая же клавиша всё лечит — это согласуется
   с описанием «появляется иногда, само проходит».
3. Окно гонки — латентность выезда store buffer, десятки наносекунд; но handshake исполняется
   при каждой коллизии «пришёл paint, пока ребёнок ещё presents». В host-process пути `in_flight`
   держится всё время сериализации 300–435 КБ (`draw.rs:531-549`, комментарий), записи в pipe,
   `create_buffer_init` и `submit_frame` у ребёнка и чтения ack (`gpu_tab_host.rs:322-366`,
   `host_process_backend.rs:493-516`) — миллисекунды. Под spinner'ом TUI (10 Гц перерисовок)
   коллизии идут постоянно; за многочасовую сессию редкое событие набирает шансы.

Что *не* доказано: что именно эта гонка сработала в конкретном кадре пользователя. Следов
в логах быть не может — `gui.host_process.frames_dropped` (`host_process_backend.rs:682`)
и `gui.render_thread.frames_dropped` (`draw.rs:274`) существуют только как metrics-счётчики,
строк в лог они не пишут; fallback-ветка `!rt.is_in_flight()` тоже молчит.

Направление исправления: (а) перевести `is_in_flight`/`set_repaint_pending`/`on_presented` на
`SeqCst` (или сделать `set_repaint_pending` через `swap`, RMW на x86 — полный барьер), лучше —
вызывать `in_flight_is_set`/`mark_repaint_pending`/`finish_in_flight_frame` из
`onlyterm-gui-render-thread`, чтобы гарантия жила в одном месте; (б) добавить `log::info!`
(с rate-limit) в обе ветки отброса кадра и в fallback-инвалидацию, чтобы следующий инцидент
можно было сопоставить по времени с `activate_tab:`/`focus_changed:`.

### 1.3. Находка B — рассинхронизированный snapshot и chunked apply внутри DEC 2026 (PROVEN достижимость)

`paint_pane` (`render/pane.rs`) читает состояние терминала тремя независимыми захватами
`Mutex<Terminal>`:

- `pos.pane.get_cursor_position()` — `:100` → `localpane.rs:364-370`;
- `pos.pane.get_dimensions()` — `:107` → `localpane.rs:420-422`;
- `pos.pane.get_lines(stable_range)` — `:864` → `with_lines_mut` (`localpane.rs:404-414`).

Плюс `check_for_dirty_lines_and_invalidate_selection` (`render_pipeline.rs:1968-2004`) и
`apply_hyperlinks` (`pane.rs:335`) — ещё два захвата до этого. Между любыми двумя из них
поток парсера pty может применить вывод.

Ключевое усиление, которого нет в consolidated-отчёте: **вывод применяется не батчами, а
чанками с отпусканием lock'а.** `perform_actions` → `perform_actions_chunked`
(`localpane.rs:734-736`, `1514-1537`): батч длиннее `mux_output_parser_chunk_size` (256,
`config.rs:1127-1136`) режется на куски по 256 действий, и между кусками `terminal.lock()`
отпускается (по дизайну task #147; `resize_guard` держится на весь батч, но рендер его не берёт).
`parse_buffered_data` при DEC 2026 (`pty_reader.rs:108-161`) копит весь synchronized-кадр и
отдаёт его одним `send_actions_to_mux` — который затем применяется по 256 действий с окнами для
рендера между ними. Полная перерисовка 53-строчного экрана TUI (атрибуты + текст + перемещения
курсора) — это тысячи действий, т.е. десятки окон по ~3 мс (`config.rs:1129-1131`:
~12 мкс/действие). Любой paint, попавший в такое окно, получает курсор из одного чанка и
строки из другого — независимо от того, что приложение просило синхронизированный вывод.

Как это выглядит: курсор (t0) стоит на строке старого промпта/поля ввода `P`; между t0 и t2
приложение перерисовало блок, и поле ввода уехало на `P+k`; в кадре строка `P` рисуется с
курсорным блоком (`screen_line.rs:108-120`, `317-434`: `cursor_cell`/`cursor_range` определяются
по `stable_line_idx == cursor.y`, без проверки согласованности), а свежая строка `P+k` — без
курсора. Это и есть «поле ввода и промпт на разных строках».

Транзиентность (PROVEN): `send_actions_to_mux` шлёт `PaneOutput` после `perform_actions`
(`pty_reader.rs:40-42`); коалесинг в `Mux::notify_from_any_thread`/`Mux::notify`
(`mux/src/lib.rs:616-648`, `470-597`) гарантирует хотя бы одну доставку после последнего
батча (`coalesce_count` увеличился во время доставки → повторный раунд; иначе запись
удаляется и следующий батч планируется заново). Доставка → `mux_pane_output_event` →
`win.invalidate()` (`render_pipeline.rs:1738-1765`), для видимой панели. `WM_PAINT`-путь
инвалидацию не теряет: `wm_paint` выставляет `invalidated=false` до диспетчеризации
`NeedRepaint` (`window.rs:3105-3107`), а throttled-ветка помечает `invalidated=true` и таймер
делает `InvalidateRect` (`window.rs:3055-3058`, `3109-3122`). Следовательно, после последнего
батча всегда будет ещё один paint с согласованным состоянием — если только *построенный* кадр
не будет отброшен без повторной перерисовки (A, C, D).

Именно поэтому B — необходимое, но не достаточное условие. В сборке `7cd27f37e` (первый
инцидент, in-process render thread с уже SeqCst-handshake) B давал только транзиентные кадры
≤ 1–2 frame'ов; такие кадры мог сделать устойчивыми только retained-путь, закрытый в
`0c7fbab37`. В сборке `e0f07e50a` retained-путь закрыт, зато появился A.

Направление исправления: Phase C плана (единый snapshot под одним коротким lock'ом: cursor,
dims, клон строк) — она устраняет «курсор из t0, строки из t2» внутри одного кадра, но не
«кадр посреди synchronized-батча». Для второго нужно либо не резать батч, пришедший из
DEC 2026 flush'а, на lock-отпускающие чанки (можно применить его целиком под lock'ом, но
это возвращает проблему task #147 для ввода), либо ввести на `Terminal` «барьер рендера»:
парсер поднимает флаг `applying_batch` на весь батч, рендер при поднятом флаге ждёт/пропускает
кадр (bounded), а ввод продолжает чередоваться между чанками как сейчас.

### 1.4. Находка C — `submit_frame` у ребёнка вернул `Err`: `in_flight` залипает навсегда (PROVEN, латентно)

`gpu_tab_host.rs:348-377`:

```rust
match result {
    Ok(Ok(())) => { presented_seq += 1; write_presented(...) }          // :352-366
    Ok(Err(err)) => { log::error!("gpu-tab-host: submit_frame failed: {err:?}"); } // :367-369
    Err(_)      => { write_fatal(3); break; }                            // :370-376
}
```

`Ok(Err)` — это `wgpu::SurfaceError` из `get_current_texture()?` (`state_impl.rs:239`):
`Timeout`/`Outdated`/`Lost`/`OutOfMemory`. Ребёнок логирует и продолжает цикл, **ack не
пишет**. У родителя `in_flight` был выставлен в `send_frame` (`host_process_backend.rs:680`) и
сбрасывается только в `on_presented` (`:530`), `spawn_generation` (`:375`) и при демоции
(`:606`, `:654`). Ничего из этого не случится: ребёнок жив. Дальше каждый `call_draw_webgpu`
уходит в ветку `is_in_flight()` (`draw.rs:271-290`), кадр отбрасывается, `repaint_pending`
некому обработать. `render_thread_is_hung()` для этого backend'а всегда `false`
(`host_process_backend.rs:716-722`), `submit_started_at` заполняется (`:685`), но никем не
проверяется. Окно перестаёт обновляться навсегда; ресайзы продолжают доходить до ребёнка
(`send_resize`, `:659-670`), что ничего не меняет.

In-process путь этот случай обрабатывает: `submit_one_frame` (`render-thread/src/lib.rs:723-818`)
на `Lost|Outdated` делает `reconfigure()` + `invalidate()`, на `OutOfMemory` — `invalidate()`,
иначе — rebuild через circuit breaker, и в любом случае `finish_in_flight_frame` (`:825-827`).

Логи: строка `submit_frame failed` не встречается ни в одном из 42 логов дочерних процессов.
Значит, в наблюдаемом периоде это не срабатывало; но `get_current_texture` с `Timeout`/`Outdated`
— штатные состояния DXGI при смене размера/окклюзии, и вероятность за время жизни долгой сессии
ненулевая. Симптом при срабатывании — «окно замёрзло целиком», что пользователь описал бы иначе;
поэтому C не претендует на объяснение инцидента, но по тяжести — первый кандидат на исправление
в этом семействе.

Направление: (а) ребёнок на `Ok(Err(Lost|Outdated))` делает `state.reconfigure()` и **всё равно
шлёт ack** (или отдельное сообщение `Failed{seq}`), на прочие ошибки — `write_fatal` + выход,
чтобы родитель respawn'нул; (б) родитель: `render_thread_is_hung()` возвращать
`is_hung_given(submit_started_at, threshold)` как в in-process handle (`lib.rs:448-453`) —
супервизор окна тогда переберёт backend через существующий rebuild-путь.

### 1.5. Находка D — сигнатура кадра фиксируется до доставки; после respawn кадр не пересылается (PROVEN, латентно)

`draw.rs:296-307`: сигнатура вычисляется и **записывается в `last_frame_signature` до**
`send_frame`. Пути, на которых кадр после этого теряется без перерисовки:

- `send_frame` → `writer_tx.send(...)` вернул `Err` (writer-поток умершей генерации уже вышел из
  `for msg in rx`, `host_process_backend.rs:437-470`) → `in_flight = false`, кадр потерян, ни
  `repaint_pending`, ни `invalidate` (`:696-698`);
- writer записал кадр в pipe, ребёнок умер до present → `handle_child_death` → (после backoff)
  `spawn_generation` → `invalidate()` (`:404`) → paint → `compute_frame_signature` совпала с
  сигнатурой потерянного кадра → `gui.frame.skipped`, `return` (`draw.rs:300-305`) **до**
  `frame_form()` — новая генерация не получает ни кадра, ни `atlas_reset`, `on_presented` не
  наступает, `swap_visual_content` не вызывается: DirectComposition-визуал продолжает показывать
  поверхность мёртвой генерации (по дизайну, `host_process_backend.rs:17-23`, `393-396`).
  Экран висит на кадре *до* потерянного, пока содержимое не изменится и сигнатура не разойдётся.

`0e9807e0d` описывает ту же ловушку для старого `send_frame` («the frame signature also got
updated before the drop, so an identical follow-up frame could additionally be skipped by
signature match»); в host-process пути она вернулась через новые точки потери. Сброс
`last_frame_signature` есть только в `resize` (`resize.rs:60`), `finish_renderer_rebuild`
(`render_pipeline.rs:1144`) и `invalidate_atlas_dependent_caches` (`render/mod.rs:1100`) —
respawn ребёнка ни одну из них не задевает.

Логи: 46 строк `generation N running`, все с `generation 0` (по одному на процесс) — respawn'ов
в наблюдаемом периоде не было. Латентно.

Направление: в `call_draw_webgpu` спрашивать `frame_form()` **до** проверки сигнатуры и при
`full_resync == true` сбрасывать `last_frame_signature`; в `send_frame` на `Err` от
`writer_tx.send` выставлять `repaint_pending` и звать `invalidate` (или сбрасывать сигнатуру
через callback), а не только `in_flight = false`.

### 1.6. Реконструкция сценария «вернул фокус → переключил вкладку» на текущем коде

Все шаги, кроме последнего, — PROVEN по коду; последний — вероятностный (A).

1. `WM_SETFOCUS` → `focus_changed(true)` (`window_handler.rs:29-65`): `quad_generation += 1`,
   `invalidate()`, `pane.focus_changed(true)` для активной панели T0 →
   `Terminal::focus_changed` при включённом `focus_tracking` пишет в pty `CSI I`
   (`term/src/terminalstate/mod.rs:835-838`). Логи: `focus_changed: focused=true …` встречается
   тысячи раз, всегда с последующим `retained rows reset` через 2–30 мс.
2. Пользователь переключается на T1: `activate_tab` (`actions.rs:629-668`) →
   `save_and_then_set_active`, **`pane.focus_changed(true)` для панели T1** (`:660-662`) → в pty
   T1 уходит `CSI I`; `update_title()` → `invalidate()`. TUI в T1 (Claude Code и подобные
   включают focus reporting) реагирует на focus-in немедленной перерисовкой: вывод прилетает
   ровно тогда, когда GUI-поток начинает первый paint T1.
3. Paint T1: stamp mismatch (поколение изменилось на шаге 1) → полный rebuild всех строк
   (`pane.rs:443-484`; `budget.rs:104` — `!has_retained` → `Build` без бюджета). Rebuild
   длинный, батч из шага 2 применяется параллельно чанками — вероятность порванного snapshot'а
   (B) на этом кадре максимальна. Кадр F1 (курсор на строке до перерисовки, поле ввода уже
   ниже) уходит ребёнку, `in_flight = true`.
4. Батч завершён → `PaneOutput` → `invalidate()` → paint F2 (согласованный) →
   `call_draw_webgpu`: F1 ещё in flight (pipe + GPU + ack) → F2 отброшен,
   `set_repaint_pending()`.
5. Если ack F1 попал в окно между двумя load'ами `in_flight` на GUI-потоке (A) — F2 никто не
   перерисует. С `SteadyBlock` нет мигания; экран показывает F1 до следующей клавиши/вывода.

Обе фокусные посылки `CSI I` (шаги 1 и 2) — это то, что делает *именно* «refocus + tab switch»
плотным источником свежего вывода в момент рендера, а не просто произвольным моментом.

### 1.7. Проверенные и отвергнутые пути потери перерисовки (отрицательные результаты)

- **`Mux` коалесинг `PaneOutput`** (`mux/src/lib.rs:470-597`, `616-648`): состояние
  `(in_flight, has_more, count)` очищается в конце каждой доставки либо переводится в новый раунд;
  залипание `in_flight=true` возможно только при панике подписчика (в логах паник нет) или
  недоставке `spawn_into_main_thread` (в рабочем режиме `Mux::try_get()` всегда `Some`).
  Не теряет.
- **`is_pane_visible`-gate** (`render_pipeline.rs:1718-1736`, `1760-1764`): вывод невидимой
  вкладки не инвалидирует, но `activate_tab` сам инвалидирует через `update_title`, а любой
  последующий вывод уже видимой панели — через gate. Не теряет.
- **`WM_PAINT`/`InvalidateRect`** (`window.rs:2106-2114`, `3051-3125`): инвалидация из любого
  потока во время paint переживает `BeginPaint`/`EndPaint` (они в начале, до
  `NeedRepaint`), throttled-ветка сохраняет флаг. Не теряет (но см. §3.1 о spin).
- **In-process render thread** (`render-thread/src/lib.rs:622-828`): каждая ветка ошибки
  заканчивается `invalidate()` или rebuild'ом; handshake SeqCst. Не теряет.
- **`resizes_pending`** (`render_pipeline.rs:1371-1373`, `1317-1327`): `SetInnerSizeCompleted`
  диспетчеризуется только в ветке `can_resize()` (`window.rs:2174-2200`); `apply_dimensions`
  не вызывает `set_inner_size` при `!can_resize()` (`resize.rs:155-162`), поэтому рассинхрон
  возможен лишь если состояние окна изменилось между этими двумя проверками. Узко; при
  срабатывании — полная остановка перерисовок, не призрак. UNCONFIRMED как причина.
- **`cursor_row_slot` vs `stable_top`** (`pane.rs:420-421`, `864`; `screen.rs:516-535`):
  origin слота и origin строк совпадают, кроме случая клампа `stable_range` (буфер короче
  viewport'а — только при старте). Не причина.
- **Ресайз-гонка `9c8ba85b9`**: см. §2.2 — остаточное окно есть, но самоисцеляется следующей
  перерисовкой ConPTY; в модель терминала оно кладёт *содержимое* не туда, а не курсор.
- **`focus_changed` не дошёл до `TermWindow`** (вариант из consolidated §3): в логах каждое
  `activate_tab` после долгой паузы окружено `focus_changed`-парами — этот вариант не
  подтверждается.

### 1.8. Сводка по логам

- Trace-логирование `58097a015` работает: 12 797 сбросов штампа, 750 переключений вкладок,
  ~9 000 смен фокуса. Ни одна строка не противоречит консистентности снимка в момент сброса
  (§1.1), но момент сброса — плохая точка наблюдения для B.
- В логах **нет** строк `submit_frame failed`, `Fatal(`, `respawn`, `demot`, `generation ≥ 1` —
  C и D не срабатывали.
- Для A наблюдение невозможно: нет ни одной строки на пути отброса кадра. Это главный пробел
  инструментирования, см. §5.

## 2. Прочие дефекты корректности кадра

### 2.1. `RetainedStamp` не содержит origin viewport'а → пере-эмит прошлого содержимого слота (PROVEN, транзиентно)

`RetainedPaneRows.rows` индексируется визуальным слотом (`render/mod.rs:133-136`), а
`RetainedStamp` (`:156-171`) не включает `stable_top`/`viewport_top`. После прокрутки на `k`
строк содержимое слота `N` уезжает в слот `N-k`, где оно — cache hit (ключ
`LineQuadCacheKey` позиционно-независим, `:84-87`) и получает `EmitCached`. Слот `N` с новым
содержимым — cache miss; если он не cursor row, не rotation target и не `contains_cursor`, то
при просроченном дедлайне **или просто при `slot < work_start`** (`budget.rs:111-122`) он
получает `EmitRetained` = квады *прежнего* содержимого слота `N`, которое уже показано в `N-k`.
Визуально — дублированная строка текста на кадр, до прихода ротации. Курсорные слоты защищены.
Это объясняет «мусорную строку мелькнула при быстром выводе», не призрак курсора.

Направление: добавить в `RetainedStamp` `viewport_top` (полный сброс при любой прокрутке —
дорого при потоке вывода) либо, точнее, при смене origin сдвигать `rows` на `k`
(`rotate`/`drain`) вместо сброса — retained-квады позиционно-независимы, так что сдвиг
корректен.

### 2.2. Ресайз-гонка `9c8ba85b9`: остаточное окно (PROVEN, самоисцеляется)

Фикс держит `terminal.lock()` на pty-resize + term-resize (`localpane.rs:777-807`) и
`resize_guard` на весь chunked-батч (`:1529`). Но батчи `≤ chunk_size` идут коротким путём без
`resize_guard` (`:1515-1527`, комментарий «a single lock acquisition can't be sliced by a
concurrent resize» — верно для одного батча). Перерисовка ConPTY после ресайза прилетает не
одним батчем: парсер флашит каждые `mux_output_parser_coalesce_delay_ms` = 3 мс
(`pty_reader.rs:249-273`), и следующий ресайз (live-resize даёт их десятками) может встать
между двумя короткими батчами одной логической перерисовки. Хвост применится под новой
геометрией — та же картина «текст на строку ниже», которую фикс лечил для длинных батчей.
ConPTY после последнего ресайза перерисовывает всё заново, поэтому дефект не задерживается.
Полное закрытие — держать `resize_guard` и в коротком пути, либо не отпускать его между
флашами, пока парсер «внутри» ответа на ресайз (сложно определить). Низкий приоритет.

### 2.3. Мелкое

- `stable_range` кламп (`screen.rs:516-535`): при буфере короче `range_len` возвращается меньше
  строк, нижние слоты панели не эмитятся вовсе (ни свежие, ни retained) — только при старте
  панели, невидимо.
- `contains_cursor` ставится по `cursor.y == stable_row` даже для неактивной панели
  (`pane.rs:663`, `776`), тогда как `render_screen_line` рисует курсор для любой панели
  (`screen_line.rs:108-120`). Консервативно и корректно.
- `EmitRetained` не проверяет `expires` retained-строки на истечение до эмита (`pane.rs:788-807`)
  — анимированный retained-слот может показать устаревший кадр анимации. Косметика.

## 3. CPU / GPU / память

### 3.1. Busy-spin `WM_PAINT` во время throttle (PROVEN)

`wm_paint` (`window.rs:3051-3125`): при `paint_throttled` ветка `:3055-3058` ставит
`invalidated = true` и возвращает `Some(0)` — **без `BeginPaint`/`EndPaint`/`ValidateRect`**,
`wnd_proc` при `Some` не зовёт `DefWindowProcW` (`window.rs:4284-4300`). Регион обновления
остаётся непустым, и `PeekMessageW` синтезирует `WM_PAINT` снова на следующей же итерации
`run_message_loop` (`connection.rs:104-141`: `SPAWN_QUEUE.run()` → `PeekMessageW(PM_REMOVE)` →
`DispatchMessageW`, `wait_message` только при пустой очереди). Цикл крутится без сна до
срабатывания таймера `:3112-3121` (1000/`max_fps` = 16 мс при дефолте 60). Это происходит при
каждом invalidate, пришедшем в течение 16 мс после кадра — под потоком вывода/spinner'ом
практически после каждого кадра: GUI-поток перестаёт когда-либо блокироваться в
`MsgWaitForMultipleObjects`. `record_heartbeat` (`:108`) тикает на каждой итерации, поэтому
watchdog spin не видит.

Направление: в throttled-ветке вызвать `ValidateRect(hwnd, null)` (или `BeginPaint`/`EndPaint`)
и полагаться только на свой `InvalidateRect` из таймера; альтернатива — не throttle'ить в
`wm_paint`, а в `invalidate()`.

### 3.2. Кадр строится, потом выбрасывается по `in_flight` (PROVEN)

Порядок в `paint_impl` (`render/paint.rs:55-143`): полный `paint_pass` (все строки,
`apply_to_translated` каждой, hash сигнатуры всех инстансов) → `call_draw` →
`call_draw_webgpu`, и только там (`draw.rs:271-290`) проверка `is_in_flight()`. Комментарий
«Check render-thread back-pressure BEFORE building the frame» относится к GPU-буферам, а не к
`paint_pass`. При host-process backend `in_flight` покрывает сериализацию (300–435 КБ),
pipe, `create_buffer_init` на каждый draw у ребёнка, `submit_frame`/present и ack — типично
единицы мс, при vsync-ожидании до 16 мс. При потоке вывода (`PaneOutput` каждые 3 мс) большая
доля paint'ов попадает в in-flight и выбрасывается целиком, чтобы быть построенной заново по
ack'у (`on_presented` → `invalidate`). Двойная работа шейпинга/квадов на самом дорогом пути.

Направление: проверять `is_in_flight()` в начале `NeedRepaint`/`paint_impl`: если in flight —
`set_repaint_pending()` и выход без `paint_pass`.

### 3.3. Эффективность пропуска идентичных кадров (`c48fc2563`)

Механизм (`draw.rs:296-307`, `96-124`) работает как задуман: детерминированный ahash по всем
инстансам + размеры + uniform без `milliseconds`. Обходных путей, форсирующих submit каждый
кадр при статичном экране, не найдено: мигание выключено дефолтом (§1.2), прогресс-spinner
tab bar только при `Alert::Progress`. Ограничения: (а) проверка стоит полного `paint_pass` +
hash всех байт инстансов (`~53 × cols` квадов × 84 байта, `onlyterm-gpu-protocol/src/quad.rs:32`)
на каждый кандидат-кадр; (б) `last_frame_signature` — состояние GUI, а не факт показа: см. D;
(в) при host-process backend каждый *непропущенный* кадр всё равно сериализуется целиком —
дельты по строкам нет, хотя retained-квады позиционно-стабильны и дельта была бы дешёвой.

### 3.4. Шторм снапшотов процессов (PROVEN)

`bidi_disabled_by_foreground_process` (`render/mod.rs:1251-1272`) вызывается раз на панель на
кадр (`pane.rs:400-403`) и при непустом `disable_bidi_for_processes_named` (дефолт
`["claude.exe", "claude"]`, `config.rs:1339-1344`) идёт в `get_foreground_process_name(AllowStale)`
→ `divine_process_list` (`localpane.rs:1668-1730`): при `expired()` (TTL 300 мс, `:39`) —
фоновый `compute_proc_info` (`:1568-1609`) = `LocalProcessInfo::with_root_pid` →
`CreateToolhelp32Snapshot` всего процессного списка машины + `ProcHandle::new`/`get_params` на
**каждый** процесс (`procinfo/src/windows.rs:30`, `408-432`). То же — для активной панели
каждой вкладки при обновлении заголовка (`actions.rs:2096-2107`, `2109-2148`;
`update_title_impl` вызывается на каждую клавишу/фокус/alert, storm-пути ограничены 100 мс).
Под TUI, непрерывно шлющим title/progress OSC, это до ~3.3 полных снапшотов системы в секунду
на панель × число вкладок — фоновый CPU, растущий с числом процессов на машине и числом
вкладок. Кэш (300 мс) не даёт снапшотам совпасть между панелями: у каждой свой `proc_list`.

Направление: один общий процессный снапшот на процесс с TTL (а не на панель), либо для bidi —
вычислять флаг по событию (смена foreground-процесса), а не опрашивать на каждом кадре.

### 3.5. Утечки на закрытии панелей (PROVEN)

- `retained_rows: HashMap<PaneId, RetainedPaneRows>` (`termwindow/mod.rs:385`): записи только
  вставляются/обновляются в `paint_pane` (`pane.rs:460-481`); `remove`/`clear` нет нигде
  (grep по crate). Каждая запись — `Vec<Option<RetainedRow>>` на `viewport_rows` слотов с
  `Rc<HeapQuadAllocator>` (квады строки, 84 байта на квад; ~1–3 квада на занятую ячейку). Для
  53 × 150 ячеек — порядка 0.5–1 МБ на панель в худшем случае, десятки–сотни КБ типично.
- `pane_state` (`:349`) и `semantic_zones` (`:350`) — аналогично без удаления (`clear_all_overlays`
  чистит только при уничтожении окна). Малы.
- LFU-кэши (`line_quad_cache`, `shape_hash_cache`, `line_to_ele_shape_cache`, по 1024,
  `config.rs:1456-1474`) ограничены и содержат `pane_id` в ключе — вытесняются сами.

Направление: обрабатывать `MuxNotification::PaneRemoved` (сейчас `render_pipeline.rs:1628-1634`
— пусто) и удалять `pane_id` из всех трёх карт.

### 3.6. Атлас глифов: только рост, тихая демоция (PROVEN)

`glyph_cache: AHashMap<GlyphKey, Rc<CachedGlyph>>` (`glyphcache/mod.rs:194`) без eviction;
атлас растёт удвоением (`window/src/bitmaps/atlas.rs:117-121`) и никогда не сжимается;
`recreate_texture_atlas` (`render/mod.rs:1110-1126`) сбрасывает все кэши и пересоздаёт
`GlyphCache`. Зеркало для host-process (`AtlasMirrorLog.written`, `webgpu/mod.rs:277-331`)
ограничено `MAX_ATLAS_MIRROR_BYTES` = 128 МиБ (`:248`); при превышении — `over_budget` →
`mirroring_failed()` → `atlas_mirroring_failed()` → **тихая демоция окна на in-process
рендер** (`draw.rs:481-487`, `host_process_backend.rs:648-657`, только `log::error!`). Атлас
8192² уже даёт 256 МиБ пикселей, т.е. долгая сессия с большим набором глифов (CJK/emoji/много
размеров) молча теряет crash-изоляцию. Не утечка, но незаметная смена режима.

### 3.7. Прочее

- `status_update_interval` (1 с, `config.rs:1378-1380`) → `schedule_next_status_update`
  (`actions.rs:544-559`) → `EmitStatusUpdate` → `emit_status_event` — no-op
  (`render_pipeline.rs:1886-1889`). Ежесекундный холостой таймер на окно.
- 1047 строк `ERROR impossible C0/C1 control code '\u{81}'` в логах — синхронная запись в
  файл на потоке парсера на каждый байт `0x81` (вероятно, сломанная кодировка чьего-то вывода).
  Стоит понизить до `debug`/rate-limit.
- `apply_dimensions` ресайзит **все** вкладки окна (`resize.rs:299-304`) — ConPTY RPC на
  каждую панель на GUI-потоке при каждом событии ресайза, включая изменение только
  `window_state`.
- Watchdog: 61 stall'ов GUI-потока ≥ 4 с (реально 5–28 с) в логах. Они синхронны в трёх
  процессах одновременно (23:38–23:43 и 10:33–10:47 в `24616`, `10936`, `11580`) — причина
  системная (память/GPU/драйвер), не per-process; вокруг них в логах ничего нет. Полезно при
  срабатывании писать последнее диспетчеризованное сообщение/задачу.

## 4. Ветка `fix/hostprocess-oom` (`cbd71a9ff`)

Факты:

- `git cherry main fix/hostprocess-oom` → `+ cbd71a9ff` (patch-id не совпадает, т.к. пути
  изменились), но `32b92900a` на `main` («gui: bound host-process atlas mirror memory»,
  2026-08-21, тот же заголовок) содержит тот же код: `MAX_ATLAS_MIRROR_BYTES = 128 МиБ`,
  `AtlasMirrorLog::{bytes,max_bytes,over_budget}`, `record()` с проверкой бюджета,
  `mirroring_failed()`, `RenderBackend::atlas_mirroring_failed`, `Arc<[u8]>` в `AtlasUpdate`,
  bail в `build_wire_frame` — всё присутствует в текущем `main`
  (`webgpu/mod.rs:248-346`, `502-530`; `host_process_backend.rs:648-657`; `draw.rs:481-487`).
  `git diff cbd71a9ff 32b92900a` показывает только последующий вынос кода в
  `wezterm-gpu-render` (`e6377d254`), не содержательные различия.
- После этого `main` ушёл дальше: `8cbb27818` (HashMap + `pending_set` вместо `BTreeMap` и
  `Vec::contains`), `2e227e77b` (ребёнок валидирует каждый `AtlasUpdate` и выходит при
  рассинхроне, `atlas_generation` вместо адреса `Rc`), `1730bdce9`/`596faf2c8`/`d8125f64d`
  (пулы буферов wire-пути — то, что в OOM-отчёте было главным подозреваемым по
  «never-reused ~300–435 KB allocations»).

Вердикт: **superseded**. Rebase'ить нечего; ветку можно удалить (после подтверждения
пользователем). Проблема роста памяти, которую она закрывала, на `main` закрыта тем же
способом плюс пулами; оставшиеся вопросы в этой области — не «память растёт», а
(а) тихая демоция при превышении 128 МиБ (§3.6) и (б) отсутствие логирования размера
`written`/`pending`, о котором просил OOM-отчёт (пункт 1 его «следующих шагов»).

## 5. Что нужно, чтобы подтвердить или закрыть A/B/C/D

1. `draw.rs:271-290`: `log::info!` (rate-limit 1/с) при отбросе кадра по `in_flight` и отдельно
   при срабатывании fallback-инвалидации; `host_process_backend.rs:553-555`: `log::info!`,
   когда `repaint_pending` оказался выставлен. Сопоставление по времени с `activate_tab:` /
   `focus_changed:` даст прямой ответ, был ли отброс кадра в момент инцидента.
2. `pane.rs` после `get_lines`: если `current_viewport.is_none()` и
   `!(stable_top..stable_top+lines.len()).contains(&cursor.y)` либо `stable_top != viewport_top`
   — `log::info!` с обоими значениями и `pane.get_current_seqno()` до/после. Это ловит B в
   грубой форме на каждом кадре, а не только при сбросе штампа.
3. Родитель пишет в свой лог `Fatal`-коды ребёнка уже сейчас (`host_process_backend.rs:499-504`),
   но `submit_frame failed` живёт только в логе ребёнка — дублировать в ack-канал.
4. Для C/D — регрессионные тесты на `HostProcessBackend` по образцу существующих
   (`host_process_backend.rs:894-952`): «ребёнок не ack'нул кадр → супервизор пересобирает
   backend в пределах threshold»; «после respawn первый кадр отправляется даже при совпадении
   сигнатуры».

## 6. Уверенность

| Вывод | Уверенность |
|---|---|
| Retained-дефект `0c7fbab37` закрыт; retained-путь при tab switch жив (146/750 без сброса) | Высокая |
| `HostProcessBackend` handshake без SeqCst = регрессия `aaf1f8f58`, lost wakeup достижим | Высокая (код + история) |
| Lost wakeup сработал в конкретном инциденте пользователя | Низкая–средняя; runtime-подтверждения нет и не может быть без логирования §5.1 |
| Порванный snapshot (B) достижим и даёт ровно наблюдаемую картину на один кадр | Высокая |
| B становится устойчивым только через отброс построенного кадра (A/C/D) | Высокая |
| C: `Ok(Err)` у ребёнка замораживает окно навсегда | Высокая; в логах не встречалось |
| D: потерянный кадр после respawn не пересылается из-за сигнатуры | Высокая; в логах не встречалось |
| Busy-spin `WM_PAINT` при throttle | Высокая |
| Снапшоты процессов каждые 300 мс на панель | Высокая |
| `retained_rows`/`pane_state`/`semantic_zones` не чистятся | Высокая |
| `fix/hostprocess-oom` superseded `32b92900a` | Высокая |
| Watchdog-stall'ы — системная причина, не баг рендера | Средняя (только по синхронности между процессами) |
