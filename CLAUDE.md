# OnlyTerm (форк wezterm под Windows)

## СТОП-ПРАВИЛО: процессы OnlyTerm неприкосновенны

**Никогда не завершать процессы `onlyterm-gui.exe` / `onlyterm.exe`** — ни `taskkill` (ни `/IM`, ни `/PID`), ни `Stop-Process`, ни `pkill`, ни любым другим способом.

Единственное исключение: процесс, который **ты сам запустил в этой же сессии** и чей PID лично зафиксировал при запуске — только по этому точному PID.

Причина не гипотетическая. 2026-08-09 в 20:33 делегированная crush-сессия выполнила
`taskkill /F /PID 25824 /PID 21324 /PID 6260 /PID 28796 /PID 35136`, чтобы снять файловую
блокировку с `target/debug/onlyterm-gui.exe` и дать пройти `cargo build`. Она уничтожила
четыре рабочие сессии пользователя (три из них жили с 5 августа) и окно, внутри которого
работал сам Claude Code, — то есть собственного предка, и умерла вместе с ним. Дампов нет,
логов нет: `TerminateProcess` не оставляет следов. Восстановить содержимое сессий невозможно.

### Если `cargo build` падает с «Access is denied» на `onlyterm-gui.exe`

Это значит, что запущен экземпляр OnlyTerm и он держит файл. **Остановиться и сообщить
пользователю.** Не «расчищать» блокировку. Пользователь сам закроет нужное окно.

### Обязательно вставлять в каждый промпт делегируемому агенту (crush и др.)

```
HARD CONSTRAINT -- process safety:
Never terminate any onlyterm-gui.exe / onlyterm.exe process, by ANY mechanism --
not taskkill (neither /IM nor /PID), not Stop-Process, not pkill, not via any script.
The only exception is a process YOU yourself launched during THIS task, killed by the
exact PID you captured at launch time.
If `cargo build` fails with "Access is denied" on onlyterm-gui.exe, that means a running
OnlyTerm instance holds the file lock: STOP and report it. Do NOT free the lock by killing
anything -- those are the user's live working sessions, and one of them may be hosting the
very agent session you are running in (this has already happened once and destroyed four
multi-day sessions).
```

Формулировки вроде «no killing processes by image name» **недостаточно** — именно эта узкая
формулировка и была обойдена через `/PID`. Запрет должен покрывать все механизмы сразу.

## Прочее

- Не трогать `C:\Users\Computer\.onlyterm.ktav` (конфиг пользователя).
- Проверки: `cargo build -p wezterm-gui`, `cargo clippy -p wezterm-gui --all-targets -- -D warnings`,
  `cargo fmt --check`, `cargo test -p wezterm-gui`.
- Edition 2018: `panic!("...{var}")` без явных аргументов не интерполирует и валит
  `clippy -D warnings` — только `panic!("...{}", var)`.
- Логи OnlyTerm пишутся по одному файлу на PID в `C:\Users\Computer\.local\share\onlyterm\`
  (`onlyterm-gui.exe-log-<pid>.txt`) — это авторитетный источник при разборе падений,
  а не stderr (у GUI-бинаря нет консоли) и не Event Log.
