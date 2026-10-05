# Приёмка correctness/performance followups — 2026-10-04

База: `9bc9bf4d5`; XL работали в изолированных worktree. Все сборки, smoke, mutation и A/B выполнила основная сессия. Версии, коммиты и push не изменялись.

## Решения

| Задача | Решение |
| --- | --- |
| CF-1 | Безопасный extreme-width fallback; debug/release regression и runtime. |
| CF-2 | Восемь feature-комбинаций vtparse, no-alloc runtime и три убитые mutation. |
| CF-3 | 69 parser-тестов со std, три strip-теста, CLI smoke и пять убитых mutation. |
| PF-1 | Opt-in pipeline profiler и isolated native harness; десять сценариев. |
| PF-2 | Colored-eviction prototype отклонён: полного lifecycle выигрыша нет. |
| PF-3 | Vec и checkpoint prototypes не приняты: шумные controls не позволяют зачесть ускорение. Production algorithm оставлен прежним. |
| PF-4 | Принят soft 64-KiB print budget; borrowed fragments, grapheme/NFC границы, contiguous print coalescing и resize_guard. 16 KiB отклонён. |
| PF-5 | Принят move owned scratch cells вместо второй clone. |
| PF-6 | Validation-window scanner отклонён; production scanner не изменён. |

Дополнительно исправлен stale wrap-cache в `resize_and_clear` при том же seqno. Независимая регрессия падала до исправления; после исправления равенство scroll checkpoints восстановлено. Удалены incidental fast-path-hit assertions и их счётчики; semantic/reference проверки сохранены.

## PF-1: final native profile

Windows x64, dev-install binaries, fresh own GUI per scenario, --skip-config, Info без trace. Данные cumulative и включают startup/warmup. Значения — HDR-quantized ms.

| Сценарий | Paint samples | Paint p50 | p95 | p99 | max | Apply p99 | Apply max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| ascii | 6 | 1.884 | 4.719 | 4.719 | 4.719 | 0.406 | 0.406 |
| cyrillic | 7 | 3.178 | 10.748 | 10.748 | 10.748 | 0.373 | 0.373 |
| sgr | 10 | 2.474 | 9.961 | 9.961 | 9.961 | 0.524 | 0.524 |
| tui | 146 | 2.621 | 3.637 | 8.585 | 14.287 | 0.725 | 2.720 |
| history | 12 | 2.392 | 20.578 | 20.578 | 20.578 | 0.252 | 0.279 |
| bulk-printstring | 42 | 2.769 | 3.949 | 4.817 | 4.817 | 1.327 | 140.509 |
| bulk-lines | 9 | 4.080 | 6.128 | 6.128 | 6.128 | 2.638 | 2.638 |
| multi-pane | 183 | 1.614 | 3.768 | 13.566 | 15.335 | 0.668 | 0.819 |
| resize | 182 | 1.671 | 3.195 | 12.583 | 13.631 | 0.496 | 0.791 |
| input-probe | 206 | 0.416 | 0.877 | 1.450 | 2.195 | 0.115 | 0.147 |

Малые sample counts не доказывают стабильный tail. Bulk-printstring включает 8-MiB фазу; max apply 140.509 ms показывает, что byte budget не гарантирует wall-time против cold work/descheduling.

Top-3 TUI по selected stage workload: paint 49.66% (393.484 ms), worker CPU submit 25.75% (204.062 ms), worker frame build 8.24% (65.331 ms). Знаменатель 792.374 ms: paint, apply, parse, host encode/write, worker build/submit и resize. Snapshot/collection исключены из суммы как nested; это не GPU critical path и не exclusive CPU profile.

ResizePseudoConsole: 31 samples, p50 0.012415 ms, p95 0.026623 ms, p99/max 0.037375 ms. Shaping/quad/IPC/ack и metadata lock waits сохранены в per-PID CSV. Аллокации изолированно не измерялись; числа allocations не заявляются.

