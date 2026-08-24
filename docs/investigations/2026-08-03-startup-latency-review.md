# Ревью расследования startup latency OnlyTerm

Дата: 2026-08-03.

Исходное расследование: `2026-08-03-startup-latency.md`.

## Вывод

Наиболее вероятная причина задержки — не `CreateProcessW`, Job Object или
стоимость создания PTY, а трёхсекундный startup-handshake новой bundled ConPTY
и несвоевременный либо нераспознанный ответ OnlyTerm на запросы DSR/DA1.

Уверенность высокая, но для окончательного доказательства нужен один
инструментированный прогон с трассировкой запросов и ответов.

## Почему это главный кандидат

OnlyTerm вызывает `CreatePseudoConsole` с флагом
`PSEUDOCONSOLE_INHERIT_CURSOR`:

```rust
PSEUDOCONSOLE_INHERIT_CURSOR
    | PSEUDOCONSOLE_RESIZE_QUIRK
    | PSEUDOCONSOLE_WIN32_INPUT_MODE
```

Файл: `crates/pty/src/win/pseudocon.rs`, вызов около строки 157.

В установленном OnlyTerm `20240203-110809-5046fc22`, который использовался как
быстрый эталон, этого флага ещё не было. Он появился в upstream-коммитах:

- `454ec0a7d` — `windows: set PSEUDOCONSOLE_INHERIT_CURSOR`;
- `9d8345863` — добавление именованной константы флага.

Microsoft документирует, что при `PSEUDOCONSOLE_INHERIT_CURSOR` вызывающее
приложение обязано асинхронно прочитать запрос состояния курсора из `hOutput` и
ответить через `hInput`. В противном случае последующие операции с
псевдоконсолью могут зависнуть:

https://learn.microsoft.com/en-us/windows/console/createpseudoconsole

Текущая пара OnlyTerm имеет версию `1.22.2502.04002` / package version
`1.22.250204002`:

| файл | размер | SHA-256 |
|---|---:|---|
| `conpty.dll` | 109104 | `375BFB0479B6C53836AB307E3F9FD17BEDBD733F2E9690943D0F12E72FB80777` |
| `OpenConsole.exe` | 1153064 | `55B18996761C88C351820E82508E05AB0EC2194AEEAD20724FDFAEEDEC076EF4` |

Хеши совпадают в трёх местах:

- `assets/windows/conhost`;
- `D:/dev/rust/.cargo-target/release`;
- `C:/Program Files/OnlyTerm`.

Эта пара была внесена в upstream WezTerm коммитом `4accc376f` от 2025-02-08.

В соответствующем коде Microsoft ConPTY при старте:

1. При cursor inheritance отправляет `CSI 6 n` — запрос позиции курсора.
2. Отправляет `CSI c` — запрос Primary Device Attributes (DA1).
3. Вызывает `WaitUntilDA1(3000)`.

Источник:

https://github.com/microsoft/terminal/blob/b25fe55e94197be667625acef00506ef1636d137/src/host/VtIo.cpp#L163-L198

Разработчик ConPTY также прямо подтверждает трёхсекундный timeout cursor
inheritance:

https://github.com/microsoft/terminal/issues/17688

Это практически точно совпадает с замером расследования: дочерний процесс
завершается через **3.0–3.9 секунды после возврата `CreateProcessW`**.

## Важное уточнение про флаг

В версии ConPTY, соответствующей bundled-паре 1.22, ожидание DA1 выполняется
даже без `PSEUDOCONSOLE_INHERIT_CURSOR`. Флаг добавляет запрос позиции курсора,
но DA1-запрос и `WaitUntilDA1(3000)` остаются.

Поэтому удаление только `PSEUDOCONSOLE_INHERIT_CURSOR` — полезный A/B-тест, но
оно не обязано устранить задержку. Если задержка останется, проверять нужно
именно доставку и распознавание DA1-ответа.

## Где должен формироваться ответ

OnlyTerm умеет отвечать на оба запроса:

- DA1 (`RequestPrimaryDeviceAttributes`) —
  `crates/term/src/terminalstate/mod.rs`, около строки 1381;
- CPR/DSR (`RequestActivePositionReport`) — тот же файл, около строки 2640.

DA1-ответ имеет вид:

```text
ESC [ ? 65 ; 4 ; 6 ; 18 ; 22 ; 52 c
```

Он должен удовлетворять парсер ConPTY: conformance level `65` больше требуемого
`61`, а список атрибутов непустой.

При этом текущий порядок создания панели следующий:

1. `openpty()`;
2. `pair.slave.spawn_command(cmd)`;
3. создание `WriterWrapper` и `Terminal`;
4. создание `LocalPane`;
5. `mux.add_pane()`;
6. только внутри `add_pane()` запускается поток чтения PTY.

