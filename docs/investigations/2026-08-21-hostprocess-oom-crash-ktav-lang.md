# Крах PID 67832 (`ktav-lang`): allocation failure / Rust OOM-abort в родительском процессе

Дата: 2026-08-21
Сборка: `v0.0.14-alpha-14-g58097a015`, commit #603 `58097a01` (built 2026-08-21 13:58 UTC) —
это ровно тот коммит, которым закончился сегодняшний пуш (`58097a015`, «gui: always-on
trace logging for the ghost-cursor investigation»), установленный пользователем вручную
поверх `C:\Program Files\OnlyTerm\`. Краш — в коде, отгруженном этой же сессией.

## Симптом

Пользователь сообщил о крахе окна (`D:\dev\ktav-lang`, конфиг `ktav-lang.ktav`) практически
сразу после его открытия.

## Ложный след в начале расследования

Первая попытка сопоставить PID была ошибочной: `0x108f8` (hex из WER) был неверно
переведён в 67704 вместо правильных **67832** (`printf "%d" 0x108f8` → 67832). PID 67704
принадлежал совсем другому, гораздо более раннему процессу (стартовал 10:58, лог которого
обрывается в 11:01 без единой ошибки — скорее всего просто закрыт пользователем в
обычном порядке, не крах). Урок: hex→dec на глаз ненадёжен, `0x108f8` ≠ 67704 —
пересчитывать явно, не прикидывать.

## Источники

- `onlyterm-gui.exe-log-67832.txt` (`C:\Users\Computer\.local\share\onlyterm\`) — стартовал
  16:28:42.489, последняя строка 16:30:02.529.
- Windows Event Log, `Application`, provider `Application Error` (Id 1000) и
  `Windows Error Reporting` (Id 1001), TimeCreated 16:31:17–16:31:20.
- Дамп `C:\CrashDumps\OnlyTerm\onlyterm-gui.exe.67832.dmp` (5.3 ГБ, полный, сконфигурирован
  через `HKLM\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\onlyterm-gui.exe`,
  `DumpType=2`). WER Temp/ReportArchive копий дампа не сохранили — только `Report.wer`
  (метаданные); реальный `.dmp` нашёлся только в LocalDumps.
- Символы: `C:\Program Files\OnlyTerm\onlyterm_gui.pdb`, время записи 16:07 — совпадает с
  exe (та же сборка), 20 минут до краха.
- Отладчик: `cdb` из пакета WinDbg (Microsoft Store,
  `...\WindowsApps\Microsoft.WinDbg_8wekyb3d8bbwe\cdbX64.exe`) — рабочий консольный
  отладчик из более ранней части сессии (задача #629) в PATH не висит, найден напрямую по
  известному расположению WindowsApps.

## WER: проблемный сигнатурный блок

```
P1: onlyterm-gui.exe          P2: 0.0.14.594
P3: 6a885baf (timestamp)      P7: 0000000000aeae41 (fault offset)
P8: c0000409                  P9: 0000000000000007
Faulting process id: 0x108f8  (= 67832)
Process Uptime: 0 days 0:02:38.000
```

`0xc0000409` = `STATUS_STACK_BUFFER_OVERRUN` / «Security check failure or stack buffer
overrun», subcode `0x7 FAST_FAIL_FATAL_APP_EXIT`. На вид похоже на переполнение стека —
но это ровно тот код, которым `std::process::abort()`/`__fastfail` сигнализирует о
принудительном завершении на Windows. Не читать буквально как «buffer overrun» без
проверки стека — что и подтвердилось.

## Разбор дампа (`cdb -z ... -y "C:\Program Files\OnlyTerm"`, `.ecxr; kv 50`)

Падающий (единственный, `.ecxr`-контекст) кадр:

```
onlyterm_gui!std::alloc::rust_oom::closure$0+0x31              alloc.rs @ 429
onlyterm_gui!std::alloc::rust_oom+0x1b                          alloc.rs @ 424
onlyterm_gui!std::alloc::_::__rust_alloc_error_handler+0x18     alloc.rs @ 423
onlyterm_gui!alloc::alloc::handle_alloc_error+0x18               alloc.rs @ 557
onlyterm_gui!alloc::raw_vec::handle_error+0x1b                   raw_vec\mod.rs @ 890
onlyterm_gui!onlyterm_gui::termwindow::TermWindow::paint_impl+0x4b81
    D:\dev\rust\onlyterm\crates\onlyterm-gui\src\termwindow\render\paint.rs @ 143
  (inline) do_paint_webgpu_impl+0x8      render_pipeline.rs @ 1419
