# OPT-4: ускорение reflow при ресайзе по ширине — дизайн (вариант E)

Дата: 2026-10-01. Автор дизайна: @ox (Opus 5.5 xhigh), по запросу владельца продукта.
Статус: решение принято, реализация — по шагам ниже.

## Измерения (release, 3550 строк истории, машина нагружена, цифры ~×1.5 завышены)

| Сценарий | conpty=false | conpty=true |
|---|---|---|
| сужение 120→100 | 21.4 мс | 22.3 мс |
| расширение 100→120 | 2.2 мс | 1.6 мс |
| drag по 1 колонке 120→80→120, текст из бенча | 13.5 мс/шаг | 13.3 мс/шаг |
| то же, 30% длинных строк с 24-bit цветом на каждом слове | 43.5 мс/шаг | 44.1 мс/шаг |

Среднее ~14 мс в `perf_bench_resize` = смесь сужения (~22 мс) и расширения (~2 мс).

Где время (сужение): `Line::wrap_keeping` (`onlyterm-surface/src/line/line/editing.rs`) ~80%
(15.7–19.3 мс на 3550 логических строках): пересборка по одной ячейке (`CellRef` в `Vec`,
`set_cell_grapheme`, `Arc::make_mut`, `append_grapheme`, invalidate, seqno, клон
`CellAttributes` на каждую ячейку). Освобождение строк ~1.6 мс, `rewrap_lines` ~0.3 мс,
клон 3550 `Line` ~0.1 мс, ветки ConPTY незаметны. Расширение = `Line::append_line`, тоже по
одной ячейке. Нижняя граница для тех же кусков через `set_ascii_run`: 2.6 мс.

Опровергнуто: время не уходит в аллокации `VecDeque`, клонирование `Line`, логику ConPTY.

## Выбранный вариант: E — то же поведение, но без работы по одной ячейке

Отвергнуты: A (ленивый reflow — сдвиг стабильных индексов/выделения/поиска/semantic zones,
прыжки скроллбара), B (то же + гонки с потоком чтения pty), C (склейка resize в GUI —
меняет UX и итоговое состояние в ConPTY), D (логические строки — переписывание Screen).

### Рекомендация (в промпты агентов)

```
OPT-4 = вариант E: ускорить существующий reflow без изменения семантики. A/B/C/D не делать.
Менять только Line::wrap_keeping и Line::append_line (onlyterm-surface) + новые методы ClusteredLine
(wrap_bounds/copy_range/append_clustered); screen/resize.rs и вся ConPTY-логика — НЕ трогать.
Старые тела перенести дословно в #[doc(hidden)] pub wrap_keeping_reference / append_line_reference — это оракул.
Быстрый путь только для CellStorage::C при защитах (width 1..u16::MAX-2; кластеры: cell_width>0 и сумма==len;
для append: сумма len<=u16::MAX и нет байтов <0x20/0x7F в other.text); иначе — reference.
Wrap: один проход для границ (для чистого печатного ASCII без wide — арифметика, без прохода), куски
копировать срезом text + пересечением кластеров со слиянием равных attrs (один clone attrs на кластер), bitset
и last_cell_width — как в append_grapheme; bits = set_bidi_info + HAS_HYPERLINK; не последним кускам
set_last_cell_was_wrapped(true). Append: push_str + слияние кластеров (move при Arc::try_unwrap) + сдвиг wide-бит.
Результат обязан быть равен reference по Line: PartialEq (не только «видимо равен») — на случайных строках
и на полном состоянии Screen (обе ветки ConPTY). В term добавить #[cfg(test)] thread-local переключатель
на reference в rewrap_lines для дифференциального теста; в прод-сборке его нет.
Каждый шаг — зелёный; «зубы» тестов доказывать временной мутацией. Версии не поднимать.
Цель: resize width в perf_bench_resize ≤35% базы (≈≤5 мс из 14–15), height — без изменений.
```

### Семантика и инварианты
- Результат каждого `Screen::resize` побитово совпадает с текущим в любой момент (`Line: PartialEq`:
  seqno, bits, cells вкл. разбиение на кластеры и `FixedBitSet`), число строк, стабильные индексы,
  курсор, `output_rows`. Ничего не бывает «временно устаревшим».
- `screen/resize.rs` (`rewrap_lines`, `conpty_*`, расстановка `keep`) не меняется. Короткие
  перенесённые строки в ConPTY идут через `line.resize(old_cols)` в V-хранилище по старому пути.