Основной путь находится в `crates/mux/src/domain.rs`, начиная примерно со
строки 678; `spawn_command` вызывается около строки 724.

Следовательно, оставшийся неизвестный участок уже достаточно узок:

```text
ConPTY посылает DSR/DA1
        ↓
поток read_pty читает запрос
        ↓
парсер распознаёт RequestActivePositionReport / RequestPrimaryDeviceAttributes
        ↓
Terminal формирует CPR/DA1
        ↓
WriterWrapper передаёт ответ потоку pane-writer
        ↓
реальный WriteFile пишет ответ в ConPTY input
        ↓
ConPTY принимает ответ и снимает WaitUntilDA1
```

## Что в исходном расследовании стоит скорректировать

Гипотезу «виновата версия ConPTY» пока нельзя считать опровергнутой.

Подмена старой пары `conpty.dll`/`OpenConsole.exe`, после которой оболочка не
запускается вообще, показывает несовместимость старой пары с текущим клиентом,
но не доказывает, что новая пара не создаёт трёхсекундную задержку.

Кроме того, сравнение текущего OnlyTerm с установленным OnlyTerm от 2024 года
одновременно меняет:

- код клиента ConPTY;
- флаги `CreatePseudoConsole`;
- bundled-версию ConPTY;
- обработку ввода/вывода и writer path.

Поэтому вывод «разница наша, а не свойство машины» верен, но пока нельзя
утверждать, что это именно OnlyTerm-специфичная правка. Проблема может быть
регрессией или несовместимостью более нового upstream WezTerm/ConPTY.

В списке следующего шага также необходимо явно сравнить **флаги
`CreatePseudoConsole`**, а не только флаги `CreateProcessW` и размер
псевдоконсоли.

## Почему остальные кандидаты слабее

- `CreatePseudoConsole`, `CreateProcessW` и весь `spawn_command` укладываются в
  десятки миллисекунд.
- Job Object создаётся и назначается внутри измеренного `spawn_command`; в этом
  коде нет трёхсекундного ожидания.
- Флаги `CreateProcessW`, environment block и cwd сами по себе плохо объясняют
  стабильный интервал, совпадающий с известным `WaitUntilDA1(3000)`.
- Рендерер запускается в другом участке жизненного цикла и не объясняет
  задержку выполнения уже созданного дочернего `cmd.exe`.

## Следующий диагностический прогон

Нужно добавить временные timestamps в четыре точки:

1. После каждого первого чтения из ConPTY — вместе с hex-представлением сырых
   байтов. Ожидаются как минимум `ESC[6n` и `ESC[c`.
2. При входе в обработчики `RequestActivePositionReport` и
   `RequestPrimaryDeviceAttributes`.
3. При постановке CPR/DA1-ответа в очередь `WriterWrapper`.
4. Непосредственно до и после настоящего `writer.write_all` в потоке
   `pane-writer`.

Интерпретация результата:

| наблюдение | вывод |
|---|---|
| запросы появляются только после трёх секунд | проблема в startup ordering или выдаче ConPTY output |
| запросы прочитаны, но обработчики не вызываются | проблема в parser path |
| обработчики вызваны, но реальной записи нет | проблема в `WriterWrapper`/writer thread |
| корректный DA1 реально записан сразу, но ConPTY ждёт 3 секунды | несовместимость протокола или конкретной ConPTY 1.22 |

Дополнительные A/B-тесты:

- убрать только `PSEUDOCONSOLE_INHERIT_CURSOR`;
- сравнить writer path до и после коммита `41522c8ae`;
- проверить современный upstream WezTerm с той же самой парой ConPTY
  `1.22.250204002`, а не релиз 2024 года;
- проверить промежуточную bundled-пару ConPTY до Microsoft Terminal PR
  `#17510`, сохранив остальной код текущим.

## Итоговая рабочая гипотеза

Дочерний процесс создан быстро, но новый `OpenConsole` удерживает его console
startup в `WaitUntilDA1(3000)`. OnlyTerm либо не отвечает на стартовый DSR/DA1
вовремя, либо ответ не доходит/не распознаётся. По истечении трёх секунд ConPTY
продолжает запуск, после чего `cmd.exe` выполняет команду и появляется маркер.

Это объясняет одновременно:

- белую пустую панель;
- быстрый возврат `CreateProcessW`;
- дешёвый PTY path;
- задержку именно внутри уже созданного дочернего процесса;
- стабильную величину около трёх секунд;
- отсутствие задержки у установленного OnlyTerm с парой ConPTY 2024 года.