SGR model output с таймерами и без них совпал: SHA-256 `bbf6f65f0a3ccdc92e488fdf5e4dcbb8c22cabfbd8439fa8685bdd094dea94e4`. Endpoint-ограничения и запуск: [GUI pipeline profile](2026-10-04-gui-pipeline-profile.md).

## PF-4: native input и fixed-dose throughput

Два immutable GUI/CLI набора; профилировочные таймеры выключены в обеих ветках. Warmup плюс пять чередующихся раундов; 100 application receipts под живым SGR bulk writer на раунд. Конец — Console.ReadKey shared-QPC receipt, не hardware key и не displayed pixel. При bulk приёмный поток не пишет echo в общий Console.Out. Readiness сопоставляется по nonce в shared-read CSV, а не по строке, которую flood может прокрутить.

| Pooled 500 receipts | Baseline ms | 64-KiB candidate ms |
| --- | ---: | ---: |
| p50 | 2.3000 | 1.5481 |
| p95 | 4.7732 | 4.9816 |
| p99 | 42.2215 | 18.3304 |
| max | 91.3259 | 63.1396 |

Pooled p99 улучшен на 56.6%, max на 30.9%; p95 +4.4%. Медиана пяти run-max: 61.4952 → 47.9653 ms (-22.0%). Медиана run-p99 6.8775 → 7.0448 ms практически неизменна; pooled p99 и median run-p99 — разные статистики, обе приведены.

8-MiB command-to-model completion включает CLI/polling: медиана 3.490 → 3.162 s, 2.292 → 2.530 MiB/s. Regression throughput не наблюдался; разброс велик, отдельный throughput speedup не зачтён.

16-KiB prototype: throughput ratio 0.9364, median p95 ratio 1.7348, p99 ratio 1.9314 — отклонён несмотря на хороший single smoke.

Budget не копирует chunks исходного String; для unmerged adjacent print actions выполняется такое же объединение текста, которое раньше делал Performer. Indivisible grapheme может быть больше soft budget. Parser backpressure/DEC 2026/query logic не менялись. Guard regression ловит возврат count-only gate; Unicode regression ловит потерю combining mark при byte-only split. NFC, wide, emoji/ZWJ, attributes/cursor и unmerged action boundary проверены на реальном model.

## PF-5 и отрицательные эксперименты

PF-5 full create/write/erase/scroll/resize/write/erase, 100 partial scrolls, пять alternating rounds в каждой из трёх серий:

| Серия | Baseline median ms | Move median ms | Baseline spread | Move spread |
| --- | ---: | ---: | ---: | ---: |
| 1 | 80.4489 | 39.6134 | 25.2162 | 6.9400 |
| 2 | 68.2601 | 40.6672 | 15.9825 | 19.2008 |
| 3 | 88.3770 | 40.9074 | 7.3452 | 14.4887 |

Целевой выигрыш 40–54%; не утверждается такой же выигрыш для всего GUI/TUI. Независимый margin test сохраняет outside cells, wide/hyperlink/pen attributes и отсутствие history; blank-destination mutation убита. Production smoke: `AABBBBBBAA`, `BBCCCCCCBB`, `CCDDDDDDCC`, `DD      DD`.

PF-2 SGR eviction серии: 131.988 → 133.570, 121.659 → 121.430, 157.892 → 173.925 ms. Gain не подтверждён; candidate не попал в product. Checkpoint поймал stale wrapped cache; новая checkpoint mutation ловит потерю blank attrs.

PF-3 checkpoint PartialEq/observable oracle: 107 tests и три убитые mutation (trim checkpoint, wide state, hyperlink cache). Пять alternating rounds дали cyrillic median gains, но control times гуляют в разы; credit не дан. Например ASCII narrow baseline 4.883–17.125 ms, candidate 7.535–23.756 ms. Это не доказанный regression algorithm, а недостаточная стабильность приёмки. Semantic Unicode fixtures сохранены.

