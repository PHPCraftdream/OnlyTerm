> **СУПЕРСЕДЕНО.** Пользователь решил не мигрировать SSH-клиент, а удалить его
> целиком (нужен только быстрый локальный терминал, удалённый доступ не нужен).
> См. `2026-07-23-remove-ssh-tls-feature.md`. Этот файл оставлен как история
> исследования (архитектура SessionInner/backend-agnostic API остаётся верной
> информацией о коде на момент удаления).

# План: миграция wezterm-ssh с libssh2/libssh-rs (C+OpenSSL) на russh (чистый Rust)

Дата: 2026-07-23. Репо: форк `PHPCraftdream/wezterm`, ветка `main`.

## Контекст и почему это вообще всплыло

При работе над задачей #10 (почини сборку `wezterm-font`) обнаружился независимый
блокер: `wezterm-font → config → wezterm-ssh → openssl (vendored)` требует Perl для
сборки OpenSSL из C-исходников, и нужного Perl-модуля на машине нет. Решение
(подтверждено пользователем): не чинить vendored OpenSSL точечно, а убрать саму
зависимость от него — заменить SSH-стек на `russh` (чистый Rust, tokio-async,
RustCrypto-бэкенд, без C/asm). Задача #10 закрыта (`deleted`) как более неактуальная.

**Решение по scope (подтверждено пользователем):** полная замена, один бэкенд —
`ssh2` и `libssh-rs` (оба C) удаляются целиком, остаётся только `russh`.

## Важное отличие от cairo→tiny-skia миграции

Там был один узкий, изолированный потребитель (5 файлов рендера глифов).
Здесь `wezterm-ssh` — core-функциональность (~6000 строк): session, channel, pty,
sftp, auth, host-key verification, keepalive, proxy. Это **security-critical** код:
баг в аутентификации или в проверке host-key — это не косметика, а риск для
реального доступа пользователя к машинам. Более высокая планка верификации,
чем у cairo (там было "визуально неотличимо", здесь нужны прогоны против
реального SSH-сервера, а не только unit-тесты).

## Результаты исследования (зафиксировано)

### Текущая архитектура — почему миграция реалистична

Ключевая находка: публичный API `wezterm-ssh` уже **backend-agnostic** и
async-facing:

- `wezterm-ssh/src/session.rs`: `Session` — тонкий фасад с async-методами
  (`connect`, `request_pty`, `exec`, `sftp`), использующий `smol::channel` для
  запросов/ответов. Сам `Session` не содержит ни строчки, завязанной на
  ssh2/libssh-rs напрямую.
- Реальная работа происходит в `SessionInner` (`sessioninner.rs`), которая
  живёт на **отдельном OS-треде** (`std::thread::spawn(move || inner.run())`)
  и общается с фасадом через каналы + self-pipe для будильника (неблокирующий
  сокет-пара для wakeup из блокирующих вызовов).
- `SessionInner::run_impl()` диспетчерит на `run_impl_libssh()` или
  `run_impl_ssh2()` в зависимости от `SshBackend` (`config/src/ssh.rs`,
  `enum SshBackend { Ssh2, LibSsh }`) — это ровно та точка, где нужно
  добавить `run_impl_russh()`.
- `auth.rs` / `host.rs`: аутентификация (agent/pubkey/password/keyboard-interactive)
  и host-key verification (`known_hosts`) реализованы как методы на
  `SessionInner`, с `#[cfg(feature = "ssh2")]` / `#[cfg(feature = "libssh-rs")]`
  вариантами бок о бок — прямой шаблон для третьего варианта на russh.
- `config.rs` (1636 строк) — парсинг `~/.ssh/config` **уже 100% своя Rust-реализация**,
  не делегирует в C-библиотеку. Ничего мигрировать не нужно, russh-бэкенд
  использует тот же `ConfigMap`.

Вывод: публичный контур (`Session`, `SessionEvent`, `SessionRequest`,
`SftpRequest`) можно оставить как есть — меняется только начинка
`SessionInner` (плюс `channelwrap.rs`/`sftpwrap.rs`/`dirwrap.rs`/`filewrap.rs`,
которые сейчас enum-обёртки над ssh2/libssh-rs типами). Вызывающий код в
`wezterm-gui`/`mux` не должен почувствовать разницы.

