# Триаж bug-issues апстрима, часть 2 (issues 477-952 из 952, отсортированные по комментариям)

Источник: `docs/upstream-research/issues_chunk_01` (476 строк, issues с 2 и 1 комментарием, затем 0 комментариев).
Проверено применительно к нашему форку: SSH-клиент (wezterm-ssh, libssh/ssh2) и TLS-мультиплексор удалены целиком (только `UnixDomain` с опциональным `proxy_command`/`serve_command` остался — это НЕ ssh-клиент, а просто spawn внешней команды, поэтому баги в proxy_command/unix-domain это применимо к нам). `mux_enable_ssh_agent`/`ssh_agent` модуль остался (форвардинг агента для локальных доменов) — применимо. `harfbuzz_features` остался как имя конфиг-опции (маппится на rustybuzz) — применимо. `freetype`-крейт не используется. Lua(mlua) заменён на rhai — функционально эквивалентные API (wezterm.strftime, format-tab-title, wezterm.time и т.д.) считаются применимыми через rhai-биндинги. WebGpu backend (wgpu) присутствует (`wezterm-gui/src/termwindow/webgpu.rs`) — применимо.

## Важное — крэши/зависания/тормоза/поломанная функциональность (427 штук)

### Issue #5446 (2 коммент.) — panic at startup on wayland
- Суть: панику при старте на Wayland.
- Применимо: да, общий GUI/window-init код.
- Класс: crash
- Приоритет: high — крэш при старте на целой платформенной конфигурации.

### Issue #5443 (2) — window_padding + window_decorations issues
- Суть: взаимодействие padding и decorations ломает геометрию окна.
- Применимо: да, общий рендер окна.
- Класс: visual/functional
- Приоритет: medium

### Issue #5419 (2) — WezTerm nightly does not display default TITLE | RESIZE on Wayland
- Суть: заголовок окна по умолчанию не показывается на Wayland.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5339 (2) — Tabs moved with MoveTabRelative don't persist
- Суть: перестановка вкладок не сохраняется.
- Применимо: да, общий mux/tab код.
- Класс: functional
- Приоритет: medium