- `wrap_keeping` быстрый путь: C-хранилище, `1 <= width < u16::MAX-1`, непротиворечивые кластеры.
  1) `end` = исключающий конец последней видимой ячейки с `str != " " || cell_index < keep`;
  нет такой — `vec![self]`. 2) границы кусков: ASCII-ярус (нет wide-бит, байты 0x20..=0x7E,
  `text.len()==len`) — фиксированные отрезки; иначе один проход `cl.iter()` с байтовым смещением,
  новый кусок при `cur_len + w > width`. 3) `ClusteredLine::copy_range(cells, bytes, wide) ->
  (ClusteredLine, has_link)`: text — копия среза; clusters — пересечение с диапазоном, соседние с
  равными attrs сливаются; bitset как в `append_grapheme` (`with_capacity(off+1)`, `grow(off+1)`+`set`);
  `last_cell_width` = 2 если wide на `b-2`, иначе 1. 4) `Line` собирается напрямую: seqno, bidi_info,
  `HAS_HYPERLINK`; не последним кускам `set_last_cell_was_wrapped(true, seqno)`.
- `append_line` быстрый путь: обе стороны C, суммарный `len <= u16::MAX`, в `other.text` нет байтов
  `<0x20`/`0x7F` (эталон «обезвреживает» их через `as_cell()`/`TeenyString`), кластеры `other`
  непротиворечивы; иначе `append_line_reference`. `ClusteredLine::append_clustered`: `push_str`,
  слияние кластеров с последним при равных attrs, перенос при `Arc::try_unwrap` иначе клон, сдвиг
  wide-бит на `base`, `last_cell_width` от `other`, `len += other.len`, пустой `other` — no-op.
  Хвост `append_line` (seqno, `invalidate_zones`, кэш) прежний.
- Пинить тестами: быстрый путь == эталон по `PartialEq` и наблюдаемому компаратору (`visible_cells`:
  index/str/width/attrs, `len`, `last_cell_was_wrapped`, `bidi_info`, `has_hyperlink`,
  `current_seqno`); полное состояние `Screen` после каждого resize и после дальнейшего вывода
  совпадает с эталоном в обоих режимах ConPTY; `conpty_reflow.rs` (`native_captures`,
  `random_resizes_match_native_model`) и `resize.rs` зелёные без правок; откат на эталон для
  V-хранилища, управляющих байтов, кластеров нулевой ширины, переполнения `u16`.

### Риски
- E остаётся O(history): при 50–100k строк длинного переносимого вывода шаг будет десятки мс.
  Лекарство — вариант C (debounce модели между `WM_ENTERSIZEMOVE` и `WM_EXITSIZEMOVE`), меняет
  UX — не делаем без решения владельца.
- Не измерено: стоимость paint после смены ширины (все видимые строки перешейпиваются из-за
  подъёма seqno — сохранено намеренно) и RPC `ResizePseudoConsole`. После E могут стать
  основной ценой drag — мерить в приложении (OPT-5).
- Кириллица: выигрыш ограничен `Graphemes` в `next_grapheme_at` (~5.5 мс проход).
- `append_line` используют также tab bar, текст выделения, `get_logical_lines` (поиск),
  `apply_hyperlink_rules`; безопасность обеспечивает оракул по `PartialEq`.
- Замеры шумные: критерии относительные (медиана 3 прогонов). Холодная release-сборка term
  (thin LTO) с `-j 4` ~14 мин, пересборка 3–4 мин.

## План шагов (каждый шаг оставляет workspace зелёным)

Общее: `unset CARGO_TARGET_DIR`, cargo с `-j 4`; проверки: `cargo test -p onlyterm-surface`,
`cargo test -p onlyterm-term`, `cargo clippy -p onlyterm-surface -p onlyterm-term --all-targets -- -D warnings`,
`cargo fmt --check`; edition 2018 (`panic!("...{}", v)`); бенч:
`cargo test -p onlyterm-term --release --lib perf_bench_resize -- --ignored --nocapture --test-threads=1`,
медиана 3 прогонов; «зубы» = временная мутация, названный тест падает, откат + `touch`.

1. **Только бенч** (`term/src/test/perf_bench.rs`): оставить строку `resize width … /step`; добавить
   раздельно narrow 120→100 и widen 100→120; drag по 1 колонке 120→80→120; тот же drag на смешанном
   содержимом (70% коротких строк, 30% длиной 150–300 с `\x1b[38;2;r;g;bm` на каждом слове);
   narrow/widen на кириллице; обе ветки conpty. Приёмка: зелёно, в отчёте базовые медианы.
