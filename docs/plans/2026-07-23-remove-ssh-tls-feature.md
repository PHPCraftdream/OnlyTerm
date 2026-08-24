# План: удалить SSH-клиент и TLS-mux целиком (не мигрировать — удалить)

Дата: 2026-07-23. Заменяет `2026-07-23-ssh-russh-migration.md`. Причина смены
направления: пользователю нужен быстрый локальный терминал, удалённый доступ
(ни по SSH, ни по TLS) не нужен. При исследовании russh выяснилось, что даже
он требует C/asm крипто-ядро (`ring`/`aws-lc-rs`) — чистого RustCrypto-варианта
для SSH AEAD-шифров не существует. Раз ценность фичи для пользователя нулевая,
дешевле и надёжнее убрать её целиком, чем тащить C/asm ради неиспользуемой
возможности.

## Scope (подтверждено пользователем)

Удаляются **обе** сетевые фичи, использующие OpenSSL:
1. **SSH-клиент** (`onlyterm ssh`, SSH-domains в конфиге).
2. **TLS-mux** (`TlsDomainClient`/`TlsDomainServer` — подключение к удалённому
   `onlyterm-mux-server` по сети без SSH).

**Остаётся без изменений**: локальный мультиплексор через `UnixDomain`
(юникс-сокет/named pipe на той же машине — detach/reattach, `onlyterm-mux-server`
локально) — он не использует OpenSSL и не зависит от удаляемого кода.

## Результаты исследования — периметр удаления

### Кто зависит от onlyterm-ssh (`Cargo.toml` во всех этих крейтах)

`config`, `mux`, `onlyterm-client`, `onlyterm-gui`, `lua-api-crates/ssh-funcs`,
корневой workspace `Cargo.toml`.

### Кто зависит от async_ossl/openssl (шире, чем просто SSH — TLS-mux тоже)

`onlyterm-client` (`client.rs`, 1392 строк — судя по размеру, обрабатывает
и TLS-, и Unix-домены вперемешку, нужно аккуратно вычленить только Unix-путь),
`onlyterm-mux-server` (`ossl.rs`, 188 строк — вероятно, весь файл под удаление),
`onlyterm-mux-server-impl` (`dispatch.rs`, 213 строк — тоже смешанный код).

### Файлы/модули под удаление или правку

| Путь | Строк | Действие |
|---|---|---|
| `onlyterm-ssh/` (весь крейт) | ~6000 | удалить целиком |
| `mux/src/ssh.rs` | 1148 | удалить целиком |
| `onlyterm-mux-server/src/ossl.rs` | 188 | удалить целиком |
| `lua-api-crates/ssh-funcs/` (весь крейт) | 43 | удалить целиком |
| `config/src/ssh.rs` | 185 | удалить целиком (SshDomain, SshBackend) |
| `config/src/tls.rs` | 105 | удалить целиком (TlsDomainClient/Server) |
| `onlyterm-client/src/client.rs` | 1392 | **правка** — вычленить и оставить только Unix-domain путь, убрать TLS/SSH-специфичный код |
| `onlyterm-mux-server-impl/src/dispatch.rs` | 213 | **правка** — то же самое, оставить только Unix-listener путь |
| `config/src/unix.rs` | 131 | **не трогать** — это то, что остаётся |
| `onlyterm/src/main.rs` | — | убрать `SubCommand::Ssh`, убрать SSH/TLS-ветки из `SubCommand::Connect` (оставить только unix/local-домены, если Connect используется и для них — проверить при реализации) |
| `onlyterm-gui/src/main.rs` | — | убрать ссылки на `SshDomain`/`TlsDomain` (launcher-меню, автосоздание доменов из ssh_config) |
| Корневой `Cargo.toml` | — | убрать `onlyterm-ssh` из `workspace.members`, убрать `ssh2`/`libssh-rs`/`openssl`/`async_ossl`(если после этого больше никем не используется)/`git2`(нет, не относится) |
| `Cargo.lock` | — | пересобрать после правок |