### Issue #5319 (2) — `\` input on modifier key
- Суть: неверный ввод `\` при модификаторах.
- Применимо: да, общий keyboard-mapping код.
- Класс: functional
- Приоритет: medium

### Issue #5318 (2) — set_config_overrides overrides active key_table
- Суть: активная key_table сбрасывается при overrides.
- Применимо: да, конфиг-система общая (сейчас rhai).
- Класс: functional
- Приоритет: medium

### Issue #5235 (2) — Shell Integration not working
- Суть: shell integration (OSC 133 и т.п.) не срабатывает.
- Применимо: да, общий код.
- Класс: functional
- Приоритет: medium

### Issue #5233 (2) — Double Width Double Height Pixelated
- Суть: DECDWL/DECDHL рендерится с артефактами пикселизации.
- Применимо: да, наш кастомный рендер-пайплайн (tiny-skia).
- Класс: visual
- Приоритет: medium-high — рендер корректность двойной ширины/высоты.

### Issue #5213 (2) — bad captures of key combinations
- Суть: некорректный захват сочетаний клавиш.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5179 (2) — No keyboard events on Wayland when front_end is WebGpu
- Суть: клавиатурные события не доходят при WebGpu backend на Wayland.
- Применимо: да, WebGpu backend присутствует.
- Класс: functional (эффективно hang для ввода)
- Приоритет: high — полностью ломает ввод в целой конфигурации backend+platform.

### Issue #5165 (2) — Pane resizing not working correctly when using multiplexer
- Суть: неверный ресайз панелей в мультиплексоре.
- Применимо: да (unix-domain mux остался).
- Класс: functional
- Приоритет: medium-high

### Issue #5164 (2) — format-tab-title with use_fancy_tab_bar results in invalid hover detection
- Суть: неверный hover-детект при кастомном заголовке вкладки.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5141 (2) — Jump to semantic regions not working through non-local domains
- Суть: semantic zones навигация не работает через mux-домены.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5137 (2) — tput cols reports wrong number of columns at first
- Суть: неверный размер терминала при первом запросе.
- Применимо: да, core term crate.
- Класс: functional
- Приоритет: medium-high — влияет на приложения, зависящие от размера терминала.

### Issue #5136 (2) — very large icons, iconsize doesn't match with text size
- Суть: несоответствие размера иконок (kitty image?) и текста.
- Применимо: да.
- Класс: visual
- Приоритет: low-medium

### Issue #5107 (2) — portable-pty: clone_killer gives an invalid handle on windows
- Суть: невалидный handle при клонировании "киллера" процесса на Windows.
- Применимо: да, portable-pty общий крейт.
- Класс: functional (может приводить к незавершённым процессам)
- Приоритет: medium-high

### Issue #5043 (2) — Weird bold font rendering
- Суть: странный рендер жирного шрифта.
- Применимо: да, наш шрифтовый стек (rustybuzz/swash).
- Класс: visual
- Приоритет: medium

### Issue #5005 (2) — Window has no shadow when unfocused after minimizing on macOS
- Суть: пропадает тень окна после сворачивания, macOS.
- Применимо: да, но косметика.
- Класс: visual
- Приоритет: low

### Issue #4713 (2) — Doesn't work with/ignores Twemoji?
- Суть: проблемы рендера конкретного эмодзи-шрифта.
- Применимо: да, шрифтовый/эмодзи пайплайн.
- Класс: visual
- Приоритет: medium

### Issue #4686 (2) — Top level split break terminal size
- Суть: top-level split ломает размер терминала.
- Применимо: да, общий split/pane код.
- Класс: functional
- Приоритет: medium-high — некорректный размер терминала, влияет на все приложения внутри.

### Issue #4683 (2) — Wezterm does not preserve tab characters
- Суть: символы табуляции не сохраняются/некорректно обрабатываются.
- Применимо: да, core term crate.
- Класс: functional
- Приоритет: medium-high

### Issue #4644 (2) — Slow (first) startup (not the next ones) ~4s
- Суть: медленный первый старт (~4с).
- Применимо: да.
- Класс: perf
- Приоритет: medium-high

### Issue #4531 (2) — Kitty Image Protocol on tmux
- Суть: kitty image protocol не работает через tmux passthrough.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4525 (2) — Transparency not working on full-screen in windows 11
- Суть: прозрачность отключается в полноэкранном режиме на Win11.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #4417 (2) — gui-sock-$PID socket handling errors
- Суть: ошибки обработки unix-сокета GUI.
- Применимо: да, общий mux-socket код.
- Класс: functional
- Приоритет: medium-high

### Issue #4408 (2) — mux.spawn_window creates multiple tabs if domain is passed
- Суть: лишние вкладки при spawn_window с доменом.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4287 (2) — current_working_dir and foreground_process_name doesn't work with xargs
- Суть: неверный cwd/foreground-process при использовании xargs.
- Применимо: да — процесс-дерево трекается в procinfo/mux (недавно рефакторено в 0894be2c4 — итеративные обходы дерева).
- Класс: functional (edge case)
- Приоритет: medium

### Issue #4240 (2) — [Windows] Incorrect cursor position after printing images
- Суть: неверная позиция курсора после вывода изображений, Windows.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4200 (2) — SwapWithActive doesn't work with unix domains
- Суть: действие SwapWithActive не работает с unix-доменами.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4035 (2) — Links terminate at emoji
- Суть: гиперссылки обрываются на эмодзи.
- Применимо: да, общий url-detection код (termwiz).
- Класс: functional
- Приоритет: medium

### Issue #3976 (2) — Inconsistent positioning of combining diacritical marks
- Суть: неверное позиционирование комбинирующих диакритик.
- Применимо: да, наш шейпинг (rustybuzz).
- Класс: visual
- Приоритет: medium-high

### Issue #3956 (2) — Wezterm goes crazy when entering 125% scale screen
- Суть: поломка при переходе на экран с масштабом 125%.
- Применимо: да, общий DPI-код.
- Класс: visual/functional
- Приоритет: high — DPI-скейлинг частый сценарий (мультимонитор).

### Issue #3917 (2) — Wayland + WebGpu
- Суть: (по телу issue) высокая загрузка CPU под Wayland+nvidia с WebGpu backend.
- Применимо: да, WebGpu присутствует.
- Класс: perf
- Приоритет: high

### Issue #3827 (2) — Wezterm becomes slow, when other wezterm window is minimized
- Суть: замедление при сворачивании другого окна wezterm.
- Применимо: да, общий event loop/render код.
- Класс: perf
- Приоритет: high — влияет на все окна, общий event-loop баг.

### Issue #3673 (2) — alt + space to open menu don't work on full screen mode on windows
- Суть: системное меню не открывается в fullscreen, Windows.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #3662 (2) — Split{Vertical,Horizontal} not take over current directory of active pane in WSL2
- Суть: сплит не наследует cwd активной панели в WSL2.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3616 (2) — Font rendering issue on macos
- Суть: проблемы рендера шрифтов на macOS.
- Применимо: да, шрифтовый стек общий.
- Класс: visual
- Приоритет: medium-high

### Issue #3508 (2) — macOS menubar key shortcuts for shifted keys are tricky to override
- Суть: сложно переопределить системные шорткаты меню на shifted-клавишах.
- Применимо: да, но специфично macOS UX.
- Класс: functional
- Приоритет: low-medium

### Issue #3459 (2) — URLs not recognized across lines in TLS multiplexer sessions
- Применимо: N/A — подсистема удалена (TLS-мультиплексор).

### Issue #3368 (2) — Laggy trailing effect when resizing window
- Суть: "хвостовой" лаг/визуальный шлейф при ресайзе окна.
- Применимо: да, общий рендер при ресайзе.
- Класс: visual/perf
- Приоритет: high — фундаментальный рендер-баг при ресайзе (повторяющаяся тема, см. #2659, #1265, #922 ниже).

### Issue #3223 (2) — Wezterm panic for `tmux -CC a` command via ssh
- Суть: паника в парсере tmux control-mode при подключении через ssh как транспорт (ssh — внешняя команда-шелл, не наш удалённый ssh-клиент).
- Применимо: да — баг в общем tmux-CC-парсере (термвиз/mux), ssh тут просто транспорт до удалённого tmux.
- Класс: crash
- Приоритет: high

### Issue #3172 (2) — default_gui_startup_args is not used when starting with program menu/desktop-file
- Суть: аргументы запуска игнорируются при старте из .desktop-файла.
- Применимо: да, linux-специфично, но реальный баг конфигурации.
- Класс: functional
- Приоритет: low-medium

### Issue #3151 (2) — Emoji in window title is rendered much larger than surrounding text
- Суть: эмодзи в заголовке окна рендерится крупнее текста.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #3114 (2) — TERM_PROGRAM is not set to 'WezTerm' in root shells
- Суть: переменная окружения не проставляется в root-шеллах.
- Применимо: да, но малое влияние.
- Класс: functional
- Приоритет: low

### Issue #2918 (2) — meta.password_input does not detect password input mode in remote machine
- Суть: детект password-режима не работает через удалённые машины (mux).
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #2880 (2) — ScrollToPrompt working inconsistently or not at all with multiplexer
- Суть: ScrollToPrompt нестабилен в мультиплексоре.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #2807 (2) — `libEGL` crash on forwarded X session when starting wezterm
- Суть: крэш EGL при X11-форвардинге.
- Применимо: да, но нишевое окружение (forwarded X).
- Класс: crash
- Приоритет: medium — крэш, но специфичный редкий сценарий транспорта X11.

### Issue #2659 (2) — lines disappear when resizing
- Суть: строки пропадают при ресайзе.
- Применимо: да, общий рендер/reflow.
- Класс: visual/functional
- Приоритет: high — потеря контента при ресайзе, фундаментальный баг.

### Issue #2651 (2) — Scrolling down in Vim scrolls up significant portion of the time
- Суть: скролл в обратную сторону внутри Vim.
- Применимо: да, общая обработка мыши/alt-screen.
- Класс: functional
- Приоритет: high — базовая функциональность скролла в TUI.

### Issue #2595 (2) — Mouse pointer size GNOME setting is ignored by wezterm windows
- Суть: игнорируется системная настройка размера курсора GNOME.
- Применимо: да, косметика linux.
- Класс: visual
- Приоритет: low

### Issue #2456 (2) — [wezterm-ssh] libssh backend hangs and ssh2 backend fails
- Применимо: N/A — подсистема удалена (SSH-клиент).

### Issue #1839 (2) — mux server silently fails when no space is left on device
- Суть: mux-сервер молча падает при заполненном диске.
- Применимо: да, общий mux код.
- Класс: functional (тихий отказ, плохая диагностика)
- Приоритет: medium-high

### Issue #1378 (2) — Horizontal Split vim Ctrl-F Scroll display incorrect blanks/chars
- Суть: артефакты рендера при скролле в сплите.
- Применимо: да.
- Класс: visual
- Приоритет: medium-high

### Issue #1265 (2) — Re-draw issue on resize
- Суть: баг перерисовки при ресайзе.
- Применимо: да, общий рендер.
- Класс: visual
- Приоритет: high — базовый рендер-баг (см. также #2659, #922, #3368, #3033).

### Issue #7906 (1 коммент.) — Large clipboard paste fails under XWayland on KDE Wayland
- Суть: большая вставка из буфера не проходит под XWayland/KDE.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7887 (1) — inconsistent translucency with config.window_frame.*_titlebar_bg
- Суть: непоследовательная прозрачность титульной панели.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7852 (1) — Gap on top of maximized window when window decorations are set to none
- Суть: зазор сверху развёрнутого окна без декораций.
- Применимо: да, общий window-chrome код.
- Класс: visual
- Приоритет: medium

### Issue #7798 (1) — Inconsistent keyboard protocol handling macOS / Windows
- Суть: расхождение обработки kitty keyboard protocol между платформами.
- Применимо: да.
- Класс: functional
- Приоритет: medium-high

### Issue #7788 (1) — Background image issue
- Суть: проблема рендера фонового изображения (детали не раскрыты в заголовке).
- Применимо: да, общий фон-рендер (наш tiny-skia пайплайн).
- Класс: visual
- Приоритет: medium

### Issue #7744 (1) — Tab bar padding is incorrect for many tabs
- Суть: неверные отступы tab bar при большом числе вкладок.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7731 (1) — Regex issue: quick select does not work with Windows path \\ separator
- Суть: quick-select regex не ловит пути с `\`.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7725 (1) — WezTerm crashes on Wayland (Hyprland) running large curl command (wl_display message length exceeds 4096)
- Суть: крэш при большом выводе curl из-за протокольной ошибки Wayland.
- Применимо: да.
- Класс: crash
- Приоритет: high

### Issue #7693 (1) — Failed to decode webp
- Суть: ошибка декодирования webp-изображений.
- Применимо: да, общий image-decoding код.
- Класс: functional
- Приоритет: medium

### Issue #7689 (1) — WezTerm crashing on macOS with EGL errors
- Суть: крэш из-за EGL на macOS.
- Применимо: да.
- Класс: crash
- Приоритет: high

### Issue #7657 (1) — transparent window on launch
- Суть: окно прозрачное сразу после запуска (рендер не готов).
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7551 (1) — Fancy tab bar: left_status obscured by macOS traffic light buttons
- Суть: левый статус перекрывается кнопками светофора macOS.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7537 (1) — cpu hog
- Суть: (заголовок краткий) высокая загрузка CPU.
- Применимо: да, требует уточнения деталей, но категория "perf" сама по себе приоритетна.
- Класс: perf
- Приоритет: high

### Issue #7461 (1) — Openconsole 1.22.2502 is bugged
- Суть: баг в OpenConsole (conpty), от которого зависит Windows-бэкенд pty.
- Применимо: да, влияет на работу portable-pty на Windows.
- Класс: functional
- Приоритет: medium

### Issue #7423 (1) — AttachDomain / wezterm cli spawn --domain-name requires passphrase for identityfile
- Применимо: N/A — подсистема удалена (SSH identity file/passphrase — ssh-клиент).

### Issue #7413 (1) — default workspace name a bit too aggressive
- Суть: авто-именование workspace слишком навязчиво.
- Применимо: да.
- Класс: functional
- Приоритет: low

### Issue #7409 (1) — links are highlighted when the mouse pointer is in a different pane
- Суть: подсветка ссылок в неактивной панели при наведении в другой.
- Применимо: да, общий hover/highlight код.
- Класс: visual/functional
- Приоритет: medium

### Issue #7365 (1) — edit in Windows wezterm, the character "555" will be automatically inserted at the beginning of the file
- Суть: самопроизвольная вставка символов "555" при редактировании, Windows.
- Применимо: да, похоже на баг ввода/pty echo.
- Класс: functional (порча содержимого файла)
- Приоритет: high

### Issue #7355 (1) — Unable to Render Hindi Font
- Суть: не рендерится хинди (сложный шейпинг).
- Применимо: да, наш шейпинг-пайплайн (rustybuzz для complex scripts).
- Класс: visual
- Приоритет: medium-high

### Issue #7315 (1) — glow markdown rendering got broken when you scroll up/down in Wezterm
- Суть: рендер ломается при скролле (взаимодействие с TUI-приложением glow).
- Применимо: да, общий scrollback/redraw код.
- Класс: visual
- Приоритет: medium

### Issue #7281 (1) — Scaling issue when moving windows between monitors with different DPI on Windows with GlazeWM
- Суть: баг масштабирования при переносе между мониторами разного DPI.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7265 (1) — segfault w/macos Sequoia
- Суть: сегфолт на macOS Sequoia.
- Применимо: да.
- Класс: crash
- Приоритет: high

### Issue #7263 (1) — ghost chars when using OpenSSH-Win64 from Windows 10 to Linux and then tmux
- Суть: "призрачные" символы через OpenSSH (внешний бинарник) + tmux.
- Применимо: да — транспорт внешний ssh.exe, баг в рендере/tmux-CC парсинге.
- Класс: visual
- Приоритет: medium-high

### Issue #7244 (1) — macOS: Ctrl-R and Ctrl-A not working despite configuration attempts
- Суть: не работают базовые readline-сочетания на macOS.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7237 (1) — Wezterm search blinks when used with tmux
- Суть: мигание поискового оверлея при tmux control mode.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7173 (1) — PromptInputLine cannot use InputMethod
- Суть: встроенный prompt-оверлей не работает с IME.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7167 (1) — ResetAttributes not working as expected in fancy tab bar
- Суть: сброс атрибутов не работает в fancy tab bar.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7078 (1) — Win+V paste adds unwanted characters (null + newline)
- Суть: вставка из истории буфера Windows добавляет мусорные символы.
- Применимо: да, частый пользовательский сценарий на Windows.
- Класс: functional
- Приоритет: high

### Issue #7058 (1) — Broken render pipe on fraction scaling in wayland
- Суть: сломан рендер при дробном масштабировании на Wayland.
- Применимо: да.
- Класс: visual
- Приоритет: medium-high

### Issue #7055 (1) — Mouse scroll wheel switches tabs after clicking GNOME top bar instead of scrolling scrollback
- Суть: фокус/скролл конфликт после клика по системной панели GNOME.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7050 (1) — Crash/Unresponsive when interacting with Albert
- Суть: крэш/зависание при взаимодействии с лаунчером Albert (X11/linux).
- Применимо: да.
- Класс: crash/hang
- Приоритет: medium-high

### Issue #7026 (1) — The UTF-8 "small" character variants render with an extra monospace unnecessarily
- Суть: неверный расчёт ширины ячейки для некоторых unicode-вариантов.
- Применимо: да, core рендер ширины символов.
- Класс: visual
- Приоритет: medium-high

### Issue #6976 (1) — `ClearSelection` not working
- Суть: действие ClearSelection не работает.
- Применимо: да, общий selection-код.
- Класс: functional
- Приоритет: medium-high

### Issue #6967 (1) — Cursor flickering in Neovim embedded terminal on Windows
- Суть: мерцание курсора в embedded-терминале Neovim, Windows.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #6947 (1) — entering unix domain on startup is not working
- Суть: автоподключение unix-домена при старте не срабатывает.
- Применимо: да.
- Класс: functional
- Приоритет: medium-high

### Issue #6900 (1) — (win11) kitty keyboard inputs invalid shift+keys
- Суть: неверные shift-комбинации в kitty keyboard protocol, Win11.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6846 (1) — Neovim cannot receive "Ctrl Alt h" keybinding in zellij when using Wezterm
- Суть: клавиша не доходит через zellij внутри wezterm.
- Применимо: да, но нишевая комбинация мультиплексоров.
- Класс: functional
- Приоритет: low-medium

### Issue #6818 (1) — 15% CPU usage on Wezterm vs 1% on Windows Terminal when holding down a key
- Суть: сильно повышенная загрузка CPU при удержании клавиши (key-repeat).
- Применимо: да, общий цикл обработки ввода.
- Класс: perf
- Приоритет: high

### Issue #6733 (1) — Double-click select text; if last character is Chinese, only half is selected
- Суть: неверная граница слова при double-click на CJK-символе.
- Применимо: да, общий word-boundary код.
- Класс: functional
- Приоритет: medium-high

### Issue #6732 (1) — WezTerm fails to launch on Windows after OpenConsole.exe update
- Суть: регрессия запуска после обновления conpty.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6720 (1) — Paste selection stopped working after update to plasma 6.3
- Суть: регрессия paste после обновления KDE Plasma.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6716 (1) — WSL Ubuntu interprets caps as ^@
- Суть: неверная интерпретация Caps Lock в WSL.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6674 (1) — maximize on gui-startup doesn't work when using unix domain
- Суть: gui-startup maximize не работает с unix-доменом.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6669 (1) — Cursor position incorrect after DECSC, resize with reflow, then DECRC
- Суть: некорректная позиция курсора после save/resize-reflow/restore.
- Применимо: да, core term state (VT100 корректность).
- Класс: functional
- Приоритет: high

### Issue #6635 (1) — Terminal blinks sometimes during command execution
- Суть: мигание терминала во время выполнения команды.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #6561 (1) — Doesn't support kitty images with trailing comma in control data
- Суть: парсер kitty image protocol не понимает trailing comma.
- Применимо: да, общий парсер протокола.
- Класс: functional
- Приоритет: medium

### Issue #6560 (1) — OSC Shell Integration not working on Windows11 + Oh-my-posh OSC 133
- Суть: OSC 133 shell integration не работает с oh-my-posh на Win11.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6547 (1) — [AppImage] Error on startup - mux::ssh_agent failed to create symlink
- Суть: ошибка при создании симлинка ssh-agent сокета при старте.
- Применимо: да — `mux_enable_ssh_agent`/ssh_agent форвардинг остался в форке (это не наш удалённый ssh-клиент, а форвардинг агента для доменов).
- Класс: functional
- Приоритет: medium-high

### Issue #6531 (1) — Windows 11 Wezterm crashes on start
- Суть: крэш при старте на Win11.
- Применимо: да.
- Класс: crash
- Приоритет: high

### Issue #6522 (1) — Can't use 'Next' and 'Prev' with ActivatePaneDirection
- Суть: направления Next/Prev не работают в ActivatePaneDirection.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6480 (1) — wezterm crashes when unplugging external monitor
- Суть: крэш при отключении внешнего монитора.
- Применимо: да.
- Класс: crash
- Приоритет: high

### Issue #6458 (1) — Invalid email handling when it contains period
- Суть: regex распознавания email не учитывает точку.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #6453 (1) — Split pane divider overlaps top-positioned normal (non-fancy) tab bar
- Суть: разделитель панелей перекрывает tab bar сверху.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #6452 (1) — Can't connect to remote multiplexer
- Суть: не удаётся подключиться к удалённому mux-серверу (client/server версии разные).
- Применимо: неопределённо — зависит от типа домена (unix+proxy_command применимо, встроенный ssh-домен — N/A). Возможно частично N/A.
- Класс: functional
- Приоритет: medium

### Issue #6446 (1) — PageUp and PageDown, not sent to terminal editors
- Суть: PageUp/PageDown не доходят до приложения.
- Применимо: да, базовая функциональность клавиатуры.
- Класс: functional
- Приоритет: high

### Issue #6434 (1) — Kitty Keyboard input with flags set to 0 has unexpected encodings
- Суть: неверное кодирование при flags=0 в kitty keyboard protocol.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6381 (1) — Wezterm not picking up config in linked config folder on Windows
- Суть: конфиг не подхватывается из симлинк-папки на Windows.
- Применимо: да, общая загрузка конфига.
- Класс: functional
- Приоритет: medium

### Issue #6353 (1) — Wezterm window jumps out
- Суть: (из тела) окно "выпрыгивает" по диагонали при клике вне окна, Wayland.
- Применимо: да.
- Класс: visual/functional
- Приоритет: medium

### Issue #6321 (1) — Trailing equal sign '=' not detected as part of URL
- Суть: regex URL не включает завершающий `=`.
- Применимо: да.
- Класс: functional
- Приоритет: low

### Issue #6305 (1) — wezterm connect fails with timeout
- Суть: таймаут при `wezterm connect`.
- Применимо: да (если домен unix+proxy_command), иначе возможен N/A для ssh-доменов.
- Класс: functional
- Приоритет: medium

### Issue #6282 (1) — Setting initial_rows > 24 maxes window
- Суть: initial_rows > 24 разворачивает окно на весь экран.
- Применимо: да, общая логика размера окна.
- Класс: functional
- Приоритет: medium

### Issue #6261 (1) — GPOS-modified glyphs may be being cached inappropriately
- Суть: некорректное кэширование глифов после GPOS-модификаций.
- Применимо: да, наш шейпинг/кэш глифов.
- Класс: visual
- Приоритет: medium-high

### Issue #6254 (1) — Wezterm slow startup time and noticeable resize after window was spawned
- Суть: медленный старт + заметный ресайз после появления окна.
- Применимо: да.
- Класс: perf
- Приоритет: medium-high

### Issue #6198 (1) — close confirmation does not work for single tab pane running tmux
- Суть: подтверждение закрытия не работает при tmux в единственной вкладке.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6197 (1) — Long startup times, weird messages in log
- Суть: долгий старт, странные сообщения в логе.
- Применимо: да.
- Класс: perf
- Приоритет: medium

### Issue #6160 (1) — Kitty keyboard: ctrl+space release produce wrong sequence
- Суть: неверная последовательность на release ctrl+space.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6144 (1) — Newly created workspaces don't appear in GUI workspace selection list on macOS
- Суть: новые workspace не отображаются в списке выбора, macOS.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6123 (1) — Search deselects when following logs if a new match occurs
- Суть: поиск сбрасывает выделение при появлении нового совпадения (follow-логи).
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6105 (1) — `--config window_decorations="RESIZE"` do not make effect
- Суть: CLI-override декораций окна не действует.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6089 (1) — Vertical pane separator bleeds into the tab bar
- Суть: вертикальный разделитель панелей залезает в tab bar.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #6056 (1) — Splitting pane/creating tab for currentDomain inside WSL defaults to / instead of current directory
- Суть: неверный cwd по умолчанию в WSL при сплите/новой вкладке.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6047 (1) — Configuring font_size causes new windows to spawn with large number of rows and columns
- Суть: при изменении font_size новые окна получают неверный размер в строках/колонках.
- Применимо: да, расчёт размера окна по метрикам шрифта (наш шрифтовый стек).
- Класс: functional
- Приоритет: medium-high

### Issue #6024 (1) — Tab title not refreshed at toggling debug overlay
- Суть: заголовок вкладки не обновляется при открытии debug-оверлея.
- Применимо: да.
- Класс: visual
- Приоритет: low

### Issue #6019 (1) — Changing argument to wezterm.action.Search has no effect when argument is an empty string
- Суть: пустая строка в аргументе Search игнорируется.
- Применимо: да, через rhai-эквивалент action API.
- Класс: functional
- Приоритет: low-medium

### Issue #6001 (1) — On windows10, using SHIFT in modifiers behaves unexpectedly, different from macOS
- Суть: несогласованное поведение SHIFT между Windows и macOS.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5939 (1) — Rendering complex status on 'update-status' causing wezterm to feel slow and appear choppy
- Суть: тяжёлый рендер статус-бара вызывает подтормаживание.
- Применимо: да, общий статус-бар рендер.
- Класс: perf
- Приоритет: medium-high

### Issue #5906 (1) — run_child_process doesn't respect current shell environment variables
- Суть: спавн процесса игнорирует текущие переменные окружения шелла.
- Применимо: да, общий процесс-спавн (portable-pty/procinfo).
- Класс: functional
- Приоритет: medium

### Issue #5852 (1) — Sometimes invisible since start
- Суть: (из тела) окно иногда полностью невидимо с самого старта (кроме декораций WM), Wayland/River.
- Применимо: да, общий window-init/render код.
- Класс: visual/functional
- Приоритет: medium-high

### Issue #5823 (1) — Keyboard layout switches made before wezterm is opened isn't registered, sometimes
- Суть: смена раскладки клавиатуры до запуска не подхватывается.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5797 (1) — keypad activation is inverted, and takes double "NumLock" input to get back num
- Суть: инвертированная активация numpad, двойной NumLock.
- Применимо: да, общая обработка клавиатуры.
- Класс: functional
- Приоритет: medium

### Issue #5764 (1) — Opening new terminal with no currently open windows
- Суть:边ge-случай открытия нового терминала когда нет открытых окон.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5749 (1) — higher order function keys (F13-F24) don't work
- Суть: F13-F24 не работают.
- Применимо: да, общее кодирование клавиш.
- Класс: functional
- Приоритет: medium

### Issue #5508 (1) — wrong up arrow icon alignment
- Суть: неверное выравнивание иконки стрелки вверх (scrollbar/UI).
- Применимо: да, косметика.
- Класс: visual
- Приоритет: low

### Issue #5485 (1) — Extreme lag and/or forever buffered input. split panes Xorg
- Суть: экстремальный лаг/навсегда буферизованный ввод в сплитах, Xorg.
- Применимо: да.
- Класс: hang/perf
- Приоритет: high

### Issue #5471 (1) — Changing font size while app outputs to terminal causes wezterm to misinterpret new lines
- Суть: смена font_size во время активного вывода ломает интерпретацию новых строк (reflow race).
- Применимо: да, core reflow-логика.
- Класс: functional
- Приоритет: high

### Issue #5420 (1) — window config override for harfbuzz_features doesn't work if font already specifies them
- Суть: override harfbuzz_features не применяется, если шрифт уже задаёт свои фичи.
- Применимо: да — опция сохранена в конфиге (маппится на rustybuzz).
- Класс: functional
- Приоритет: medium

### Issue #5309 (1) — First left-click after FocusChanged(true) triggers redundant MouseEvent Move, losing tmux clipboard content
- Суть: лишнее событие Move после фокуса приводит к потере содержимого буфера tmux.
- Применимо: да, общая обработка фокуса/мыши.
- Класс: functional (потеря данных)
- Приоритет: medium-high

### Issue #5219 (1) — tab_bar_style not applied
- Суть: конфиг tab_bar_style не применяется.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5210 (1) — unexpected and unstable rendering of bg w/ tiled gradients
- Суть: нестабильный рендер фона с tiled-градиентами.
- Применимо: да, наш фон-рендер (tiny-skia).
- Класс: visual
- Приоритет: medium-high

### Issue #5201 (1) — bug: CTRL+SHIFT+h does not get correctly encoded
- Суть: неверное кодирование Ctrl+Shift+H.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5181 (1) — input delay after installing nvidia Vulkan Driver
- Суть: задержка ввода после установки Vulkan-драйвера nvidia.
- Применимо: да, наш GPU render backend (wgpu).
- Класс: perf
- Приоритет: medium-high

### Issue #5147 (1) — Kitty keyboard: keys not reported as escape keys
- Суть: клавиши не репортятся как escape-последовательности в kitty protocol.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5126 (1) — Unable to Interact with Background Windows in Fullscreen Mode
- Суть: невозможно взаимодействовать с фоновыми окнами в fullscreen.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5019 (1) — Inconsistent window movement across different operating systems with integrated tab bar enabled
- Суть: несогласованное перемещение окна между ОС с integrated tab bar.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4984 (1) — SplitPane using top_level=true leaves behind ghost space
- Суть: "призрачное" пустое пространство после top-level сплита.
- Применимо: да.
- Класс: visual/functional
- Приоритет: medium-high

### Issue #4972 (1) — Select to copy broken on nightly
- Суть: выделение-с-копированием сломано (регрессия).
- Применимо: да, core selection код.
- Класс: functional
- Приоритет: high — базовая фича сломана.

### Issue #4922 (1) — mouse scroll in alternate mode not working under multiplexing
- Суть: скролл мышью в alt-screen не работает под mux.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4901 (1) — Multiplexing "connect_automatically" open two windows
- Суть: дублирование окон при автоподключении к mux.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4783 (1) — Undercurl not working - different behavior in shell and tmux/neovim
- Суть: рендер undercurl отличается/не работает между шеллом и tmux/neovim.
- Применимо: да, наш рендер декораций текста.
- Класс: visual
- Приоритет: medium-high

### Issue #4664 (1) — Flicker on M1 Mac
- Суть: мерцание рендера на Apple Silicon.
- Применимо: да.
- Класс: visual
- Приоритет: medium-high

### Issue #4556 (1) — UI teleports to (x/2, y/2) upon dragging tab bar/with SUPER on MACOS
- Суть: окно "телепортируется" при перетаскивании tab bar, macOS.
- Применимо: да.
- Класс: functional/visual
- Приоритет: medium

### Issue #4536 (1) — CTRL-click does not work for opening links when bypass_mouse_reporting_modifiers is also CTRL
- Суть: конфликт конфигурации модификаторов для открытия ссылок.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4522 (1) — Selecting topmost line drags the window instead
- Суть: выделение верхней строки вместо этого перетаскивает окно.
- Применимо: да, конфликт hit-test drag-area/selection.
- Класс: functional
- Приоритет: medium-high

### Issue #4440 (1) — wezterm cli set-tab-title is not working correctly within tmux session
- Суть: CLI set-tab-title некорректно работает под tmux.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4405 (1) — Unable to bind cmd+shift+/ on macOS
- Суть: невозможно забиндить конкретное сочетание, macOS.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #4264 (1) — Maximizing on Windows with duplicated monitor projection: constantly swapping font size/redrawing
- Суть: зацикленная перерисовка/смена размера шрифта при дублировании экрана, Windows.
- Применимо: да.
- Класс: perf/visual
- Приоритет: medium-high

### Issue #4255 (1) — Mangled text in Helix when unicode is involved
- Суть: искажение текста в Helix при unicode.
- Применимо: да, core unicode/рендер.
- Класс: visual
- Приоритет: high

### Issue #4229 (1) — pane:get_semantic_zones is empty in wezterm connect sessions
- Суть: semantic zones API пустое в mux-сессиях.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4199 (1) — Closing pane/window always asks for confirmation with unix domains
- Суть: подтверждение закрытия всегда всплывает для unix-доменов.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4076 (1) — In Windows, two sets of buttons when System Backdrop and Tab at Bottom are both on
- Суть: дублирование кнопок управления окном, Windows.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #3994 (1) — Quickly switching tabs in a wezterm-mux-server causes an uncontrolled tab switching frenzy
- Суть: неконтролируемое переключение вкладок (обратная связь событий) при быстром переключении на mux-сервере.
- Применимо: да.
- Класс: functional
- Приоритет: medium-high

### Issue #3763 (1) — Toggling window decorations off expands beyond initial window size
- Суть: отключение декораций увеличивает размер окна сверх исходного.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3671 (1) — Wrong split pane dimensions after window:gui_window:maximize()
- Суть: неверные размеры панелей после программного maximize().
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3625 (1) — Color and font rendering differences between WebGpu and OpenGL front_end
- Суть: различия рендера цвета/шрифта между backend'ами.
- Применимо: да, оба backend'а (WebGpu/OpenGL) присутствуют.
- Класс: visual
- Приоритет: medium-high

### Issue #3494 (1) — Triple click doesn't select long lines
- Суть: тройной клик не выделяет длинные строки полностью.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3302 (1) — Clipboard operations are asynchronous, wrt key assignment action dispatch
- Суть: гонка между асинхронным clipboard и диспетчеризацией действий.
- Применимо: да, потенциальная гонка данных.
- Класс: functional (race)
- Приоритет: medium-high

### Issue #3224 (1) — Unhelpful scrolling behavior on resize
- Суть: неудобное поведение скролла при ресайзе.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3171 (1) — bash PROMPT_COMMAND syntax error
- Суть: синтаксическая ошибка в скрипте shell integration для bash.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3149 (1) — force_reverse_video_cursor doesn't work with cursor_border (unfocused cursor)
- Суть: конфликт опций рендера курсора без фокуса.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #3097 (1) — Invalid cursor_thickness for SteadyBar cursor
- Суть: неверная толщина курсора SteadyBar.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #3033 (1) — Resizing window while ping is running causes content to be written over itself
- Суть: содержимое терминала затирается само собой при ресайзе во время активного вывода.
- Применимо: да, core рендер/reflow при ресайзе.
- Класс: visual/functional
- Приоритет: high

### Issue #2693 (1) — Can't Quick Select the very last line
- Суть: quick select не работает на последней строке.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #2543 (1) — termwiz layouts
- Суть: (из тела) баг в layout виджетов termwiz (widgets_nested).
- Применимо: да, termwiz — общий разделяемый крейт.
- Класс: visual/functional
- Приоритет: medium

### Issue #2446 (1) — Titlebar focus colors won't change
- Суть: цвета титульной панели не меняются при смене фокуса.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #922 (1) — Resizing makes rendered contents jitter
- Суть: дрожание отрендеренного содержимого при ресайзе.
- Применимо: да, повторяющаяся тема фундаментальных рендер-багов при ресайзе.
- Класс: visual
- Приоритет: high

### Issue #858 (1) — Idle mux connections never recover
- Суть: простаивающие mux-соединения никогда не восстанавливаются (эффективно зависание).
- Применимо: да, общий mux reconnect-код.
- Класс: hang
- Приоритет: high

### Issue #7969 (0 коммент.) — loop error during event_g.dispatch... Protocol error (os error 71)
- Суть: (из тела) краш/выход из-за протокольной ошибки Wayland (mutter/GNOME).
- Применимо: да.
- Класс: crash
- Приоритет: high

### Issue #7963 (0) — Windows: panic in wezterm-font load_fallback when a fallback font resolves to ACL-protected WindowsApps font
- Суть: паника при недоступном (ACL) fallback-шрифте.
- Применимо: да — УЖЕ ИСПРАВЛЕНО в нашем форке коммитом `5752050b8` ("wezterm-font: skip unreadable fallback font candidates instead of erroring out", `fixes #7963`).
- Класс: crash (исправлено)
- Приоритет: high (уже закрыто у нас)