onlyterm_gui!...::do_paint_webgpu+0x63
onlyterm_gui!...::dispatch_window_event+0x2103
onlyterm_gui!...::render_pipeline::impl$0::new_window::async_fn$0::closure$6+0x7a
onlyterm_gui!window::WindowEventSender::dispatch+0x13f
onlyterm_gui!window::os::windows::window::wm_paint+0x144
onlyterm_gui!window::os::windows::window::do_wnd_proc+0xd99
onlyterm_gui!window::os::windows::window::wnd_proc+0x2d
user32!CallWindowProcW → DispatchMessageW → главный message loop
onlyterm_gui!onlyterm_gui::run_terminal_gui → run → main
```

Однозначно: **не GS/stack corruption, не GPU-драйвер** — настоящая причина — `Vec`/`RawVec`
не смог аллоцировать память (`handle_alloc_error`), Rust вызвал свой alloc-error handler,
тот — `abort()`. Всё происходит на **главном GUI-потоке**, синхронно внутри обработки
`WM_PAINT`, внутри `paint_impl` → `self.call_draw(frame)` (paint.rs:143 — сама строка не
аллоцирует, это внешняя граница инлайна в release-сборке; реальная аллокация — где-то
внутри `call_draw`'s инлайнированного тела).

Кандидат на размер провалившейся аллокации — значение `0x6105c` (397 404 байта, ~388 КБ),
повторяющееся как аргумент через кадры `rust_oom`→`handle_alloc_error`. Сама по себе эта
аллокация небольшая — типичная картина «упала не потому что запрошено много, а потому что
адресное пространство/память уже почти исчерпаны чем-то другим».

`!address -summary` не сработал офлайн (нет паблик-символов `ntdll` без доступа к
Microsoft symbol server) — точный итоговый объём committed-памяти процесса на момент краха
не получен.

## Контекст по логу перед крахом

```
16:28:42.700  HostProcessBackend: generation 0 running as PID 62772
16:28:43.788  paint_pane: retained rows reset ... cursor.y=2   viewport_top=0
16:28:47.271  paint_pane: retained rows reset ... cursor.y=916 viewport_top=867
16:28:47.285/93/337/386  … ещё 4 таких же сброса за 115 мс, тот же cursor.y=916
16:29:13 – 16:30:02       обычная активность (клавиши, смены фокуса)
16:30:02.529              последняя строка лога
16:31:17–20 (WER)         фактический abort — то есть ~75 c без единой строки в логе
```

За первые ~4 секунды жизни курсор перескочил с row 2 на row 916 — то есть в окно очень
быстро влетел большой объём вывода (900+ строк). Дальше — обычное использование, потом
тишина ~75 c, потом abort. Процесс прожил всего 2 мин 38 с суммарно.

## Рабочая гипотеза (INFERRED, не подтверждена окончательно)

Это окно рендерилось через `HostProcessBackend` (см. лог), то есть шло по wire-frame пути
с зеркалированием атласа глифов — код, который сегодня стал безусловным дефолтом для
каждого окна.

`AtlasMirrorLog.written` (`crates/onlyterm-gui/src/termwindow/webgpu/mod.rs:273-276`,
`BTreeMap<AtlasRect, Vec<u8>>`) хранит **полную CPU-копию пикселей каждого когда-либо
записанного в атлас прямоугольника** — то есть удваивает память атласа (GPU-текстура +
идентичная CPU-копия), пока жива эта генерация мирроринга. Согласно doc-комментарию
(`mod.rs:264-271`) рост «ограничен occupancy атласа», а полный сброс происходит только
при regrow атласа («regrow builds a whole new texture with its own fresh log»). Сама
логика записи (`record`, `mod.rs:279-284`) действительно только ЗАМЕНЯЕТ запись по тому же
rect-ключу, а не копит дубликаты для одного и того же rect.

Однако: `glyphcache/mod.rs:185` подтверждает, что атлас **не делает точечного eviction** —
«eviction is managed by recreating Self when the Atlas is filled», то есть в рамках ОДНОЙ
генерации атлас только растёт (новые уникальные глифы = новые rect'ы), полный сброс — лишь
при заполнении и regrow. При всплеске из сотен строк разнообразно оформленного вывода
(похоже на цветной вывод компилятора/линтера — характерно для `ktav-lang`) атлас должен
был быстро расти новыми уникальными (глиф, цвет, стиль) комбинациями, и `written` растёт
вместе с ним 1:1, добавляя ПОЛНОЕ дублирование памяти атласа на CPU — нагрузка, которую до
сегодняшнего дня не платило ни одно окно (mirroring существовал только для
host-process-пути, который был опциональным экспериментом; теперь это дефолт для всех).

`needs_full_resync`/`frame_form()` (`host_process_backend.rs:114-115,255,361,611-613`)
сама по себе выглядит корректно (`AtomicBool::swap(false, ...)` — читает и гасит атомарно,
не может залипнуть в `true` навсегда) — это не проверенная причина.

**Не проверено и не доказано:**
- итоговый объём committed-памяти процесса на момент краха (не удалось получить офлайн);
- действительно ли именно `AtlasMirrorLog`/атлас были источником конкретной провалившейся
  аллокации на 397 КБ, а не что-то ещё в `call_draw`'s инлайнированном теле;
- сколько именно regrow'ов атласа произошло за 2 мин 38 с и сколько памяти каждый занял.

## Почему это важно

Сегодняшняя работа (`HostProcessBackend`) существует специально для того, чтобы GPU-крах
дочернего процесса не ронял всё окно. Respawn-логика лечит смерть *дочернего* процесса.
OOM в *родительском* процессе (главный GUI-поток, `paint_impl`) она никак не покрывает —
родитель просто абортит целиком, ровно то самое «упало всё окно», от которого вся
конструкция должна была защищать.

## Следующие шаги (не выполнены, решение за пользователем)

1. Точечное логирование размера `AtlasMirrorLog.written`/`pending` (сумма байт, число
   rect'ов) — по аналогии с Phase B ghost-cursor-логированием, чтобы при следующем
   похожем крахе в логе уже была цифра, а не только косвенные признаки.
2. Рассмотреть ограничение/бюджет для `written` (LRU-эвикция по объёму, а не только по
   regrow атласа), или отказ от полного CPU-дублирования пикселей в пользу
   перестраиваемого на лету full-resync без постоянного хранения.
3. Дождаться отдельного вердикта, прежде чем чинить — механизм пока INFERRED, не
   подтверждён измерением реальной памяти на моменте краха.
