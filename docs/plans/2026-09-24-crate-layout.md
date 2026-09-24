# План: разложить пакеты Cargo по назначению

Статус: исследование, изменений путей и кода нет. Область этого плана — **только физическое размещение**. Имена пакетов, crate name, версии, публичные API, зависимости и поведение не меняются. Переименование и возможное объединение пакетов — отдельные решения.

## Отношение к прежнему плану

[`crates-folder-migration-plan.md`](crates-folder-migration-plan.md) описывает прежний перенос пакетов из корня в плоскую `crates/`. Этот перенос уже состоялся. В старом документе упоминаются `lua-api-crates` и `nix/flake.nix`, которых в текущем дереве нет; он не является инструкцией для нынешнего состояния. Здесь исходной точкой служат текущие пути `crates/*`, `crates/api-crates/*` и `xtask`. Сохраняем его полезный вывод о перекрёстных `include_bytes!`, но проверяем реальные пути заново.

По `cargo metadata --no-deps --format-version 1` на исходном дереве: **54 члена workspace**. Кроме них существуют один исключённый генератор (`onlyterm-char-props/codegen`) и локально исправленная копия `wgpu-hal`, заданная через `[patch.crates-io]`. Всего найдено 57 `Cargo.toml`: 54 члена, эти два отдельных пакета и корневой виртуальный манифест. В `workspace.exclude` всё ещё указан `crates/termwiz/codegen`, но такого каталога сейчас нет; строка устарела. Пакет `xtask` остаётся в корне.

## Целевое дерево

Ниже перечислены **все 54 члена workspace**. Путь справа задан относительно корня репозитория; вложенные `derive` и `generate` перемещаются вместе с родителем. Группы имеют лишь один дополнительный уровень под `crates/`.