2. **Эталоны + построчный дифференциальный тест** (поведение не меняется): в `editing.rs` перенести
   тела дословно в `wrap_keeping_reference` / `append_line_reference`, публичные делегируют. Новый
   `onlyterm-surface/src/line/tests/reflow_fastpath_test.rs` (регистрация в `line/line/mod.rs`).
   Генератор: детерминированный LCG (печатать seed при падении); графемы ASCII, `" "`, `"é"`,
   `"e\u{301}"`, `"ж"`, `"漢"`, `"👍"`, `"🇷🇺"`; attrs: default, bold, палитра, 24-bit fg/bg, цвет
   подчёркивания, гиперссылки A/B, semantic Prompt/Input, `wrapped=true`; операции:
   `set_cell_grapheme`, `set_ascii_run`, `set_last_cell_was_wrapped`, `prune_trailing_blanks`,
   `set_bidi_info`, `compress_for_scrollback`, изредка `"\x07"` и ширина 0, часть входов в V. width
   1..=len+3, keep ∈ {0, случайное, len, len+5}; вход строить дважды из seed плюс случай с общим
   клоном; ≥20k случаев + точечные (wide на границе, width=1 с wide, одни пробелы, цветные хвостовые
   пробелы, гиперссылка только в хвосте). Зубы: в `wrap_keeping` делегировать с `width+1`.
3. **Хук и дифференциальный тест уровня Screen** (поведение не меняется): в `term/src/screen/resize.rs`
   помощники `split_row`/`join_rows`; под `#[cfg(test)]` thread-local `REFERENCE_REFLOW` и
   `with_reference_reflow` с drop-guard. Новый `term/src/test/screen/reflow_differential.rs`.
   Seeds 0..300 (нечётные — conpty); scrollback ∈ {0,5,50,400}; cols 10..140, rows 3..40; вывод: слова
   (ASCII/кириллица/CJK/эмодзи/комбинирующие), SGR 16/256/24-bit + `58`, OSC 8, OSC 133 A–D, изредка
   kitty `TINY_PNG_BASE64` из `test/image.rs`, CR/LF/CRLF, длинные строки 2–5 ширин, цветные пробелы,
   EL 0/1/2, ED 0/1, CUP, DECAWM off/on, изредка 1049 h/l; 12 resize (на 1, большие прыжки, cols до 1).
   Два терминала с одинаковыми байтами, эталонный ресайзится внутри `with_reference_reflow`. Снимки
   после каждого resize и доп. вывода: `all_lines()` по `==`, alt-экран, `scrollback_rows()`,
   `phys_to_stable_row_index(0)`, размеры, `cursor_pos()`, `get_semantic_zones()`,
   `get_changed_stable_rows`. Debug <15 с. Зубы: в не-эталонной ветке `join_rows` передавать `seqno+1`.
4. **Быстрый `wrap_keeping`** (`clusterline.rs`: `wrap_bounds`, `copy_range`, проверка кластеров;
   `editing.rs`: диспетчеризация). Тесты шагов 2–3 без правок + точечный «на C-входе берётся быстрый
   путь». Зубы (каждая валит хотя бы один тест): `>=` вместо `>` в начале куска; нет `HAS_HYPERLINK`;
   нет слияния равных attrs (ловит только `PartialEq`); `last_cell_width` всегда 1; ASCII-ярус
   игнорирует `keep`. Приёмка к базе шага 1: narrow ≤35% (обе ветки conpty); drag со смешанным
   24-bit ≤50%; narrow на кириллице ≤60%; height без изменений.
5. **Быстрый `append_line`** (`clusterline.rs`: `append_clustered`, перенос при `Arc::try_unwrap`;
   `editing.rs`). В генераторе оба варианта `other` (уникальный/разделяемый). Зубы: всегда добавлять
   кластер вместо слияния; wide-биты без сдвига на `base`; убрать проверку управляющих байтов (ловит
   точечный тест с `"\x07"`); не обновлять `last_cell_width`. Приёмка: widen ≤70%; итоговый resize
   width ≤35% (обе ветки conpty); drag со смешанным 24-bit ≤35%.
6. **Документация и финальные замеры:** запись в `docs/changelog.md` без поднятия версии, обновить
   doc-комментарий `perf_bench.rs`, итоговая таблица база → после (медианы).
7. **Только если не достигнута цель шага 5 для смешанного 24-bit:** в `wrap_keeping` переносить кластеры
   исходника при уникальном `Arc` вместо клонирования; тот же оракул по `PartialEq`.

## Базовые замеры (шаг 1, медиана 3 прогонов, release, 3500 строк истории, нагруженная машина)

| Сценарий | conpty=false | conpty=true |
|---|---|---|
| resize width (старая строка бенча) | 10.2 мс/шаг | 15.1 мс/шаг |
| resize height | 5.1 мкс/шаг | 11.3 мкс/шаг |
| narrow 120→100 ascii | 24.7 мс | 22.8 мс |
| widen 100→120 ascii | 2.4 мс | 2.1 мс |
| narrow 120→100 кириллица | 26.6 мс | 30.9 мс |
| widen 100→120 кириллица | 3.8 мс | 4.9 мс |
| drag 1 колонка ascii | 13.8 мс/шаг | 13.3 мс/шаг |
| drag 1 колонка mixed 24-bit | 43.7 мс/шаг | 43.2 мс/шаг |