### Issue #7959 (0) — Unix-domain proxy_command via wsl.exe hangs at "Checking server version" when spawned by the GUI
- Суть: зависание при использовании proxy_command=wsl.exe, если mux запущен из GUI (а не из консоли).
- Применимо: да — proxy_command механизм UnixDomain сохранён в форке.
- Класс: hang
- Приоритет: high

### Issue #7949 (0) — crash when maximizing/minimizing window on latest Windows
- Суть: крэш при максимизации/минимизации, Windows.
- Применимо: да.
- Класс: crash
- Приоритет: high

### Issue #7921 (0) — ~ is pasted as control character 0x1E in WezTerm on Windows with WSL
- Суть: `~` вставляется как control-символ 0x1E, Windows+WSL.
- Применимо: да, порча вставляемого текста.
- Класс: functional
- Приоритет: medium-high

### Issue #7902 (0) — start --cwd no longer works
- Суть: регрессия CLI-флага --cwd.
- Применимо: да.
- Класс: functional
- Приоритет: medium-high

### Issue #7895 (0) — SpawnCommandInNewTab drops pending data
- Суть: потеря pending-данных при спавне команды в новой вкладке.
- Применимо: да, потеря данных.
- Класс: functional
- Приоритет: high

### Issue #7873 (0) — send-text fail for psmux
- Суть: CLI send-text не работает для мультиплексной панели.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7860 (0) — Command palette DPI issue on wayland
- Суть: неверный DPI командной палитры на Wayland.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7835 (0) — Hyperlink hover underline can be incorrect for identical hyperlinks
- Суть: неверное подчёркивание hover для одинаковых ссылок.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7830 (0) — use_fancy_tab_bar with three-finger-drag some error
- Суть: ошибка при трёхпальцевом жесте с fancy tab bar.
- Применимо: да, но нишевый жест трекпада.
- Класс: functional
- Приоритет: low-medium