| Пакет | Сейчас | После |
| --- | --- | --- |
| `onlyterm` | `crates/onlyterm` | `crates/apps/onlyterm` |
| `onlyterm-gui` | `crates/onlyterm-gui` | `crates/apps/onlyterm-gui` |
| `onlyterm-client` | `crates/onlyterm-client` | `crates/apps/onlyterm-client` |
| `onlyterm-mux-server` | `crates/onlyterm-mux-server` | `crates/apps/onlyterm-mux-server` |
| `onlyterm-gui-subcommands` | `crates/onlyterm-gui-subcommands` | `crates/apps/onlyterm-gui-subcommands` |
| `onlyterm-term` | `crates/term` | `crates/terminal/term` |
| `termwiz` | `crates/termwiz` | `crates/terminal/termwiz` |
| `onlyterm-bidi` | `crates/bidi` | `crates/terminal/bidi` |
| `generate-bidi` | `crates/bidi/generate` | `crates/terminal/bidi/generate` |
| `onlyterm-cell` | `crates/onlyterm-cell` | `crates/terminal/onlyterm-cell` |
| `onlyterm-char-props` | `crates/onlyterm-char-props` | `crates/terminal/onlyterm-char-props` |
| `onlyterm-escape-parser` | `crates/onlyterm-escape-parser` | `crates/terminal/onlyterm-escape-parser` |
| `onlyterm-input-types` | `crates/onlyterm-input-types` | `crates/terminal/onlyterm-input-types` |
| `onlyterm-color-types` | `crates/color-types` | `crates/terminal/color-types` |
| `vtparse` | `crates/vtparse` | `crates/terminal/vtparse` |
| `strip-ansi-escapes` | `crates/strip-ansi-escapes` | `crates/terminal/strip-ansi-escapes` |
| `mux` | `crates/mux` | `crates/session/mux` |
| `onlyterm-mux-server-impl` | `crates/onlyterm-mux-server-impl` | `crates/session/onlyterm-mux-server-impl` |
| `portable-pty` | `crates/pty` | `crates/session/pty` |
| `codec` | `crates/codec` | `crates/session/codec` |
| `base91` | `crates/base91` | `crates/session/base91` |
| `bintree` | `crates/bintree` | `crates/session/bintree` |
| `config` | `crates/config` | `crates/configuration/config` |
| `onlyterm-config-derive` | `crates/config/derive` | `crates/configuration/config/derive` |
| `onlyterm-dynamic` | `crates/onlyterm-dynamic` | `crates/configuration/onlyterm-dynamic` |
| `onlyterm-dynamic-derive` | `crates/onlyterm-dynamic/derive` | `crates/configuration/onlyterm-dynamic/derive` |
| `onlyterm-color-schemes-data` | `crates/onlyterm-color-schemes-data` | `crates/configuration/onlyterm-color-schemes-data` |
| `sync-color-schemes` | `crates/sync-color-schemes` | `crates/configuration/sync-color-schemes` |
| `color-funcs` | `crates/api-crates/color-funcs` | `crates/configuration/api-crates/color-funcs` |
| `mux-funcs` | `crates/api-crates/mux-funcs` | `crates/configuration/api-crates/mux-funcs` |
| `termwiz-funcs` | `crates/api-crates/termwiz-funcs` | `crates/configuration/api-crates/termwiz-funcs` |
| `onlyterm-font` | `crates/onlyterm-font` | `crates/graphics/onlyterm-font` |
| `onlyterm-gpu-render` | `crates/onlyterm-gpu-render` | `crates/graphics/onlyterm-gpu-render` |
| `onlyterm-gpu-protocol` | `crates/onlyterm-gpu-protocol` | `crates/graphics/onlyterm-gpu-protocol` |
| `onlyterm-gui-render-thread` | `crates/onlyterm-gui-render-thread` | `crates/graphics/onlyterm-gui-render-thread` |
| `onlyterm-surface` | `crates/onlyterm-surface` | `crates/graphics/onlyterm-surface` |
| `window` | `crates/window` | `crates/graphics/window` |
| `filedescriptor` | `crates/filedescriptor` | `crates/platform/filedescriptor` |
| `procinfo` | `crates/procinfo` | `crates/platform/procinfo` |
| `umask` | `crates/umask` | `crates/platform/umask` |
| `onlyterm-elevated-transport` | `crates/onlyterm-elevated-transport` | `crates/platform/onlyterm-elevated-transport` |
| `onlyterm-uds` | `crates/onlyterm-uds` | `crates/platform/onlyterm-uds` |
| `onlyterm-open-url` | `crates/onlyterm-open-url` | `crates/platform/onlyterm-open-url` |
| `onlyterm-toast-notification` | `crates/onlyterm-toast-notification` | `crates/platform/onlyterm-toast-notification` |
| `promise` | `crates/promise` | `crates/support/promise` |
| `rangeset` | `crates/rangeset` | `crates/support/rangeset` |
| `ratelim` | `crates/ratelim` | `crates/support/ratelim` |
| `lfucache` | `crates/lfucache` | `crates/support/lfucache` |
| `frecency` | `crates/frecency` | `crates/support/frecency` |
| `tabout` | `crates/tabout` | `crates/support/tabout` |
| `onlyterm-blob-leases` | `crates/onlyterm-blob-leases` | `crates/support/onlyterm-blob-leases` |
| `env-bootstrap` | `crates/env-bootstrap` | `crates/support/env-bootstrap` |
| `onlyterm-version` | `crates/onlyterm-version` | `crates/support/onlyterm-version` |
| `xtask` | `xtask` | `xtask` (исключение) |

Дополнительный манифест: `crates/onlyterm-char-props/codegen` → `crates/terminal/onlyterm-char-props/codegen`; его новый путь остаётся в `workspace.exclude`. Устаревшую строку `crates/termwiz/codegen` удалить из `exclude` при миграции, убедившись, что генератор не восстанавливается другой веткой. Патч `crates/wgpu-hal-vendored` остаётся на месте: это vendored сторонний исходник, и перенос ради симметрии увеличил бы шум diff; `[patch.crates-io]` сохраняет текущий путь. `xtask` остаётся рядом с корневым `Cargo.toml`, поскольку `xtask/src/main.rs::repo_root()` берёт родительский каталог `CARGO_MANIFEST_DIR`. Генератор и `derive` не становятся отдельными верхнеуровневыми группами.

## Что должно измениться при переносе