Критерии приёмки шагов 4–5 считаются от этих чисел (на той же машине, тем же бинарём бенча).

## Итоги шага 5 (фаза D: быстрый `append_line`, 2026-10-02)

Медианы 3 прогонов release (`perf_bench_resize`, `-j 4`), тот же бенч, что в базовой таблице:

| Сценарий | conpty=false | conpty=true | база (f/t) | после фазы C |
|---|---|---|---|---|
| resize width | 2.9 мс/шаг | 3.2 мс/шаг | 10.2 / 15.1 | — |
| resize height | 5.2 мкс | 7.4 мкс | 5.1 / 11.3 | без изменений ✓ |
| narrow 120→100 ascii | 4.2 мс | 4.8 мс | 24.7 / 22.8 | ~3.7 |
| widen 100→120 ascii | 2.25 мс | 2.61 мс | 2.4 / 2.1 | ~1.8 |
| narrow 120→100 кириллица | 19.4 мс | 22.1 мс | 26.6 / 30.9 | ~18.6 |
| widen 100→120 кириллица | 2.35 мс | 2.40 мс | 3.8 / 4.9 | ~4.4 |
| drag 1 колонка ascii | 2.6 мс/шаг | 2.6 мс/шаг | 13.8 / 13.3 | — |
| drag 1 колонка mixed 24-bit | 6.7 мс/шаг | 6.7 мс/шаг | 43.7 / 43.2 | ~18.3 |

Приёмка шага 5: resize width ≤35% базы ✓ (29%/21%); drag mixed ≤35% ✓ (≈15–16%, шаг 7 не
понадобился); height без изменений ✓; widen ascii ≤70% базы — по медианам НЕ достигнуто
(94%/124%), лучшие прогоны 1.44/1.50 мс (60%/71%). Машина нагружена (кластеры прогонов
3.5–7.4 мс на узких сценариях в пределах одной серии); абсолютные числа widen сопоставимы
с после-фазой-C (~1.8 мс): расширение теперь, судя по всему, лимитируется не поячеечным
append, а фиксированной остаточной стоимостью (подъём seqno/кэш/invalidate на строку и
уровень Screen). Кириллический widen улучшился относительно фазы C за счёт дешёвого яруса
гейтера (сканы сегментации пропускаются, когда ни один байт `other.text` не может начать
нестартовый графемный символ).

Фаза D: `ClusteredLine::append_clustered` / `append_clustered_owned` (`Arc::try_unwrap` —
перенос кластеров, иначе клон attrs), сдвиг wide-бит теми же `grow`+`set`, что в эталоне;
гейт: обе стороны C + `clusters_consistent`, суммарный `len <= u16::MAX`, нет управляющих
байтов (<0x20/0x7F) в `other.text`, плюс проверка согласованности сегментации текста
(слипание графем внутри `other` или на стыке — откат на эталон; для «чистых» скриптов —
дешёвый тест по ведущим байтам UTF-8).

### Проверка шага 5 (A/B фаза C ↔ фаза D, чередующиеся прогоны, медианы, мс)

| Сценарий | C | D | D/C |
|---|---|---|---|
| drag mixed 24-bit (conpty off / on) | 19.6 / 19.9 | 6.8 / 7.1 | 0.35 / 0.36 |
| widen кириллица (off / on) | 4.5 / 5.3 | 2.5 / 2.5 | 0.55 / 0.47 |
| widen ascii (off / on) | 2.0 / 1.8 | 1.7 / 1.8 | 0.85 / 0.97 |
| narrow ascii (off / on) | 4.4 / 4.1 | 4.1 / 3.9 | 0.93 / 0.96 |
| drag ascii (off / on) | 3.5 / 3.4 | 2.7 / 2.9 | 0.76 / 0.83 |
| resize height | без изменений | | |

Гейт согласованности сегментации (`append_fast_path_safe`): дешёвый ярус держится на множестве
ведущих байтов UTF-8 `{CC, CD, D2, D5..F4}`. Тест `append_line_fusing_graphemes_match_reference`
содержит по случаю на каждый ведущий байт множества; мутация «убрать байт» ловится для каждого
из `CC CD D2 D6 D7 D8 D9 E0 E1 E2 E3 EF F0 F3`. Байт `D5` в множестве лишний (в блоке U+0540–057F нет
символов Extend/SpacingMark/Prepend), его удаление эквивалентная мутация; он оставлен как запас.
Порог проверки стыка (`lead < 0xD8`) и `boundary_merges_graphemes` тоже избыточны для равенства
результата с эталоном (хранимые текст/кластеры/len при склейке совпадают независимо от
пересегментации), оставлены как консервативная защита.