### Issue #7819 (0) — WebGpu backend panics in Surface::configure
- Суть: паника WebGpu backend при конфигурации поверхности.
- Применимо: да, WebGpu backend присутствует.
- Класс: crash
- Приоритет: high

### Issue #7765 (0) — mux::tab::adjust_y_size dead loop on macOS
- Суть: бесконечный цикл в adjust_y_size (mux, общий код).
- Применимо: да — общая mux-логика, вероятно воспроизводима не только на macOS.
- Класс: hang
- Приоритет: high

### Issue #7742 (0) — portable-pty aborts on exec failure instead of returning a spawn error
- Суть: abort() вместо возврата ошибки при неудачном exec.
- Применимо: да, portable-pty общий крейт (используется везде).
- Класс: crash
- Приоритет: high

### Issue #7729 (0) — Regex issue: A-f ≠ A-Fa-f (quick_select_patterns)
- Суть: некорректный regex-класс символов в quick select.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #7724 (0) — Slow windows rendering on startup - Windows 11 25H2 with AMD Radeon RX7600
- Суть: медленный старт рендера на новых Windows+AMD.
- Применимо: да.
- Класс: perf
- Приоритет: medium

### Issue #7702 (0) — Retro tab bar does not span the whole width
- Суть: retro-стиль tab bar не растягивается на всю ширину.
- Применимо: да.
- Класс: visual
- Приоритет: low-medium

### Issue #7695 (0) — OSC 12 cursor color changes ignored after IME mode switch on macOS
- Суть: OSC 12 игнорируется после переключения IME, macOS.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7665 (0) — Horizontal scroll wheel (buttons 6/7) not recognized as WheelLeft/WheelRight on X11
- Суть: горизонтальный скролл не распознаётся на X11.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7660 (0) — Windows: Cannot spawn new admin instance when non-admin instance is running
- Суть: не удаётся запустить admin-инстанс поверх non-admin.
- Применимо: да, но нишевый сценарий.
- Класс: functional
- Приоритет: low-medium

### Issue #7645 (0) — Scrolling inside TUIs (tmux, vim, emacs) is sluggish with precision pointing devices (trackpad, Magic Mouse)
- Суть: подтормаживание скролла с точными указателями (трекпад/Magic Mouse) внутри TUI.
- Применимо: да, общая обработка событий скролла.
- Класс: perf
- Приоритет: high

### Issue #7631 (0) — Sending Kitty images via shared memory does not work on macOS
- Суть: shared-memory путь kitty image protocol не работает на macOS.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7630 (0) — Prompt scrolls down to the bottom whenever the window is resized on hyprland
- Суть: позиция скролла сбрасывается на низ при ресайзе.
- Применимо: да, общая логика resize+scroll position.
- Класс: functional
- Приоритет: medium-high

### Issue #7611 (0) — WezTerm engages adaptive sync
- Суть: (из тела) WezTerm включает адаптивную синхронизацию (G-Sync) даже не в fullscreen, снижая частоту обновления монитора.
- Применимо: да, наш GPU render backend (wgpu/frame pacing).
- Класс: perf/visual
- Приоритет: medium

### Issue #7600 (0) — window_decoration to NONE displays native title bar on COSMIC desktop
- Суть: NONE-декорации не убирают нативный титлбар на COSMIC DE.
- Применимо: да, но новое нишевое DE.
- Класс: visual
- Приоритет: low-medium

### Issue #7582 (0) — Flag --attach ignored when delegating to existing GUI instance
- Суть: флаг --attach игнорируется при делегировании существующему инстансу.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7573 (0) — toast_notification has no reliable timeout support
- Суть: ненадёжный таймаут toast-уведомлений.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #7570 (0) — Wrongly rendering ghost character when two or more wide characters are followed by a Unicode Variation Selector
- Суть: артефакт рендера при wide-char + variation selector.
- Применимо: да, core рендер ширины/unicode.
- Класс: visual
- Приоритет: high

### Issue #7560 (0) — Wezterm doesn't enter fullscreen when opened from another fullscreen app on macOS
- Суть: не входит в fullscreen при запуске поверх другого fullscreen-приложения.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #7538 (0) — input method selection menu is hidden after full screen
- Суть: меню выбора IME скрыто в fullscreen.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7531 (0) — Claude text entry freezes / no scrollbar
- Суть: зависание при вводе текста и отсутствие скроллбара при активном использовании (тяжёлый вывод/большой скроллбэк).
- Применимо: да, вероятно общий рендер/perf-баг при большом объёме вывода.
- Класс: hang/perf
- Приоритет: high