Cargo допускает пути и glob-шаблоны в `workspace.members`, а локальные `path`-зависимости внутри дерева workspace могут стать участниками автоматически. Поэтому простой подсчёт строк `members` недостаточен: после каждого этапа проверять фактический список через `cargo metadata`. Корневые `members`, `exclude`, около 50 `workspace.dependencies` с `path`, локальные `path` в дочерних манифестах и `[patch.crates-io]` сверять с картой выше. Для этого плана лучше сохранить явные `members` как сейчас, добавив новые пути, без широкого `crates/*/*`: шаблон случайно включит генераторы или vendored пакет. `package.workspace`, если встретится при реализации, сверять отдельно. [Cargo Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html), [Cargo dependency paths](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html), [Cargo patches](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html).

Места, где текстовый поиск `path =` не поймает проблему:

- `crates/apps/onlyterm-gui/build.rs` и `crates/apps/onlyterm/build.rs`, `crates/apps/onlyterm-mux-server/build.rs`: подъём от текущего каталога на два уровня перестанет вести в корень; менять на вычисление корня с проверяемым инвариантом. В GUI обновить чтение `.tag`, `cargo:rerun-if-changed` для неё и `assets/windows/terminal.ico`. В `crates/support/onlyterm-version/build.rs` проверить чтение `../.tag` относительно manifest dir (сейчас оно указывает на `crates/.tag`) и согласовать с GUI, отдельно проверив worktree и release tag. Это обнаруженный риск, не повод менять семантику версии в переносе без теста.
- `crates/apps/onlyterm-gui/src/window/termwindow/mod.rs`: `include_bytes!` иконки из `assets/icon`; `crates/graphics/onlyterm-font/src/parser.rs` и тест `src/font/locator/mod.rs`: встроенные шрифты/путь через `CARGO_MANIFEST_DIR`. При углублении на одну папку добавить один подъём или, где уместно, строить путь от явного корня.
- `crates/terminal/term/src/terminal/terminalstate/mod.rs` содержит `include_bytes!` данных `termwiz`; `crates/configuration/api-crates/termwiz-funcs/src/lib.rs` также читает `termwiz/data`. Пересчитать их относительно новых мест. Внутренние `termwiz`/`bidi`/`onlyterm-char-props` `data`, `include!`, codegen и примеры переезжают вместе с родителем, но их проверять компиляцией, включая features и примеры.
- `xtask/src/main.rs::package_dir()` ищет на ограниченную глубину 3. После переноса прямой пакет окажется на глубине 3 от `crates`, а `derive`/`api-crates/*` на глубине 4–5. Изменить поиск (лучше использовать фактический список manifest paths из Cargo metadata или явный root-aware обход с тестом), включая `onlyterm-term`/`portable-pty`, где имя пакета и каталога уже не совпадают. `repo_root()` в `xtask` оставляем корректным, так как сам пакет не движется.
- `.github/workflows/termwiz.yml`: обновить **оба** path-фильтра и `Swatinem/rust-cache` `workspaces: crates/termwiz`. Основные сгенерированные Windows workflows используют `**/*.rs`/`**/Cargo.toml`, но проверить реальные фильтры, а не только генератор `ci/generate-workflows.py`; если меняется генератор, регенерировать и проверить diff. `CONTRIBUTING.md` сейчас содержит много конкретных ссылок на `crates/term`, `crates/mux`, benchmarks; их обновить вместе с `ci/*.ps1`, `ci/*.sh`, `.cargo/config.toml`, `Makefile`, docs и scripts после поиска старых путей. Текстовые ссылки в комментариях и документации поправить по мере переноса, не смешивая с функциональным рефакторингом.
- Генерируемые ресурсы (`termwiz/data/*`, `bidi/data/*`, `onlyterm-char-props/data/*`, цветовые схемы, иконки и шрифты) должны остаться доступными с новых путей. Для каждого генератора, примера и build script проверить его рабочий каталог и реальные входные/выходные пути; не перемещать `assets/` в этом плане.

## Порядок внедрения и проверка

