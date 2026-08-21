# План решения: фантомный курсор (задача #658)

Дата: 2026-08-21
Основание: три investigation-отчёта этой даты
(`ghost-cursor-after-refocus-tab-switch.md`, `ghost-cursor-retained-stamp-refutation.md`,
`ghost-cursor-consolidated-analysis.md` — итоговый). Состояние на входе:

- Подтверждён статикой дефект cursor-bearing retained row (с `31595257a`, task #457) —
  чинится независимо от исходного инцидента.
- Причина конкретного пользовательского кадра (refocus → tab switch) НЕ доказана;
  главный кандидат — несогласованный render snapshot (три раздельных `terminal.lock()`),
  для доказательства нужен runtime trace.

Поэтому план — три независимые фазы: A чинит доказанное, B инструментирует недоказанное,
C (условная) чинит причину исходного кадра после её подтверждения.

## Phase A — cursor-aware retained fix (доказанный дефект)

Цель: retained-строка, в quad-ах которой запечён курсор, не имеет права пере-эмититься
после ухода курсора с этой строки.

Правки (узкие, три файла):

1. `crates/wezterm-gui/src/termwindow/render/mod.rs` — `RetainedRow`: добавить поле
   `contains_cursor: bool`.
2. `crates/wezterm-gui/src/termwindow/render/pane.rs` — обе ветки, записывающие
   retained row (`EmitCached` и `Build`), заполняют флаг. Консервативно:
   `cursor.y == stable_row` на момент построения quad-ов (ложный `true` стоит одного
   лишнего row build; ложный `false` сохраняет фантом — недопустим). Заодно: при
   чтении `has_retained` (`pane.rs:586-592`) доставать и флаг.
3. `crates/wezterm-gui/src/termwindow/render/budget.rs` — `RowSweep::decide`: новый
   параметр `retained_contains_cursor: bool`; условие `must_build` расширяется:
   cache-miss строка с retained-курсором строится всегда (в обход бюджета и
   `work_start`), даже если это больше не текущая cursor row.

Regression-тесты (все — в `budget.rs`, чистые юниты; список из отчёта 3 §7):

1. Слот A: retained с курсором, текущая cursor row = B, cache miss, дедлайн истёк →
   A обязан получить `Build`.
2. То же при `A < work_start` и НЕистёкшем дедлайне → `Build` (закрывает ветку
   «EmitRetained без превышения бюджета»).
3. Обычная retained-строка без курсора при истёкшем дедлайне по-прежнему получает
   `EmitRetained` (не выключить оптимизацию task #457).
4. Текущая cursor row никогда не откладывается (существующий инвариант, расширить).
5. Rotation сохраняет forward progress с новым must-build условием
   (адаптировать `rotation_starvation_does_not_happen`).
6. Инвариант сброса: после stamp mismatch ни один слот не может получить
   `EmitRetained` (юнит на связку `has_retained=false` → `Build`; уже частично
   покрыт, зафиксировать явно).

Верификация: `cargo build -p wezterm-gui`, `cargo clippy -p wezterm-gui --all-targets
-- -D warnings`, `cargo fmt --check`, `cargo test -p wezterm-gui`.

Не-цели Phase A: не трогать `quad_generation` (сужение — отдельная оптимизация, не фикс),
не чинить snapshot race, не менять `RetainedStamp`.

## Phase B — постоянное trace-логирование для исходного кадра

Баг редкий, форсировать нельзя — логирование должно быть всегда включено (уровень
`info`, попадает в per-PID лог без настройки) и срабатывать только на редких событиях,
не на каждом кадре:

1. `focus_changed` (`window_handler.rs:29`): направление, старое→новое
   `quad_generation`.
2. `paint_pane` при сбросе retained по stamp mismatch (`pane.rs:446-455`): `pane_id`,
   старое/новое поколение штампа, `viewport_rows`.
3. В том же событии сброса — снимок `cursor.y`, `viewport_top`, `dims.physical_top`
   (позволит потом сверить согласованность снимков; логируется ТОЛЬКО при сбросе,
   не каждый кадр).
4. Переключение вкладки (`set_active_idx`/эквивалент в `actions.rs`): `tab_id`,
   текущее `quad_generation` — чтобы восстановить порядок «FocusChanged vs tab switch»
   из лога постфактум.

Критерий достаточности: по одному только логу восстановимо, какая из двух веток
отчёта 3 реализовалась (stamp match + EmitRetained ИЛИ stamp reset + рассинхрон
снимков).

После сборки — установить в систему (замена `C:\Program Files\OnlyTerm\onlyterm-gui.exe`)
**только по отдельному явному подтверждению пользователя** — это боевой бинарник.

## Phase C — условная: единый render snapshot (кандидат-причина исходного кадра)

НЕ начинать до подтверждения из Phase B-лога. Если подтвердится рассинхрон
cursor/dimensions/lines: один кратковременный `terminal.lock()` в начале `paint_pane`,
под ним — cursor + dimensions + клон строк, затем lock отпускается до shaping/GPU
(сохраняет существующую цель не держать lock во время тяжёлого рендера). Затрагивает
`Pane`-трейт и overlay-реализации — потому и отложено до доказательства.

## Порядок

A → B — сразу, последовательно (A самодостаточна). C — только по результату B.
TaskList: #658 остаётся зонтиком расследования; фазы A и B заводятся отдельными
задачами при старте работ.