### Issue #7528 (0) — front_end, OpenGL and Software doesn't render anything after upgrading to AMD driver 26.1.1
- Суть: полностью пропадает рендер после обновления AMD-драйвера (все backend'ы).
- Применимо: да.
- Класс: crash/functional
- Приоритет: high

### Issue #7523 (0) — emoji don't render correctly (size issues, presentation flickering, width encroaching)
- Суть: множественные проблемы рендера эмодзи (размер/мерцание/ширина).
- Применимо: да, наш эмодзи/шрифтовый пайплайн.
- Класс: visual
- Приоритет: medium-high

### Issue #7519 (0) — Windows sleep/resume causes wezterm-gui.exe crash in d3d11.dll (APPCRASH)
- Суть: крэш в d3d11 при выходе из сна, Windows.
- Применимо: да, наш GPU backend (wgpu/d3d11).
- Класс: crash
- Приоритет: high

### Issue #7511 (0) — On Windows, impossible to set startup position with `start --position <monitor>:<x>,<y>` (monitor names contain colon)
- Суть: парсинг CLI-аргумента ломается, т.к. имена мониторов содержат `:`.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7507 (0) — Segfault on macOS Sequoia 15.7.3 (24G419)
- Суть: сегфолт на конкретной сборке macOS.
- Применимо: да.
- Класс: crash
- Приоритет: high

### Issue #7498 (0) — Copy to clipboard problem from inside distrobox
- Суть: проблема clipboard внутри distrobox-контейнера.
- Применимо: да, но нишевое контейнерное окружение linux.
- Класс: functional
- Приоритет: low-medium

### Issue #7495 (0) — Non numpad - and / key not bindable
- Суть: не-numpad `-`/`/` не биндятся.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7473 (0) — Gureum Korean input method works weirdly with arrow keys
- Суть: странности корейского IME Gureum со стрелками.
- Применимо: да, но нишевый IME.
- Класс: functional
- Приоритет: low-medium

### Issue #7465 (0) — Flicker when using CSI 2026 synchronization
- Суть: мерцание при использовании synchronized-update escape-последовательности.
- Применимо: да, core рендер/sync-update.
- Класс: visual
- Приоритет: medium-high

### Issue #7462 (0) — custom "Medium shade" block glyphs very dark
- Суть: неверная яркость блочных глифов "medium shade".
- Применимо: да, рендер глифов.
- Класс: visual
- Приоритет: medium

### Issue #7456 (0) — [macOS] Window dragging stutters on tab bar empty area when format-tab-title callback is registered
- Суть: подвисания при перетаскивании окна, если зарегистрирован rhai-колбэк format-tab-title.
- Применимо: да, через rhai-эквивалент callback API.
- Класс: perf
- Приоритет: medium-high

### Issue #7416 (0) — VTParser::action has a lot of self cycles and dominates on kitten __benchmark__
- Суть: VTParser::action — горячая точка производительности (доминирует в бенчмарке).
- Применимо: да, core парсер (используется всегда).
- Класс: perf
- Приоритет: high

### Issue #7415 (0) — Unable to load a font specified by wezterm.font(...) with weight/stretch/style args
- Суть: не удаётся загрузить шрифт по конкретным атрибутам через конфиг-API.
- Применимо: да, наш font loader/matcher.
- Класс: functional
- Приоритет: medium-high

### Issue #7406 (0) — Consistent crash when dragging image from firefox over terminal
- Суть: стабильный крэш при drag-and-drop изображения из Firefox.
- Применимо: да, общий drag&drop код.
- Класс: crash
- Приоритет: high

### Issue #7401 (0) — AdjustPaneSize does not work on the 1st vertical split after further vertical splits
- Суть: некорректный AdjustPaneSize после вложенных вертикальных сплитов.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7400 (0) — Memory leak when using kitty + gif
- Суть: утечка памяти при использовании kitty image protocol с gif.
- Применимо: да, общий image-protocol код.
- Класс: perf (memory leak)
- Приоритет: high

### Issue #7368 (0) — cli is not properly executing commands in order
- Суть: CLI-команды выполняются не в том порядке.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7344 (0) — SIXEL: omitted color parameters between semicolons are not treated as 0 but as 100
- Суть: неверный парсинг пропущенных параметров цвета в SIXEL.
- Применимо: да, core SIXEL-парсер.
- Класс: functional
- Приоритет: medium-high

### Issue #7331 (0) — Resizing of remote (mux-server) sessions very sluggish
- Суть: очень медленный ресайз в mux-сессиях.
- Применимо: да.
- Класс: perf
- Приоритет: high

### Issue #7316 (0) — promptline generated prompt oddly displayed
- Суть: странное отображение промпта конкретного генератора (promptline).
- Применимо: да, но зависит от специфики стороннего инструмента; вероятно общий рендер prompt/escape-обработки.
- Класс: visual
- Приоритет: medium

### Issue #7311 (0) — Symlinks should not be resolved (multiple places)
- Суть: symlink-пути неверно резолвятся в realpath в нескольких местах кода (cwd, --cwd и т.д.).
- Применимо: да, общая логика работы с путями.
- Класс: functional
- Приоритет: medium-high

### Issue #7309 (0) — OSX Tahoe, claude code loop/lockup/stuttering for nightly build
- Суть: зависание/подвисание при интенсивном использовании (Claude Code), macOS.
- Применимо: да, вероятно общий perf/render-баг при большом потоке вывода.
- Класс: hang/perf
- Приоритет: high

### Issue #7301 (0) — Unable to input Japanese with Mozc on Ubuntu 22.04 (apt version)
- Суть: не работает японский ввод через Mozc.
- Применимо: да, но нишевый IME/дистрибутив.
- Класс: functional
- Приоритет: medium

### Issue #7300 (0) — skip_close_confirmation_for_processes_named does not work in overrides
- Суть: опция не работает при передаче через overrides.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7289 (0) — Status bars do not respect any of the "Attribute" of a wezterm.format({})
- Суть: атрибуты форматирования игнорируются в статус-баре.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7261 (0) — Privilege escalation - send-text --no-paste
- Суть: потенциальная эскалация привилегий через CLI send-text --no-paste.
- Применимо: да — требует проверки, но заявлена как security issue.
- Класс: security/functional
- Приоритет: high — заявленная уязвимость, приоритет по умолчанию высокий вне зависимости от числа комментариев.

### Issue #7253 (0) — [Windows BUG] scaling the terminal causes the long white block at the bottom to flicker
- Суть: мерцающий белый блок снизу при масштабировании, Windows.
- Применимо: да.
- Класс: visual
- Приоритет: medium-high

### Issue #7235 (0) — Pasting issues (in nightly but not older build) over 1024 text, interacting with Claude Code poorly
- Суть: регрессия вставки больших объёмов текста (>1024 симв.).
- Применимо: да, общий paste-код.
- Класс: functional
- Приоритет: high

### Issue #7234 (0) — macOS: Ctrl+H deletes confirmed characters instead of pre-edit text when using Japanese IME
- Суть: неверная обработка Ctrl+H при композиции японского IME.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7222 (0) — kitty image not blending with text
- Суть: изображение kitty protocol не блендится с текстом поверх него.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7220 (0) — Inaccuracies in the hovering state of the tabs wrt the mouse pointer position
- Суть: неточный hover-детект вкладок.
- Применимо: да.
- Класс: visual
- Приоритет: low-medium

### Issue #7198 (0) — Underline and Italic attributes are not rendered on status bar
- Суть: атрибуты underline/italic не рендерятся в статус-баре.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7168 (0) — OSC 133 move to previous/next prompt not working inside tmux using Shell Integration
- Суть: навигация по промптам (OSC 133) не работает под tmux.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7166 (0) — Helix editor not loading ESC \u(1b) unicode character
- Суть: некорректная передача ESC-символа в Helix.
- Применимо: да, общее кодирование клавиш.
- Класс: functional
- Приоритет: medium

### Issue #7130 (0) — Slash with German layout not working with kitty input in non fish shells
- Суть: `/` на немецкой раскладке не работает с kitty keyboard protocol вне fish.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7109 (0) — No antialiasing when rendering images
- Суть: отсутствует сглаживание при рендере изображений (масштабирование).
- Применимо: да, наш image-рендер (tiny-skia).
- Класс: visual
- Приоритет: medium-high

### Issue #7099 (0) — macOS: version number/about item disappears from application menu after config reload
- Суть: пункт "About" пропадает из меню после перезагрузки конфига, macOS.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #7096 (0) — wezterm gui add tab, adds 18 new tabs with 18 kdialog
- Суть: массовое дублирование вкладок при спавне через kdialog (обратная связь событий, похоже на #3994).
- Применимо: да.
- Класс: functional
- Приоритет: medium-high

### Issue #7089 (0) — Divide by zero when Minimizing in search mode
- Суть: паника деления на ноль при сворачивании в режиме поиска.
- Применимо: да, общий search-оверлей код.
- Класс: crash
- Приоритет: high

### Issue #7085 (0) — Tmux control mode doesn't seem to realise that ssh pipe is broken
- Суть: tmux control mode не замечает разрыв пайпа (транспорт может быть внешний ssh, не наш клиент).
- Применимо: да, общий tmux-CC код.
- Класс: functional (зависание сессии)
- Приоритет: medium-high

### Issue #7082 (0) — Excessively greedy unwrapped url regex
- Суть: слишком "жадный" regex обнаружения URL.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7077 (0) — Text looks bad (white outlines?) if the transparent window has a light background
- Суть: артефакты сглаживания текста на прозрачном окне со светлым фоном под ним.
- Применимо: да, наш текст-рендер + прозрачность.
- Класс: visual
- Приоритет: medium-high

### Issue #7062 (0) — Blur not working
- Суть: эффект blur окна не работает.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #7052 (0) — SpawnCommandInNewTab from a client sends environment to server
- Суть: окружение клиента утекает на mux-сервер при спавне.
- Применимо: да, потенциальная утечка информации между клиентом/сервером.
- Класс: functional/security
- Приоритет: medium-high

### Issue #7041 (0) — OSC 7 shell integration not working with SSH to WSL
- Суть: OSC 7 не работает при подключении (внешний ssh) к WSL.
- Применимо: да, общий OSC7-парсинг.
- Класс: functional
- Приоритет: medium

### Issue #7037 (0) — Cursor style changes from bar to block when window loses focus, affecting text editors
- Суть: нежелательная смена стиля курсора при потере фокуса.
- Применимо: да.
- Класс: visual/functional
- Приоритет: medium

### Issue #7025 (0) — Portable-pty fails to launch commands on windows
- Суть: сбой запуска команд через portable-pty, Windows.
- Применимо: да, общий крейт.
- Класс: functional
- Приоритет: medium-high

### Issue #7023 (0) — Hovering cursor over inactive panes highlights URLs in the active one
- Суть: hover в неактивной панели подсвечивает URL в активной (дубль темы #7409).
- Применимо: да.
- Класс: visual/functional
- Приоритет: medium

### Issue #7012 (0) — Hover is inconsistently detected on Windows
- Суть: непоследовательный hover-детект, Windows.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #7006 (0) — Crash on Asahi Linux
- Суть: крэш на Asahi Linux (ARM).
- Применимо: да, но нишевое железо.
- Класс: crash
- Приоритет: medium-high

### Issue #6991 (0) — bug: rendering is offset vertically
- Суть: вертикальное смещение рендера.
- Применимо: да, core рендер.
- Класс: visual
- Приоритет: high

### Issue #6990 (0) — window:set_inner_size with current pixel_height still changes vertical height on Windows
- Суть: API set_inner_size меняет высоту, даже если передано текущее значение.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6987 (0) — Cannot disable CTRL+C/CTRL+V bindings on Windows
- Суть: нельзя отключить системные CTRL+C/V бинды на Windows.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #6986 (0) — Tabs do not use the full width of the tab bar
- Суть: вкладки не занимают всю ширину tab bar.
- Применимо: да.
- Класс: visual
- Приоритет: low-medium

### Issue #6974 (0) — Shell integration fails to parse hostname on windows
- Суть: shell integration не парсит hostname на Windows.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6971 (0) — SendKey with key = "Escape" Does not work
- Суть: действие SendKey с Escape не работает.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6958 (0) — Launching a new WezTerm window triggers a window demanding attention
- Суть: новое окно вызывает системный "demand attention" без причины.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #6954 (0) — SpawnTab fail by ShowLauncherArgs when current tab is wsl ubuntu
- Суть: сбой спавна вкладки через лаунчер из WSL-вкладки.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6950 (0) — Enter wsl window will lost input focus
- Суть: потеря фокуса ввода при входе в WSL-окно.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6931 (0) — Gaps between cells with ligatures using small font size, related to antialiasing
- Суть: зазоры между ячейками с лигатурами на мелком шрифте.
- Применимо: да, наш рендер лигатур/AA.
- Класс: visual
- Приоритет: medium-high

### Issue #6907 (0) — Korean input method Gureum (macOS) doesn't write the character with arrows
- Суть: (дубль #7473) IME Gureum + стрелки, macOS.
- Применимо: да, нишевый IME.
- Класс: functional
- Приоритет: low-medium

### Issue #6884 (0) — Mux Panes Should Resize When Window Size Changes
- Суть: панели mux не ресайзятся при изменении размера окна.
- Применимо: да.
- Класс: functional
- Приоритет: medium-high

### Issue #6844 (0) — Multiple AdjustPaneSize not consistent on ssh mux
- Применимо: N/A — подсистема удалена (явно указан ssh-mux домен).

### Issue #6839 (0) — 'mux.spawn_window' doesn't launch wezterm in mac
- Суть: mux.spawn_window не запускает окно на macOS.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6826 (0) — New window created by wezterm connect do not respect initial_cols, initial_rows
- Суть: mux connect игнорирует initial_cols/rows.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6793 (0) — Alt+n or ~ : weird behaviour
- Суть: (из тела) мёртвая клавиша `~` (Alt+n) не двигает курсор, macOS.
- Применимо: да, обработка dead-keys.
- Класс: functional
- Приоритет: medium

### Issue #6752 (0) — Keep symlink path when using --cwd
- Суть: --cwd резолвит symlink вместо сохранения исходного пути (дубль темы #7311).
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6719 (0) — No focus events when connected to mux server
- Суть: события фокуса не доходят в mux-сессии.
- Применимо: да.
- Класс: functional
- Приоритет: medium-high

### Issue #6623 (0) — Resize bug - with replication
- Суть: (из тела) курсор неверно позиционируется при ресайзе окна в некоторых случаях.
- Применимо: да, core resize/cursor-position (тема повторяется, см. #6669, #5100).
- Класс: functional
- Приоритет: medium-high

### Issue #6607 (0) — `gui.get_appearance()` returns 'Light' on auto-reload, regardless of system appearance
- Суть: get_appearance некорректен при авто-перезагрузке конфига.
- Применимо: да, rhai API.
- Класс: functional
- Приоритет: medium

### Issue #6603 (0) — TUI layout misalignment when encountering combining diacritical marks (grapheme clusters)
- Суть: смещение layout при grapheme-кластерах (дубль темы #3976).
- Применимо: да, core unicode/ширина ячейки.
- Класс: visual
- Приоритет: high — повторяющийся паттерн багов ширины unicode-кластеров.

### Issue #6586 (0) — App menu on Mac is not displaying the correct key binding for New Tab
- Суть: неверный шорткат отображается в меню macOS.
- Применимо: да.
- Класс: visual
- Приоритет: low-medium

### Issue #6575 (0) — Password is displayed when copy/pasted due to predictive output in multiplexer client
- Суть: пароль отображается из-за predictive echo в mux-клиенте (утечка приватных данных на экран).
- Применимо: да.
- Класс: functional/security
- Приоритет: medium-high

### Issue #6563 (0) — Terminfo not working properly for ubuntu
- Суть: проблемы terminfo-записи на Ubuntu.
- Применимо: да, общий terminfo.
- Класс: functional
- Приоритет: medium

### Issue #6555 (0) — Window in wrong place after screen lock with multi-resolution displays
- Суть: окно оказывается не на своём месте после блокировки экрана, мультимонитор.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6515 (0) — {} and [] won't input on Croatian (HR) layout
- Суть: не вводятся `{}`/`[]` на хорватской раскладке.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6513 (0) — New Windows Stop Opening After Awhile
- Суть: спустя время новые окна перестают открываться (похоже на утечку ресурсов/зависание).
- Применимо: да.
- Класс: hang/perf
- Приоритет: high

### Issue #6503 (0) — Initial window not working properly on Mac OS
- Суть: (из тела) позиционирование начального окна по gui-startup screen-ratio не работает как ожидается, macOS.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6501 (0) — BlinkingBar erases pixels under the cursor
- Суть: мигающий курсор-бар стирает пиксели под собой (артефакт рендера).
- Применимо: да.
- Класс: visual
- Приоритет: medium-high

### Issue #6465 (0) — Lower-case fallback font not found by wezterm
- Суть: fallback-шрифт не находится из-за регистра.
- Применимо: да, наш font locator.
- Класс: functional
- Приоритет: medium

### Issue #6437 (0) — text will jump up and down when focus/unfocus in full-screen mode
- Суть: текст "прыгает" при смене фокуса в fullscreen.
- Применимо: да.
- Класс: visual
- Приоритет: medium-high

### Issue #6433 (0) — Deleting all characters while compositing with IME leaves last character on terminal
- Суть: при удалении всей IME-композиции остаётся "хвост" символа.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6361 (0) — Crash in wezterm.strftime
- Суть: паника в API strftime конфигурации.
- Применимо: да, через rhai-эквивалент этой функции.
- Класс: crash
- Приоритет: high

### Issue #6314 (0) — Windows and Linux Remote Neovim Rendering Bug
- Суть: баг рендера удалённого Neovim, Windows и Linux.
- Применимо: да, вероятно общий рендер-баг, не зависящий от транспорта.
- Класс: visual
- Приоритет: medium-high

### Issue #6312 (0) — wezterm tab and window title don't update after exiting from interactive docker container
- Суть: заголовок не обновляется после выхода из интерактивного docker-контейнера (трекинг процесса).
- Применимо: да, связано с procinfo/title-tracking (недавно рефакторено).
- Класс: functional
- Приоритет: medium

### Issue #6306 (0) — highlighting text -> copy buffer is inconsistent
- Суть: непоследовательное копирование выделенного текста в буфер.
- Применимо: да.
- Класс: functional
- Приоритет: medium-high

### Issue #6299 (0) — Hitting <tab> for autocomplete leaves characters in the prompt
- Суть: автодополнение по Tab оставляет лишние символы в промпте.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6295 (0) — delete grapheme counting wrong 👩‍🚒
- Суть: неверный подсчёт при удалении grapheme-кластера (сложный ZWJ-эмодзи).
- Применимо: да, core unicode/grapheme-обработка.
- Класс: functional
- Приоритет: high

### Issue #6264 (0) — tried to install nix Darwin through flakes it shows error unsupported type gui-sock
- Суть: ошибка типа файла gui-sock при установке через nix Darwin.
- Применимо: да, но нишевый пакетный менеджер macOS.
- Класс: functional
- Приоритет: low-medium

### Issue #6234 (0) — Color fills second line when using a multi-line prompt with colored background segments
- Суть: цвет фона "перетекает" на вторую строку многострочного промпта.
- Применимо: да, core рендер фона по строкам.
- Класс: visual
- Приоритет: medium-high

### Issue #6222 (0) — Only last character of multi-character compose sequence is emitted
- Суть: при составной compose-последовательности выводится только последний символ.
- Применимо: да, общая обработка ввода/IME.
- Класс: functional
- Приоритет: high

### Issue #6215 (0) — enable_kitty_keyboard does not work together with multiplexing
- Суть: kitty keyboard protocol не работает совместно с mux.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6210 (0) — wezterm cli move-pane-to-new-tab does not work for multiplexed panes
- Суть: CLI move-pane-to-new-tab не работает для mux-панелей.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6205 (0) — Segmentation Fault on AMD Graphics driver 24.8.1
- Суть: сегфолт на конкретном AMD-драйвере.
- Применимо: да, GPU render backend.
- Класс: crash
- Приоритет: high

### Issue #6196 (0) — logname(1) returns root on macOS, instead of current user
- Суть: неверный логин-нейм внутри pty-сессии, macOS.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #6170 (0) — The display and behavior of Bengali in Vim and Neovim are quite strange
- Суть: странности рендера бенгальского письма (complex script shaping).
- Применимо: да, наш шейпинг-пайплайн.
- Класс: visual
- Приоритет: medium-high

### Issue #6166 (0) — Single line scrolling fails in mux-server, if application uses alternate screen mode
- Суть: построчный скролл не работает в mux при alt-screen приложениях.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6157 (0) — Panic after failing to load fallback font that was uninstalled after wezterm was launched
- Суть: паника при пропаже fallback-шрифта во время работы (тот же класс ошибки, что и #7963).
- Применимо: да — вероятно устранено тем же фиксом (`5752050b8`), но стоит проверить регрессионным тестом отдельно (сценарий "шрифт удалён после запуска", а не "недоступен по ACL").
- Класс: crash
- Приоритет: high

### Issue #6153 (0) — modifying "New Window" action breaks it for empty Spaces
- Суть: переопределение действия "New Window" ломает его для пустых Spaces, macOS.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #6141 (0) — Mouse click on padding area registers on first row of text being displayed
- Суть: клик в области padding засчитывается как клик по первой строке текста.
- Применимо: да, hit-testing.
- Класс: functional
- Приоритет: medium

### Issue #6139 (0) — Wezterm resize when covered by window after changing settings
- Суть: ресайз при перекрытии другим окном после смены настроек.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #6124 (0) — OSC 4 responses are limited to 31
- Суть: ответы OSC 4 (цветовая палитра) ограничены индексом 31.
- Применимо: да, core OSC-обработка.
- Класс: functional
- Приоритет: medium-high

### Issue #6080 (0) — Keycap digit sequence emoji (1️⃣-9️⃣) not rendering properly (size/cursor/column position)
- Суть: неверный рендер составных emoji-последовательностей (keycap).
- Применимо: да, ширина/рендер составных emoji.
- Класс: visual
- Приоритет: medium-high

### Issue #6071 (0) — Can't delete Zenkaku character in first (position)
- Суть: не удаляется широкий (zenkaku) символ в первой позиции.
- Применимо: да, дубль темы wide-char handling.
- Класс: functional
- Приоритет: medium

### Issue #6040 (0) — Weird issue with font sizes
- Суть: (из тела) метрики Nerd Font icons завышены по высоте относительно обычного текста того же font_size.
- Применимо: да, наш font-metrics код.
- Класс: visual
- Приоритет: medium

### Issue #6010 (0) — Capital cyrillic letters are not displayed when using TMUX with ModifyOtherKeys option
- Суть: заглавные кириллические буквы не отображаются с ModifyOtherKeys под tmux.
- Применимо: да.
- Класс: functional/visual
- Приоритет: medium

### Issue #5992 (0) — [windows] crash: wezterm_gui::termwindow > opengl context was lost; should reinit
- Суть: крэш при потере OpenGL-контекста вместо переинициализации.
- Применимо: да, общий render backend recovery код.
- Класс: crash
- Приоритет: high

### Issue #5982 (0) — get_appearance does not converge to system dark mode in various situations
- Суть: неверное определение системной тёмной темы в ряде случаев.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5955 (0) — add_to_config_reload_watch_list not working for Windows WSL paths
- Суть: отслеживание файлов конфига не работает для путей WSL на Windows.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5954 (0) — start with --class and --attach cannot attach to same instance with different class names
- Суть: --attach не находит существующий инстанс при разных --class.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #5944 (0) — Wezterm crash on launching in mac, tmux default program
- Суть: крэш при старте с tmux в качестве default_prog, macOS.
- Применимо: да.
- Класс: crash
- Приоритет: high

### Issue #5943 (0) — pane:get_cursor_position() doesn't respect Copy Mode
- Суть: API get_cursor_position не учитывает Copy Mode.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5927 (0) — [Windows] app becomes unresponsive while typing in commands
- Суть: приложение подвисает при вводе команд, Windows.
- Применимо: да.
- Класс: hang
- Приоритет: high

### Issue #5909 (0) — Duplicate semicolon between system and user PATH
- Суть: дублирование `;` при объединении PATH.
- Применимо: да, но малое влияние.
- Класс: functional
- Приоритет: low

### Issue #5908 (0) — Cannot create local domain splits on a mux tab
- Суть: нельзя создать сплит локального домена внутри mux-вкладки.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5888 (0) — copy_mode_active_highlight_* color attribute is overridden by selection_* until window is clicked
- Суть: цвета copy mode перекрываются selection-цветами до клика по окну.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #5887 (0) — Opening search overlay prevents mouse scrolling in some application content
- Суть: оверлей поиска блокирует скролл мышью в некоторых alt-screen приложениях.
- Применимо: да.
- Класс: functional
- Приоритет: medium-high

### Issue #5858 (0) — Emoji display issues with Apple Color Emoji font
- Суть: проблемы рендера с шрифтом Apple Color Emoji.
- Применимо: да, macOS-специфичный шрифтовый путь, но наш общий эмодзи-рендер.
- Класс: visual
- Приоритет: medium-high

### Issue #5837 (0) — Panic on ChromeOS using Wayland
- Суть: паника на ChromeOS/Wayland.
- Применимо: да, но нишевая ОС.
- Класс: crash
- Приоритет: medium-high

### Issue #5787 (0) — multiple links to the same destination on screen are underlined incorrectly on hover
- Суть: неверное подчёркивание при нескольких одинаковых ссылках на экране.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #5584 (0) — wezterm cannot handle symlink, makes duplicate icons when pinned on taskbar
- Суть: дублирование иконок в панели задач из-за symlink, Windows.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #5556 (0) — Unable to use ScrollToPrompt to scroll to the very first prompt on Arch Linux with Bash
- Суть: ScrollToPrompt не доходит до самого первого промпта.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5548 (0) — wezterm cli commands are very slow, for example when switching panes
- Суть: высокая задержка CLI-команд (например, переключение панелей).
- Применимо: да, общий CLI/IPC код.
- Класс: perf
- Приоритет: high

### Issue #5536 (0) — activate-pane Fail
- Суть: сбой команды activate-pane.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5520 (0) — unexpected behavior of PaneSelect with wezterm server
- Суть: неожиданное поведение PaneSelect в mux-сессии.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5499 (0) — format-tab-title does not get relevant panes passed in for the tab
- Суть: в колбэк format-tab-title передаются не те панели.
- Применимо: да, через rhai-эквивалент API.
- Класс: functional
- Приоритет: medium

### Issue #5492 (0) — Some special characters are not centered vertically
- Суть: некоторые спецсимволы не центрированы по вертикали.
- Применимо: да, рендер глифов.
- Класс: visual
- Приоритет: medium

### Issue #5486 (0) — Wezterm can't send s-<mouse-1> event when I press cmd+leftclick
- Суть: не отправляется событие мыши с модификатором Super на macOS.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5451 (0) — Updating config.colors property only hot-reloads non-focussed windows
- Суть: горячая перезагрузка цветов не применяется к активному (сфокусированному) окну.
- Применимо: да, важный баг конфиг-релоада.
- Класс: functional
- Приоритет: medium-high

### Issue #5392 (0) — Stuck on a blank white window if PowerToys FancyZones enabled with specific setting
- Суть: окно "зависает" пустым белым при взаимодействии с PowerToys FancyZones, Windows.
- Применимо: да.
- Класс: visual/functional
- Приоритет: medium-high

### Issue #5389 (0) — Alt+Tab switch app in Wayland causes maximum window layout into left panel
- Суть: переключение приложений по Alt+Tab вызывает неверное разворачивание окна в левую панель, Wayland.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5336 (0) — Fancy tab bar doesn't apply transparency consistently
- Суть: непоследовательная прозрачность fancy tab bar.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #5315 (0) — Cursor is changed from filled box to empty box after regaining focus in macOS native fullscreen mode
- Суть: неверный стиль курсора после возврата фокуса в нативном fullscreen, macOS.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #5314 (0) — MACOS_FORCE_ENABLE_SHADOW causes the window to not maximize when double-clicking the tab bar
- Суть: переменная окружения ломает maximize по двойному клику.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #5262 (0) — selection_word_boundary error with helix editor
- Суть: ошибка границы слова при выделении с Helix.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5249 (0) — Running specific command maybe cause spawn many pwsh.exe processes
- Суть: неконтролируемое размножение процессов pwsh.exe.
- Применимо: да, потенциальная утечка ресурсов.
- Класс: functional/perf
- Приоритет: medium-high

### Issue #5238 (0) — periodic lag when moving cursor or long output
- Суть: периодические лаги при движении курсора/долгом выводе.
- Применимо: да, core render/input pipeline.
- Класс: perf
- Приоритет: high

### Issue #5225 (0) — wezterm connect called from freedesktop file hangs
- Суть: зависание при запуске wezterm connect из .desktop-файла.
- Применимо: да.
- Класс: hang
- Приоритет: medium-high

### Issue #5224 (0) — Can't insert a single Chinese character in Fcitx5 with enable_kitty_keyboard
- Суть: ломается ввод китайского через Fcitx5 при включённом kitty keyboard protocol.
- Применимо: да, важно — полностью ломает CJK-ввод в этом режиме.
- Класс: functional
- Приоритет: medium-high

### Issue #5220 (0) — Hyphen-minus character (-) doesn't render using monofur font at 10pt
- Суть: не рендерится дефис в конкретном шрифте на конкретном размере.
- Применимо: да, но узкоспецифичный шрифт/размер.
- Класс: visual
- Приоритет: low-medium

### Issue #5199 (0) — Incorrectly render when using Thai language
- Суть: некорректный рендер тайского языка (complex script).
- Применимо: да, наш шейпинг-пайплайн.
- Класс: visual
- Приоритет: medium-high

### Issue #5190 (0) — use ~ or $HOME to set background doesn't work, and how to set background scale/center/full
- Суть: путь с `~`/`$HOME` для фона не разворачивается (частично баг, частично вопрос про масштабирование).
- Применимо: да, в части path expansion — реальный баг.
- Класс: functional
- Приоритет: low-medium

### Issue #5162 (0) — Doesn't work after upgrading to Plasma 6 on openSUSE
- Суть: несовместимость с KDE Plasma 6.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5139 (0) — Kitty keyboard: key repeat is not reported
- Суть: авто-повтор клавиши не репортится в kitty keyboard protocol.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5128 (0) — wezterm imgcat does not deallocate image memory after closing instance
- Суть: утечка памяти изображений после закрытия сессии imgcat.
- Применимо: да, общий image-decoding код.
- Класс: perf (memory leak)
- Приоритет: high

### Issue #5123 (0) — Side arrow divider isn't sharp compared to other terminals
- Суть: нечёткий рендер разделителя-стрелки в tab bar (антиалиасинг).
- Применимо: да, косметика.
- Класс: visual
- Приоритет: low-medium

### Issue #5106 (0) — Cannot render 𝝺 - mathematical sans-serif bold small lambda
- Суть: не рендерится редкий unicode-математический символ (нет fallback).
- Применимо: да, шрифтовый fallback.
- Класс: visual
- Приоритет: low-medium

### Issue #5100 (0) — Bad cursor position when terminal is resized while in alternate mode
- Суть: неверная позиция курсора при ресайзе в alt-screen (дубль темы #6669, #6623).
- Применимо: да, core reflow/cursor.
- Класс: functional
- Приоритет: medium-high

### Issue #5093 (0) — Crashes
- Суть: (из тела) крэш при выборе powershell из меню лаунчера внутри WSL-сессии.
- Применимо: да.
- Класс: crash
- Приоритет: high

### Issue #5088 (0) — get-pane-direction --pane-id x doesn't return expected result
- Суть: CLI get-pane-direction возвращает неверный результат.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #5060 (0) — wezterm selecting different sizes of fonts across different files
- Суть: неверный подбор размера между файлами одного шрифтового семейства (multi-file font family).
- Применимо: да, наш font matcher.
- Класс: visual
- Приоритет: medium-high

### Issue #5054 (0) — Retro bar performance issue and CPU usage
- Суть: retro-стиль tab bar вызывает повышенную нагрузку CPU.
- Применимо: да.
- Класс: perf
- Приоритет: high

### Issue #5022 (0) — Dragging Wezterm between 3840x2160 and 1920x1200 screens on Windows causes it to get stuck for some time
- Суть: временное зависание при перетаскивании между мониторами разного разрешения, Windows.
- Применимо: да.
- Класс: hang
- Приоритет: high

### Issue #5014 (0) — Windows fancy tab bar covering window management buttons
- Суть: fancy tab bar перекрывает системные кнопки управления окном, Windows.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #5011 (0) — Relative sizing of panes within a tab do not persist on GUI resize
- Суть: относительные размеры панелей не сохраняются при ресайзе GUI.
- Применимо: да.
- Класс: functional
- Приоритет: medium-high

### Issue #4935 (0) — wezterm record unusable when invoked from wsl
- Суть: функция record непригодна при вызове из WSL.
- Применимо: да, но нишевая фича.
- Класс: functional
- Приоритет: low-medium

### Issue #4871 (0) — Clickable area issue
- Суть: (из тела) URL за пределами окна wezterm остаётся кликабельным.
- Применимо: да, hit-test область окна.
- Класс: functional
- Приоритет: medium

### Issue #4838 (0) — GUI is effectively disabled if quit while part of mux session and config error window is present
- Суть: GUI фактически блокируется при выходе во время mux-сессии с окном ошибки конфигурации.
- Применимо: да.
- Класс: functional
- Приоритет: medium-high

### Issue #4723 (0) — Resize issue with wezterm connect domain
- Суть: баг ресайза при подключении к mux-домену.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4641 (0) — SelectTextAtMouseCursor(Block) not working
- Суть: действие блочного выделения по позиции мыши не работает.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4634 (0) — Open here does not work with windows network mappings
- Суть: "Open here" (explorer integration) не работает с сетевыми дисками, Windows.
- Применимо: да, но нишевый сценарий.
- Класс: functional
- Приоритет: low-medium

### Issue #4621 (0) — Modifiers keys not picked up by selection and mouse reporting when wezterm is unfocused
- Суть: модификаторы не учитываются в выделении/mouse reporting без фокуса окна.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4618 (0) — Avoid full path canonicalization when resolving cwd
- Суть: полная канонизация пути при резолве cwd (дубль темы #7311/#6752).
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4587 (0) — Monaspace underline rendering
- Суть: неверный рендер подчёркивания для конкретного шрифта (Monaspace).
- Применимо: да, наш рендер декораций текста.
- Класс: visual
- Приоритет: medium

### Issue #4579 (0) — 'mapped:' keybindings are not actually mapped when key_map_preference="Physical"
- Суть: keybindings с префиксом mapped: не работают при Physical preference.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4439 (0) — wezterm cli is not working entirely in tmux once application is quit
- Суть: CLI полностью перестаёт работать в tmux после выхода приложения.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4437 (0) — window_frame: titlebar_fg does not update along with the background counterparts
- Суть: titlebar_fg не обновляется вместе с background-аналогами.
- Применимо: да.
- Класс: visual
- Приоритет: low-medium

### Issue #4434 (0) — Wezterm render duplicate characters in Thai language
- Суть: дублирование символов при рендере тайского (дубль темы #5199).
- Применимо: да, наш шейпинг-пайплайн.
- Класс: visual
- Приоритет: medium-high

### Issue #4433 (0) — Titlebar on macOS is incorrectly semi-translucent
- Суть: неверная полупрозрачность титлбара, macOS.
- Применимо: да.
- Класс: visual
- Приоритет: low-medium

### Issue #4424 (0) — Search action has some error
- Суть: (из тела) повторное переключение режима поиска (regex/ignore-case) на третьем нажатии даёт ошибку.
- Применимо: да, общий search-оверлей.
- Класс: functional
- Приоритет: medium

### Issue #4283 (0) — Wezterm is "unresponsive" when trying to attach to an unreachable remote domain
- Суть: приложение зависает целиком при попытке подключиться к недоступному удалённому домену.
- Применимо: да, критично — блокирует весь GUI из-за одного недоступного домена.
- Класс: hang
- Приоритет: high

### Issue #4265 (0) — Prompt reprint problem after window resize
- Суть: проблема повторной печати промпта после ресайза окна.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4249 (0) — hide_mouse_cursor_when_typing doesn't work on Windows
- Суть: опция скрытия курсора мыши при наборе текста не работает на Windows.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #4156 (0) — Cannot drag from the very top of the tabbar when maximized
- Суть: невозможно перетащить окно за самый верх tab bar в развёрнутом состоянии.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #4152 (0) — connecting a different monitor while in fullscreen does not resize
- Суть: подключение другого монитора в fullscreen не вызывает ресайз.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4115 (0) — Emoji in explicit hyperlinks are unclickable
- Суть: эмодзи внутри явных гиперссылок не кликабельны.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4081 (0) — Mouse event passed to the terminal even when mouse bypass key is pressed
- Суть: событие мыши всё равно передаётся терминалу при зажатой bypass-клавише.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4048 (0) — When I set the "default_gui_startup_args", the position is random
- Суть: случайная позиция окна при заданных default_gui_startup_args.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #4003 (0) — Kitty keyboard: flag 8 (report all keys as escapes) ignored
- Суть: флаг протокола kitty keyboard игнорируется.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3997 (0) — Meta is handled as Alt in kitty mode
- Суть: Meta обрабатывается как Alt в kitty keyboard protocol (семантическая ошибка).
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3984 (0) — Kitty Keyboard protocol: Sending incorrect "associated text" on certain keypresses
- Суть: неверный "associated text" в некоторых нажатиях клавиш, kitty protocol.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3896 (0) — Wide character at rightmost column overflows the window bounds
- Суть: широкий символ в последней колонке выходит за границы окна.
- Применимо: да, core рендер широких символов.
- Класс: visual
- Приоритет: medium-high

### Issue #3885 (0) — Mouse up event swallowed on window focus
- Суть: событие отпускания кнопки мыши "проглатывается" при получении фокуса.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3883 (0) — Stops taking keyboard events when maximized and restored in quick succession
- Суть: полностью перестаёт принимать клавиатурный ввод при быстром maximize/restore.
- Применимо: да, критично — потеря ввода без явного крэша.
- Класс: functional (эффективно hang для ввода)
- Приоритет: high

### Issue #3881 (0) — wezterm.font_with_fallback breaks terminal with invalid arguments
- Суть: невалидные аргументы font_with_fallback ломают терминал (не просто ошибка конфига).
- Применимо: да, через rhai-эквивалент API.
- Класс: functional/crash-adjacent
- Приоритет: medium-high

### Issue #3759 (0) — Inconsistent sizes of one-eighth box-drawing characters
- Суть: непоследовательные размеры box-drawing глифов (1/8 блоки).
- Применимо: да, рендер глифов.
- Класс: visual
- Приоритет: medium

### Issue #3694 (0) — Resizing performance issue and sometimes incorrect dimension calculation when using MUX server
- Суть: проблемы производительности и неверный расчёт размеров при ресайзе через mux-сервер.
- Применимо: да.
- Класс: perf/functional
- Приоритет: high

### Issue #3633 (0) — Mux behavior with multiple windows
- Суть: (из тела) новые локальные окна открываются в последнем подключённом удалённом домене вместо локального.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3620 (0) — The selection is not put to clipboard if the mouse cursor hovers on the pane's border
- Суть: выделение не копируется в буфер, если курсор на границе панели.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3594 (0) — backspace key behavior differs from kitty in enhanced keyboard mode
- Суть: расхождение поведения backspace с kitty terminal в enhanced keyboard mode.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3483 (0) — JumpForward in copy-mode doesn't move to uppercase letters or symbols
- Суть: JumpForward в copy mode не находит заглавные буквы/символы.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #3229 (0) — Silent crash with WebGPU backend + Windows RDP
- Суть: тихий крэш WebGPU backend при использовании через RDP.
- Применимо: да, WebGpu backend присутствует.
- Класс: crash
- Приоритет: medium-high

### Issue #3115 (0) — Semantic right-side prompt NOT separated from next-line left-side prompt
- Суть: правосторонний семантический промпт не отделяется от промпта следующей строки.
- Применимо: да, shell integration semantic zones.
- Класс: functional
- Приоритет: medium

### Issue #3107 (0) — wezterm crashes when OS runs out of files
- Суть: крэш при исчерпании файловых дескрипторов ОС.
- Применимо: да, общая обработка ресурсов/fd.
- Класс: crash
- Приоритет: high

### Issue #2905 (0) — wezterm record - paste clipboard inserts additional chars at random positions
- Суть: вставка из буфера во время записи (record) добавляет случайные лишние символы.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #2837 (0) — Round border glitch when not using title bars
- Суть: артефакт скруглённой границы окна без титлбара.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #2800 (0) — Hiding title bar and launching wezterm with maximized, opens it in fullscreen
- Суть: при скрытом титлбаре и maximized-старте окно открывается в fullscreen вместо maximized.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #2579 (0) — Split created with top_level=true cannot be split any further if started from "non top-level split"
- Суть: ограничение/баг вложенных top-level сплитов.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #2317 (0) — drag & drop does not focus window as it should
- Суть: drag&drop не переводит фокус на окно как ожидается.
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #2217 (0) — unproperly blinking when drag through screens on Windows 10
- Суть: неверное мигание при перетаскивании окна между экранами, Win10.
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #2033 (0) — In Copy Mode, H and L (moving to top/bottom of viewport) doesn't go to correct location
- Суть: навигация H/L в Copy Mode ведёт не туда.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #1316 (0) — unknown/unspecified CSI from XTSHIFTESCAPE
- Суть: неизвестная/неспецифицированная CSI-последовательность из XTSHIFTESCAPE.
- Применимо: да, core escape-sequence парсер.
- Класс: functional
- Приоритет: low-medium

### Issue #285 (0) — Double mouse click for selection extension moves the starting point of the selection
- Суть: двойной клик для расширения выделения сдвигает начальную точку выделения.
- Применимо: да.
- Класс: functional
- Приоритет: medium

### Issue #5140 (2 коммент.) — Windows unix domains don't support separate sockets
- Суть: unix-домены на Windows не поддерживают раздельные сокеты (ограничение реализации).
- Применимо: да, общий unix-domain код.
- Класс: functional
- Приоритет: low

### Issue #7172 (1) — error retrieving stderr ttyname
- Суть: ошибка получения ttyname для stderr в некоторых сценариях.
- Применимо: да, вероятно связано с procinfo/pty.
- Класс: functional
- Приоритет: low-medium

### Issue #6237 (1) — Failure to start Wezterm after install
- Суть: (из тела) при старте после установки на Ubuntu возникает ошибка `mux::ssh_agent failed to set SSH_AUTH_SOCK`, мешающая запуску.
- Применимо: да — `ssh_agent`-форвардинг (mux_enable_ssh_agent) остался в форке, это не удалённый ssh-клиент.
- Класс: functional
- Приоритет: medium-high — блокирует запуск.

### Issue #6088 (1) — FZF Image preview with imgcat
- Суть: интеграция image preview fzf+imgcat не работает как ожидается.
- Применимо: да, общий imgcat/image-protocol код.
- Класс: functional
- Приоритет: low-medium

### Issue #5770 (1) — Emacsclient fails with errormessage
- Суть: emacsclient завершается с ошибкой при запуске из wezterm (вероятно связано со спавном процесса/pty).
- Применимо: да.
- Класс: functional
- Приоритет: low-medium

### Issue #5178 (1) — Permission denied when calling docker
- Суть: ошибка доступа при вызове docker из wezterm (edge case процесс-спавна).
- Применимо: да.
- Класс: functional
- Приоритет: low

### Issue #3477 (1) — Minor issue in wezterm's borders with Wayland and RESIZE (or the new Integrated one)
- Суть: мелкий визуальный баг границ окна на Wayland с RESIZE/Integrated декорациями.
- Применимо: да.
- Класс: visual
- Приоритет: low

### Issue #6444 (0 коммент.) — Issue when create new pane with script
- Суть: (из тела) запуск `script -f ...` через launch_menu/SpawnCommandInNewTab ведёт себя не так, как ожидается.
- Применимо: да, общий процесс-спавн/pty код.
- Класс: functional
- Приоритет: low

### Issue #6346 (0) — Confirmation window is not centred
- Суть: диалог подтверждения закрытия появляется не по центру.
- Применимо: да.
- Класс: visual
- Приоритет: low

### Issue #5847 (0) — `Time:sun_times(lat, lon)` returns incorrect progression > 1.0
- Суть: математическая ошибка в API wezterm.time (расчёт восхода/заката).
- Применимо: да, через rhai-эквивалент wezterm.time API.
- Класс: functional
- Приоритет: low-medium

### Issue #5239 (0) — macOS: fancy tab bar transparency not working properly
- Суть: непоследовательная прозрачность fancy tab bar на macOS (дубль темы #5336).
- Применимо: да.
- Класс: visual
- Приоритет: medium

### Issue #4450 (0) — Unicode version can not be set as a decimal, e.g. 15.1
- Суть: конфиг unicode_version не принимает дробные версии.
- Применимо: да.
- Класс: functional
- Приоритет: low

### Issue #3858 (0) — window_decorations='RESIZE' results in no border for floating windows on Sway
- Суть: отсутствие рамки у floating-окон с RESIZE-декорациями на Sway.
- Применимо: да, но специфично для одного WM (Sway).
- Класс: visual
- Приоритет: low-medium

## Неважное — feature requests / окружение-специфичные / нерелевантные (49 штук)

#4820 wezterm ssh new window focus issue — N/A, подсистема SSH-клиента удалена
#4693 Switching panes too quickly on SSH domain causes OOM — N/A, подсистема SSH-клиента удалена
#5934 ssh does not work when Hostname is an ipv6 — N/A, подсистема SSH-клиента удалена
#7659 WezTerm SSH can't handle long passphrases — N/A, подсистема SSH-клиента удалена
#7229 WezTerm SSH: sporadic failure - libssh ssh_connect() never called — N/A, подсистема удалена
#3784 wezterm_ssh: sftp.set_metadata fails — N/A, подсистема удалена
#3501 SSH UserKnownHostsFile /dev/null not understood — N/A, подсистема удалена
#5108 Wezterm SSH doesn't present login prompt with tailscale — N/A, подсистема удалена
#4756 Wezterm connect failed with "Match tagged" ssh config — N/A, ssh_config/ssh-домен удалены
#4284 ssh domains aren't updated when reloading config — N/A, ssh-домен удалён
#6840 Working directory expanded to realpath when ~ is symlink in SSHMUX — N/A, ssh-домен удалён
#3894 output from repl is broken when iterating over big table (Lua REPL) — N/A, mlua/Lua REPL удалён
#7477 failed to run custom build command for freetype v0.1.0 — N/A, freetype не используется в сборке
#7521 wezterm.lua doesn,t excist — вопрос пользователя, не баг
#6277 Artemis trojan false positive in Windows Setup — ложное срабатывание антивируса, не баг
#6178 KConfigIni parses wezterm-gui binary as config file — не баг wezterm
#7974 Crate spin 0.9.8 is yanked — supply-chain/build hygiene, не функциональный баг приложения
#7425 Build fails on Fedora 43 — окружение сборки, не наш build-процесс
#7952 Copr nightly opensuse builds failing — packaging/CI окружение
#7862 Wezterm launch issues Ubuntu 26.04 — окружение/дистрибутив
#7489 Ubuntu 22.04 build fails without libxkbcommon-x11-dev — недостающая системная зависимость, документируемо
#6888 Errors on Fedora 41 — окружение сборки
#5148 Can't build on FreeBSD-14-RELEASE — окружение сборки
#5484 Can not install in Debian 12 — окружение установки
#5945 add support for chimera linux — feature request (поддержка дистрибутива)
#6965 Update Flatpak to org.freedesktop.Platform/x86_64/24.08 — packaging request
#7313 Wezterm flatpak fails to run using XWayland — packaging/окружение
#7486 CFBundleShortVersionString in Info.plist is incorrect — packaging metadata, косметика
#7549 Release new termwiz — meta-запрос релиза
#5037 Support common MacOS Window options (Minimise etc) — feature request
#5487 No keyboard shortcut for modal dialog — feature gap, не поломанная функциональность
#5402 Keep aspect ratio when rendering images by kitty protocol — feature request
#7056 Support for undercurl in WINDOWS — feature gap (формулировка "support")
#1141 support kitty keyboard protocol — устаревший feature request, протокол давно реализован
#3867 Problem with MacBook text-to-speech interaction — нишевая accessibility-фича macOS, очень низкое влияние
#6279 Calendar Permissions not propagating in WezTerm — нишевая macOS-фича, не относится к терминалу
#6455 Control-[ not working from VNC — нишевый транспорт VNC
#5465 CMD+. to macOS via VNC does not act as expected — нишевый транспорт VNC
#6409 wezterm fails to start after using KVM — нишевая виртуализация, недостаточно данных
#7241 Warning when opening serial port while default_prog is set — минорное предупреждение, не влияет на работу
#5525 On Windows 11 History not working — вероятно ответственность шелла/readline, не баг wezterm
#7813 Color Scheme 'Modus-Operandi' seems not compatible with modus-themes in Emacs on macOS — совместимость конкретной цветовой темы со сторонним Emacs-пакетом, косметика/ниша
#6431 Pop Shell tiling with Wayland doesn't tile Wezterm nightly — поведение конкретного тайлингового WM, не в фокусе
#4261 cannot paste what copied from remote server (remmina with rdp) — нишевый транспорт (remmina+RDP)
#7460 DECANM vt52 default state - backwards for a reason? — вопрос о спецификации/дизайне, не баг
#7072 ReloadConfiguration doesn't add a new [ssh] domain to existing windows (again) — N/A, ssh-домен удалён
#6969 wezterm serial sluggish — нишевая функция serial-порта, не в фокусе
#5846 Cannot execute wezterm serial with default_prog = pwsh.exe — нишевая функция serial-порта
#5825 wezterm serial output differs from stty+cat — нишевая функция serial-порта