### Тонкое место: `onlyterm-client`/`onlyterm-mux-server-impl` смешивают Unix и TLS/SSH пути

Судя по размеру файлов (1392 и 213 строк), логика подключения к мультиплексору,
скорее всего, написана как один клиент/диспетчер с веткой по типу домена
(Unix/Tls/Ssh) в одном месте, а не три отдельных модуля. Нужно на этапе
реализации внимательно вычленить Unix-ветку и не сломать её, убирая
Tls/Ssh-ветки — **это единственное реально рискованное место всего удаления**
(риск случайно задеть работающий локальный мультиплексор).

### Тулинг для безручной верификации

Никакого нового тулинга строить не нужно — верификация здесь про то, что
**ничего не сломалось** в оставшейся (локальной) функциональности:
- `cargo build --workspace` — должен пройти без единого upstream-запроса на
  openssl/ssh2/libssh-rs в графе зависимостей.
- `cargo tree --workspace | grep -iE "openssl|ssh2|libssh"` — пусто.
- Существующий локальный мультиплексор: `onlyterm-mux-server` (`UnixDomain`)
  запустить и подключиться локальным клиентом, детач/реаттач — через
  существующие `/run`/`/verify` skills (реальный живой прогон, не только
  cargo test), так как это единственная часть, которую реально можно сломать
  этим удалением.

### Порядок работ

- **R1. Удалить SSH-клиент.** `onlyterm-ssh/` целиком, `mux/src/ssh.rs`,
  `lua-api-crates/ssh-funcs/` целиком, `config/src/ssh.rs`
  (SshDomain/SshBackend), `SubCommand::Ssh` в `onlyterm/src/main.rs`, ссылки в
  `onlyterm-gui/src/main.rs`. Убрать зависимости из всех перечисленных
  `Cargo.toml`.
- **R2. Удалить TLS-mux.** `config/src/tls.rs` (TlsDomainClient/Server),
  `onlyterm-mux-server/src/ossl.rs` целиком, TLS-ветки в
  `onlyterm-client/src/client.rs` и `onlyterm-mux-server-impl/src/dispatch.rs`
  (аккуратно, не задев Unix-ветку — см. тонкое место выше), TLS-ветки в
  `ConnectCommand`/`SubCommand::Connect` в `onlyterm/src/main.rs`.
- **R3. Вычистка Cargo.toml/workspace.** Убрать `ssh2`, `libssh-rs`, `openssl`,
  `async_ossl` (если не используется больше нигде — проверить перед удалением,
  напрямую grep `async_ossl`/`openssl` по всему workspace после R1+R2), убрать
  `onlyterm-ssh` из `workspace.members`. `cargo tree` — чисто.
- **R4. Верификация.** `cargo build --workspace` без Perl/OpenSSL-шага,
  `cargo test --workspace`, живой прогон локального мультиплексора
  (`/run`/`/verify`): запуск `onlyterm-mux-server` локально, подключение,
  детач/реаттач панели — должно работать как раньше.

## Риски

- Смешанный код в `onlyterm-client`/`onlyterm-mux-server-impl` — главный риск,
  не задеть рабочий Unix-domain путь при удалении TLS/SSH веток.
- `SubCommand::Connect` может быть общим для Unix/Tls/Ssh доменов — проверить,
  что после удаления команда `onlyterm connect <unix-domain-name>` продолжает
  работать.
- Нужно перепроверить `async_ossl`/`openssl` действительно больше нигде не
  нужны после R1+R2, прежде чем убирать из workspace (иначе сборка сломается
  для остающегося функционала).

## Влияние на другие треки этой инициативы

- Задача **L4c** (Lua→rhai, `docs/plans/2026-07-23-lua-rhai-migration.md`)
  ранее включала порт `lua-api-crates/ssh-funcs` — теперь этот крейт просто
  удаляется в R1, L4c больше не должен его портировать (обновлено в TaskList).
- Старые задачи #11-17 (russh-миграция) удалены из TaskList — заменены на
  R1-R4 в этом плане.