PF-6 scanner/dispatch microbenchmark, 8192 iterations, пять alternating rounds: Cyrillic 136.895 → 189.495 ms, short SGR 69.628 → 307.302 ms, invalid/C1 116.847 → 255.445 ms; fragmentation также проиграла. Это не full VTParser throughput. Альтернатива удалена, production scanner прежний.

## PF-1: полная таблица этапов TUI

Cumulative per-PID histograms, ms; nested этапы не складывать.

| Metric | PID | Samples | p50 | p95 | p99 | max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| cached_cluster_shape | 101348 | 32925 | 0.000401 | 0.006527 | 0.035327 | 11.010047 |
| conpty.resize_pseudoconsole | 101348 | 1 | 0.021631 | 0.021631 | 0.021631 | 0.021631 |
| gui.host_process.frame_encode_write | 101348 | 144 | 0.282623 | 0.638975 | 1.097727 | 2.523135 |
| gui.host_process.frame_pipe_flush | 101348 | 144 | 0.000100 | 0.000301 | 0.000703 | 0.210943 |
| gui.host_process.ipc_enqueue | 101348 | 144 | 0.006207 | 0.013951 | 0.031103 | 0.225279 |
| gui.host_process.wire_frame_build | 101348 | 144 | 0.003311 | 0.013311 | 0.023807 | 0.026111 |
| gui.paint.collect | 101348 | 1314 | 0.001503 | 2.555903 | 3.293183 | 14.155775 |
| gui.paint.impl | 101348 | 146 | 2.621439 | 3.637247 | 8.585215 | 14.286847 |
| localpane.terminal_lock.hold.perform_actions | 101348 | 249 | 0.239615 | 0.350207 | 0.724991 | 2.719743 |
| localpane.terminal_lock.hold.render_snapshot | 101348 | 146 | 0.716799 | 0.958463 | 1.097727 | 1.130495 |
| localpane.terminal_lock.wait.dimensions | 101348 | 189 | 0.001003 | 0.003407 | 0.013119 | 0.022271 |
| localpane.terminal_lock.wait.keyboard_encoding | 101348 | 9 | 0.000703 | 0.003503 | 0.003503 | 0.003503 |
| localpane.terminal_lock.wait.perform_actions | 101348 | 249 | 0.003503 | 0.010943 | 0.032127 | 0.064511 |
| localpane.terminal_lock.wait.render_snapshot | 101348 | 146 | 0.000501 | 0.001407 | 0.001903 | 0.003503 |
| mux.pty.parse_and_buffer | 101348 | 5888 | 0.001407 | 0.006207 | 0.016511 | 0.075775 |
| shape.rustybuzz | 101348 | 1360 | 0.015359 | 0.057343 | 0.094719 | 10.682367 |
| gui.host_process.worker_ack_write_flush | 106812 | 144 | 0.011455 | 0.030975 | 0.050943 | 0.067071 |
| gui.host_process.worker_frame_build | 106812 | 144 | 0.325631 | 0.987135 | 1.343487 | 8.028159 |
| gui.host_process.worker_submit | 106812 | 144 | 0.933887 | 2.162687 | 4.685823 | 48.234495 |


## Финальная интеграционная проверка

- Final GUI/core/strip suite: 587 passed, 6 ignored, восемь suites.
- `cargo fmt --check` прошёл; stable rustfmt сообщает только существующее unsupported nightly `imports_granularity`.
- Affected-package all-target clippy `-D warnings` прошёл; final GUI/mux clippy повторён после последнего source change.
- Final native GUI/CLI rebuild в dev-install прошёл без освобождения user file locks.
- Final rebuilt GUI smoke: 100/100 application receipts при активном bulk writer, completed-model marker и GPU-worker/frame-present diagnostics.
- Все созданные XL/CF worktree и branches удалены. Предсуществовавшие worktree не трогались. Temporary smoke/examples, benchmark runners и девять копий benchmark executables удалены; raw CSV/JSON/logs оставлены в `.scratch` для разбора.
- Commit, push, tag и version bump не выполнялись.