1. Зафиксировать baseline на чистой ветке: `cargo metadata --no-deps --format-version 1`, `git status`, список `Cargo.toml`, текущие CI-фильтры и `Cargo.lock`. Сохранить baseline вне репозитория или в одноразовом файле, который не попадёт в коммит. Скриптом извлечь из metadata отсортированные `{name, version, targets(kind/name), dependencies(name/req/kind/optional/features/target), workspace-member}`; не сравнивать абсолютные manifest paths или package IDs, они закономерно изменятся. Ожидание: 54 уникальных member names, один реально существующий исключённый генератор и неизменный patch; строку несуществующего `termwiz/codegen` удалить осознанно.
2. Переносить атомарными **рабочими партиями**: `support` + `platform`; `configuration` (включая api-crates и derive); `terminal` (вместе с cross-crate data); `session`; `graphics`; `apps`. В каждой партии `git mv` целые каталоги, одновременно править все входящие/исходящие Cargo paths и вычисления файловых путей, чтобы конец партии компилировался. Не делать «сначала все mv, потом починить манифесты» как в старом плане: промежуточный коммит был бы нерабочим. Из-за пересечений выполнить партии последовательно в одном интеграционном дереве; ветки с независимыми частями возможны только после фиксации интерфейса путей.
3. После каждой партии: `cargo metadata --no-deps --format-version 1` успешно, сравнение проекции с baseline без отличий, `cargo check -p` всех переехавших пакетов и прямых потребителей, точечные тесты затронутых crates; проверить `git diff --check` и поиск старых путей. Для вложенных генераторов и примеров выполнить их документированные команды/сборку. Исправлять любой сбой в этой же партии.
4. На финальном дереве: `cargo check --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked` на поддерживаемой Windows-среде (при уже известных тяжёлых наборах разбить команды, но не пропускать их молча), `cargo check --workspace --all-features --locked` где поддерживается; проверить `cargo run -p xtask -- lint-check` и его `package_dir`, Windows GUI/консольные ресурсы и release-путь `.tag`. Запустить/проверить генератор workflows и `termwiz` CI с новыми фильтрами. Сравнить `Cargo.lock` с baseline: **никаких новых версий или зависимостей**, допускаются только изменившиеся локальные идентификаторы/пути, если Cargo их отражает. Проверить diff на случайно перенесённые `target/` или приватные пути.
5. При провале партии откатить **только её собственные** `git mv` и правки по журналу перемещений; чужие изменения и предыдущие принятые партии сохранять. Если автоматический откат небезопасен из-за чужих изменений, остановиться и согласовать точные конфликтующие файлы. После отката повторить metadata-проекцию и targeted checks. Без коммита/пуша реализации до успешной приёмки; решение о слиянии отдельно.

Точная проверка invariant: сохранить до и после JSON-вывод `cargo metadata --no-deps --format-version 1`; скрипт должен строить словарь пакетов по `name`, требовать 54 ключа и равенство `version`, имён/типов targets, нормализованных зависимостей и отсортированного множества имён `workspace_members`. Исключённый `onlyterm-char-props/codegen` и patch проверять отдельно по их `Cargo.toml` и разрешению `cargo metadata`, поскольку они не входят в эти 54 пакета. Это ловит случайно потерянный или вновь добавленный пакет лучше одной проверки `cargo check`.

Например, сохранить вывод metadata в `before.json`/`after.json` вне репозитория и выполнить следующую одноразовую проверку (файлы не коммитить):

```python
import json
import sys


def projection(filename):
    data = json.load(open(filename, encoding="utf-8"))
    packages = data["packages"]
    assert len(packages) == 54
    assert len({p["name"] for p in packages}) == 54
    member_ids = set(data["workspace_members"])
    members = sorted(p["name"] for p in packages if p["id"] in member_ids)
    result = {}
    for package in packages:
        targets = sorted((t["name"], tuple(t["kind"])) for t in package["targets"])
        deps = []
        for dep in package["dependencies"]:
            deps.append({key: dep.get(key) for key in (
                "name", "req", "kind", "optional", "uses_default_features",
                "features", "target", "registry", "rename",
            )})
        deps.sort(key=lambda d: json.dumps(d, sort_keys=True))
        result[package["name"]] = (package["version"], targets, deps)
    return members, result


assert projection(sys.argv[1]) == projection(sys.argv[2])
print("54 workspace packages and manifest semantics unchanged")
```

Команда: `python compare_metadata.py before.json after.json`. В отдельном сравнении проверить, что `workspace_root` не изменился, разрешение `wgpu-hal` всё ещё указывает на vendored patch, а `Cargo.lock` не сменил версии. Сам Python-файл можно положить вне репозитория; если проверка понадобится регулярно, оформить его как тестовый скрипт отдельным решением.