### russh — проверено (docs.rs)

- Полностью Rust, никаких C-биндингов; крипто — RustCrypto pure-Rust крейты
  (`aes`, `curve25519-dalek`, `ed25519-dalek`, `sha2`, `hmac`), опционально
  `ring`/`aws-lc-rs`/RSA — **для нулевого C/asm обязательно закрепить фичи
  на чистый RustCrypto-бэкенд**, не тянуть опциональные ring/aws-lc-rs.
- Agent forwarding — есть (упоминается `pageant`-зависимость + auth через агент).
- SFTP — отдельный компаньон-крейт `russh-sftp`.
- ProxyCommand/jump-хосты — через `russh-config` (не входит в сам russh, нужно
  проверить API при реализации).
- **Асинхронная модель на tokio** — текущий код синхронный/блокирующий внутри
  выделенного треда (`smol`-каналы поверх блокирующих ssh2/libssh-rs вызовов).
  russh-бэкенд для `SessionInner` будет либо гонять свой `tokio::runtime::Runtime`
  внутри того же выделенного треда (мост tokio↔smol через каналы, самое
  безопасное — не трогать остальную часть кодовой базы, которая на `smol`),
  либо (если решим сильнее рефакторить) переводить `SessionInner` целиком на
  async — это отдельное архитектурное решение, по умолчанию берём первый,
  менее инвазивный вариант (свой tokio runtime в треде).

### Тонкие места (не забыть при реализации)

1. **Host-key verification (`host.rs`)** — сейчас читает `known_hosts` файлы
   в формате OpenSSH (`ssh2::KnownHostFileKind::OpenSSH` / `libssh_rs`
   встроенный формат) и умеет писать новую запись при первом доверии.
   russh даёt свой интерфейс для проверки host-ключа (callback), но парсинг/запись
   `~/.ssh/known_hosts` в OpenSSH-формате, скорее всего, нужно реализовать
   самим (или найти отдельный крейт) — **это самое чувствительное место
   с точки зрения безопасности, требует отдельного явного тестового покрытия**
   (mismatch должен фейлить соединение, не молча продолжать).
2. **Auth flow** — три метода (`agent`, `pubkey` с passphrase-промптом,
   `password`, `keyboard-interactive`) сейчас реализованы как цикл с
   переспросом `auth_methods` после каждой попытки (пока не аутентифицируется
   или методы не кончатся). Логика должна быть портирована 1:1 на russh
   client auth API, включая passphrase-промпт через тот же `SessionEvent::Authenticate`
   канал наружу (GUI сам рисует промпт).
3. **SFTP** (`sftpwrap.rs`, `sftp/` — ~1170 строк) переезжает на `russh-sftp`,
   отдельный протокол поверх russh-канала — нужно свериться с типами
   (`sftp/types.rs`, 421 строка) на предмет 1:1 совместимости полей
   (permissions, timestamps, symlink handling).
4. **PTY/exec/channel** (`pty.rs`, `channelwrap.rs`) — сигналы (`SignalChannel`),
   resize PTY, exit-код процесса — проверить, что у russh есть эквиваленты
   (channel `request_pty`, `window_change`, `exec`, `exit_status`).
5. **Keepalive** (`serveraliveinterval` в `session.rs`) — russh должен
   поддерживать это либо встроенно, либо реализовать вручную через
   периодический keepalive-запрос по каналу.
6. **`SshBackend` enum** (`config/src/ssh.rs`) — после полной замены варианты
   `Ssh2`/`LibSsh` убираются, значение по умолчанию меняется на единственный
   russh-бэкенд (или сам enum убирается, если выбора бэкенда больше не будет).

### Инструменты для безручной верификации (тот же принцип, что и для cairo)

SSH — не то же самое, что рендер глифов: тут diff двух PNG не поможет,
нужна **живая проверка протокола**. Но управление живым GUI/скриншоты
по-прежнему не нужны — верификация происходит на уровне крейта `wezterm-ssh`,
через integration-тесты с реальным SSH-сервером:

- **Тестовый SSH-сервер — сам russh в серверном режиме.** russh поддерживает
  не только клиент, но и сервер — поднимаем минимальный in-process test-сервер
  (пароль/pubkey auth заранее известны, слушает на localhost:<random port>)
  прямо в тесте, без внешних зависимостей (никакого системного `sshd`,
  никакого Docker) — работает одинаково в CI и локально, полностью
  скриптуемо агентом.
  Альтернатива, если russh-server окажется неудобен для тестового хоста:
  проверить, есть ли на машине системный OpenSSH-сервер (Windows 10/11 имеет
  опциональный компонент "OpenSSH Server") — но in-process russh-сервер
  предпочтителен, т.к. не требует прав администратора/системных фич.
- **`wezterm-ssh/tests/integration.rs`** (новый) — гоняет: connect + auth
  (pubkey и password) + exec простой команды + чтение вывода + resize PTY +
  sftp put/get/stat + сценарий mismatch host-key (должен вернуть ошибку,
  не тихо продолжить). Всё через тестовый russh-сервер выше — полностью
  автоматически, без участия пользователя.
- Для не-функциональных проверок (сборка, отсутствие openssl в дереве
  зависимостей) — `cargo tree -p wezterm-ssh | grep -i openssl` должен быть
  пустым после миграции; `cargo build --workspace` должен проходить без
  Perl/OpenSSL шага вообще.

### Порядок работ

- **0. Тулинг.** `wezterm-ssh/tests/support/test_server.rs` — in-process
  russh-сервер для тестов (auth, pty, exec, sftp). Это разблокирует все
  следующие шаги тестами, а не ручной проверкой.
- **1. `SessionInner::run_impl_russh()` — базовый connect + auth.** Новый
  backend-вариант: подключение, host-key verification (собственный
  known_hosts парсер/writer в OpenSSH-формате), agent/pubkey/password/
  keyboard-interactive auth 1:1 с текущей логикой. Тесты: connect+auth
  сценарии из тулинга.
- **2. PTY/exec/channel на russh.** Перенос `pty.rs`/`channelwrap.rs`:
  request_pty, resize, exec, сигналы, exit-код. Тесты: exec простой команды,
  resize, проверка сигнала.
- **3. SFTP на `russh-sftp`.** Перенос `sftpwrap.rs`/`sftp/*`. Тесты:
  put/get/stat/readdir/symlink (при поддержке).
- **4. Keepalive + edge cases.** `serveraliveinterval`, обработка обрыва
  соединения, host-key mismatch (явный тест на "должен зафейлить, не продолжить").
- **5. Вычистка.** Убрать `ssh2`, `libssh-rs`, `openssl`/`async_ossl`(там, где
  единственный потребитель — SSH) из `wezterm-ssh/Cargo.toml` и корневого
  workspace, убрать enum `SshBackend`/переключение бэкендов из
  `config/src/ssh.rs`, закрепить russh на чисто RustCrypto-фичах.
  `cargo tree | grep -i openssl` — пусто. `cargo build --workspace` без Perl.
- **6. Верификация.** Полный прогон `wezterm-ssh/tests/integration.rs`,
  `cargo test --workspace`, и (если пользователь захочет) один ручной sanity-
  коннект к реальному внешнему хосту как последняя проверка — но это
  единственное место, где решение о ручном участии за пользователем, не по
  умолчанию.

## Риски

- Host-key verification — самое чувствительное место, тестировать отдельно
  сценарий mismatch (должен рвать соединение).
- russh's ProxyCommand/jump-host поддержка через `russh-config` — не проверена
  предметно, может потребовать самостоятельной реализации при недостаточном
  покрытии.
- tokio-runtime внутри существующего `smol`-based треда — мост между двумя
  async-рантаймами, нужно аккуратно с блокировкой/deadlock при `smol::block_on`
  внутри callback'ов (сейчас `authenticate_libssh`/`pubkey_auth` дергают
  `smol::block_on` из callback'а C-библиотеки — на russh это будет по-другому,
  вероятно, через async fn на trait, без блокирующего моста).
- Библиотека `russh` активно развивается — версии/API могут отличаться от
  того, что видели в доках; на этапе реализации свериться с фактической
  версией в `Cargo.lock`/crates.io на момент работы.
