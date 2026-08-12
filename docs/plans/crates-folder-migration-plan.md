# План: перенос всех крейтов wezterm в `crates/`

(Задача TaskList #127 — только план, перенос не выполнялся)

## 1. Полный список того, что переезжает в `crates/`

Все 42 директории верхнего уровня со своим `Cargo.toml` (explicit `workspace.members`, `exclude`-крейты и крейты, подтягиваемые только через `path =` в `[workspace.dependencies]`):

`base91, bidi, bidi/generate, bintree, codec, color-types, config, env-bootstrap, filedescriptor, frecency, lfucache, mux, procinfo, promise, pty, rangeset, ratelim, strip-ansi-escapes, sync-color-schemes, tabout, term, termwiz, umask, vtparse, wezterm, wezterm-blob-leases, wezterm-cell, wezterm-char-props, wezterm-client, wezterm-dynamic, wezterm-escape-parser, wezterm-font, wezterm-gui, wezterm-gui-subcommands, wezterm-input-types, wezterm-mux-server, wezterm-mux-server-impl, wezterm-open-url, wezterm-surface, wezterm-toast-notification, wezterm-uds, wezterm-version, window`.

Плюс переносится как единое целое вместе с родителем:
- `bidi/generate` → `crates/bidi/generate`.
- `termwiz/codegen`, `wezterm-char-props/codegen` (в `exclude`, но физически вложены) → едут автоматически вместе с родителями.
- `deps/fontconfig` (крейт `fontconfig`) — рекомендация: перенести в `crates/fontconfig` для единообразия, поправить `path = "deps/fontconfig"`.
- `lua-api-crates/*` (14 под-крейтов) — вся директория переезжает как `crates/lua-api-crates/*`, сохраняя внутреннюю структуру.

Итого: **62 файла Cargo.toml** физически меняют путь (61 крейт/под-крейт + корневой воркспейс).

## 2. Что остаётся в корне

`Cargo.toml` (workspace), `Cargo.lock`, `README.md`, `LICENSE.md`, `CONTRIBUTING.md`, `PRIVACY.md`, `Makefile`, `.dockerignore`, `.gitignore`, `.rustfmt.toml`, `cooldown.toml`, `deny.toml`, `mkdocs_macros.py`, `.github/`, `.cargo/`, `assets/`, `ci/`, `docs/`, `licenses/`, `nix/`, `test-data/`, `target/`.

## 3. Места, требующие правок путей

- **Корневой `Cargo.toml`**: `members = [...]` (14 явных записей → префикс `crates/`), `exclude = [...]` (2 записи → префикс `crates/`), `[workspace.dependencies]` — все ~50+ `path = "..."` записей получают префикс `crates/`.
- **Path-зависимости внутри Cargo.toml каждого крейта** — если все крейты остаются прямыми детьми `crates/`, относительные пути между соседями (`../other-crate`) не меняются. Требуется точечная перепроверка каждого из 62 файлов при выполнении.
- **`include_bytes!` через границу крейта** (критично для атомарности переноса):
  - `term/src/terminalstate/mod.rs:40` — `include_bytes!("../../../termwiz/data/wezterm")`.
  - `lua-api-crates/termwiz-funcs/src/lib.rs:155` — `include_bytes!("../../../termwiz/data/xterm-256color")`.
  - Безопасно ТОЛЬКО если весь перенос делается одним атомарным шагом (все участвующие крейты становятся siblings под `crates/` одновременно).
- **`.github/workflows/*.yml`** — сгенерированы из `ci/generate-workflows.py`; используют `cargo build -p <crate>` (имя пакета, не путь) — правок не требуют.
- **`Makefile`** — только `-p <crate>` — правок не требуют.
- **`nix/flake.nix:235`** — `${finalAttrs.src}/termwiz/data/wezterm.terminfo` → `.../crates/termwiz/data/wezterm.terminfo`.
- **`ci/deploy.sh` (строки 39, 412)** и **`ci/generate-workflows.py` (строка 37, `EXTRA_INPUT_PATHS`)** — `termwiz/data/wezterm.terminfo` → `crates/termwiz/data/wezterm.terminfo`.
- **`ci/generate-docs.py`, `.gitignore`, `rust-toolchain.toml`, `.cargo/config.toml`** — проверены, правок не требуют.

## 4. Стратегия миграции

Один коммит на `git mv <crate> crates/<crate>` для ВСЕХ директорий разом (сохраняет git-историю), затем отдельный коммит с правками путей (корневой `Cargo.toml`, `nix/flake.nix`, `ci/deploy.sh`, `ci/generate-workflows.py`). Разбивать перенос по частям НЕ рекомендуется — кросс-крейтовые `include_bytes!` (term/termwiz-funcs → termwiz) ломаются в любом промежуточном состоянии.

## 5. Оценка масштаба и рисков

- 62 файла Cargo.toml перемещаются, из них содержательные правки — минимум в 1 (корневой, ~65 строк).
- 3 внешних файла с точечными правками путей (nix/flake.nix, ci/deploy.sh, ci/generate-workflows.py).
- Главный риск: 2 `include_bytes!`, требующие строго одновременного переноса term + termwiz-funcs + termwiz.
- Символических ссылок не обнаружено.
