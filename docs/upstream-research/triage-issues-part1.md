# Триаж bug-issues апстрима, часть 1 (issues 1-476 из 952, отсортированные по комментариям)

Источник: `docs/upstream-research/issues_chunk_00` (первая, самая обсуждаемая половина списка bug-issues wezterm/wezterm, от 77 комментариев вниз до 2).

Контекст форка: SSH-клиент и TLS-mux удалены целиком, Lua(mlua)→rhai, cairo→tiny-skia, freetype/harfbuzz→rustybuzz/swash, git2/rusqlite удалены (→ redb). Issues, завязанные на эти подсистемы, помечены "N/A — подсистема удалена/заменена" без глубокого анализа.

## Важное — крэши/зависания/тормоза/поломанная функциональность (≈230 issues)

### Issue #5990 (77 коммент.) — textures broken on NixOS master branch
- Суть: все глифы рендерятся как "тофу"-прямоугольники на NixOS; тело issue подтверждает полную поломку отображения текста.
- Применимо: вероятно сбой загрузки системных шрифтов/фолбэка (silent failure → все глифы invalid), напрямую пересекается с нашим недавним коммитом `onlyterm-font: skip unreadable fallback font candidates instead of erroring out` (5752050b8). Стоит проверить, не осталось ли похожих путей отказа.
- Класс: visual/functional (font fallback)
- Приоритет: medium — триггер специфичен для NixOS-сборки, но механизм отказа общий для шрифтовой подсистемы.

### Issue #5263 (39 коммент.) — running message loop: Error while flushing display: Broken pipe (os error 32); terminating
- Суть: приложение падает/завершается при обрыве соединения с X-сервером (broken pipe) вместо мягкой обработки ошибки.
- Применимо: обработка ошибок event-loop — общий код, не привязан к конкретному WM.
- Класс: crash
- Приоритет: high — частый краш (39 коммент.), лёгкая ошибка I/O не должна убивать процесс.

### Issue #3214 (30 коммент.) — Onlyterm has slow response and even freeze
- Суть: общие тормоза/фризы интерфейса.
- Применимо: платформонезависимая проблема производительности рендера/event loop.
- Класс: hang/perf
- Приоритет: high — часто обсуждаемая (30 коммент.), общая деградация отзывчивости.

### Issue #2665 (28 коммент.) — High CPU Usage
- Суть: общая жалоба на высокую загрузку CPU в простое/работе.
- Применимо: общий рендер-цикл/event loop.
- Класс: perf
- Приоритет: high.

### Issue #3774 (23 коммент.) — Font looks thin and spaced out compared to other terminals
- Суть: шрифт выглядит тоньше и более разрежённым, чем в других терминалах (хинтинг/AA/трекинг).
- Применимо: у нас другой стек рендера шрифтов (rustybuzz/swash + tiny-skia вместо harfbuzz/freetype/cairo) — старая причина бага могла исчезнуть или проявиться иначе, нужна повторная проверка на нашем стеке.
- Класс: visual
- Приоритет: medium.

### Issue #5332 (22 коммент.) — Window decorations do not work to move or resize the window
- Суть: перетаскивание/ресайз окна через "decorations" не работает.
- Применимо: логика CSD в window-крейте может быть общей для нескольких бэкендов.
- Класс: functional
- Приоритет: medium.

### Issue #3032 (22 коммент.) — Font rendering isn't gamma corrected when using WebGpu?
- Суть: рендер шрифтов через WebGpu не гамма-корректен, отличается от OpenGL.
- Применимо: WebGpu front-end используется у нас и сейчас (wgpu не убирали), общий рендер-пайплайн.
- Класс: visual
- Приоритет: medium/high.

### Issue #2445 (22 коммент.) — Crashes on Sway - not divisible by scale 2
- Суть: краш при нецелочисленном делении, связанном с масштабом экрана (scale factor).
- Применимо: похоже на баг в общей логике вычисления масштаба/размеров окна (quad/render code), просто проявляется на Sway; похожий паттерн краша по scale встречается в нескольких issues (см. #3687, #4857, #6233).
- Класс: crash
- Приоритет: high — краш есть краш, независимо от WM, где он был замечен впервые.

### Issue #3121 (21 коммент.) — Onlyterm fails to launch randomly
- Суть: (проверено тело) окно иногда не появляется при `onlyterm start --always-new-process`, если нет уже открытых окон; KDE Plasma/Wayland.
- Применимо: похоже на гонку в создании окна на Wayland-бэкенде, не общий код.
- Класс: functional (startup)
- Приоритет: medium — Linux/Wayland-специфичный триггер.

### Issue #2987 (21 коммент.) — Bad prompt on resize behavior
- Суть: некорректное поведение промпта/shell integration при ресайзе окна.
- Применимо: общая логика reflow/shell-integration.
- Класс: functional
- Приоритет: medium.

### Issue #2669 (21 коммент.) — Lag when switching between Stage manager tasks (macOS Ventura)
- Суть: подтормаживания при переключении между задачами Stage Manager на macOS.
- Применимо: macOS — платформа в фокусе (не депрiоритизирована как Wayland/X11).
- Класс: perf
- Приоритет: medium.

### Issue #5561 (20 коммент.) — When using Neovim in onlyterm, there will be borders around it that cannot be filled.
- Суть: визуальный артефакт — незакрашенные границы вокруг содержимого neovim.
- Применимо: общий рендер фона/паддинга.
- Класс: visual
- Приоритет: medium.

### Issue #817 (19 коммент.) — Laggy scrolling in (neo)vim on split window
- Суть: тормоза скролла в сплит-панелях с (neo)vim — старая, давно обсуждаемая проблема.
- Применимо: общий рендер-путь скролла/redraw.
- Класс: perf
- Приоритет: high — старая (issue #817) и часто повторяющаяся тема (см. также #6371, #5234, #7275).

### Issue #4052 (18 коммент.) — Typing is slow
- Суть: задержка ввода при печати.
- Применимо: общий input-latency путь.
- Класс: perf
- Приоритет: high.

### Issue #6925 (17 коммент.) — Onlyterm becomes slow after some time, I have to close and re-run it
- Суть: деградация производительности со временем работы (утечка/накопление состояния).
- Применимо: общий код (кеши, история scrollback, рендер).
- Класс: perf/leak
- Приоритет: high.

### Issue #5882 (17 коммент.) — [windows] onlyterm hangs randomly, mostly but not exclusively, when closing panes.
- Суть: зависание на Windows при закрытии панелей.
- Применимо: Windows — платформа в фокусе.
- Класс: hang
- Приоритет: high.

### Issue #5197 (17 коммент.) — Hanging after recent update on Arch
- Суть: зависание после обновления (дистрибутив Arch, но симптом — общий hang).
- Применимо: нужно уточнение причины; краш/hang класс всегда высокий приоритет независимо от дистрибутива-триггера.
- Класс: hang
- Приоритет: medium/high.

### Issue #4883 (17 коммент.) — tiling window manager resize race with "Update Available" notification, causes bad cursor positioning
- Суть: гонка между ресайзом окна и оверлеем уведомления "Update Available", ломающая позицию курсора.
- Применимо: notification-overlay и resize-логика — общий код onlyterm-gui, просто чаще проявляется в тайловых WM.
- Класс: functional (race)
- Приоритет: medium.

### Issue #4633 (17 коммент.) — Window is resized after Mac sleeps when connected to external monitor
- Суть: непрошеный ресайз окна после выхода Mac из сна с внешним монитором.
- Применимо: macOS — платформа в фокусе.
- Класс: functional
- Приоритет: medium.

### Issue #6111 (16 коммент.) — Visual Artifacts with win32_system_backdrop = 'Acrylic'
- Суть: визуальные артефакты при использовании Acrylic-фона на Windows.
- Применимо: Windows — платформа в фокусе, наш собственный рендер-код фона.
- Класс: visual
- Приоритет: medium/high.

### Issue #4607 (16 коммент.) — Onlyterm Mux Server causes weird line artifacts in neovim
- Суть: артефакты строк в neovim при использовании (локального) Mux-сервера.
- Применимо: локальный unix-domain mux сохранён в нашем форке (удалены только SSH/TLS-mux).
- Класс: visual
- Приоритет: medium/high.

### Issue #3510 (16 коммент.) — Pasting from Windows clipboard manager adds extra ``^[[200~``
- Суть: вставка из менеджера буфера обмена Windows добавляет лишнюю bracketed-paste последовательность.
- Применимо: Windows — платформа в фокусе, общий код bracketed paste.
- Класс: functional
- Приоритет: medium/high.

### Issue #6190 (15 коммент.) — Onlyterm Freeze when trying to close tab after long session
- Суть: зависание при закрытии вкладки после долгой сессии.
- Применимо: общий код управления вкладками/сессией.
- Класс: hang
- Приоритет: high.

### Issue #4975 (15 коммент.) — Non-standard modifier keys emit original ANSI characters for that key on keydown
- Суть: нестандартные модификаторы шлют исходные ANSI-символы вместо ожидаемого кода.
- Применимо: общая логика кодирования клавиш.
- Класс: functional
- Приоритет: medium.

### Issue #3425 (15 коммент.) — Unable to launch OnlyTerm with WebGPU front-end
- Суть: невозможность запуска с WebGPU front-end (краш при старте).
- Применимо: WebGpu front-end используется и у нас.
- Класс: crash
- Приоритет: high.

### Issue #3221 (15 коммент.) — Can not swtich input method
- Суть: невозможность переключить метод ввода (IME).
- Применимо: общая IME-логика, платформа неясна из заголовка.
- Класс: functional
- Приоритет: medium.

### Issue #2958 (15 коммент.) — Incorrect initial terminal size on macOS with multiple screens
- Суть: неверный начальный размер терминала на macOS с несколькими экранами.
- Применимо: macOS — платформа в фокусе.
- Класс: functional
- Приоритет: medium.

### Issue #1983 (15 коммент.) — Moving a OnlyTerm window between monitors causes erratic jumping and resizing of the window
- Суть: хаотичные прыжки/ресайз при перетаскивании окна между мониторами (разный DPI).
- Класс: visual/functional
- Применимо: общая логика обработки смены монитора/DPI.
- Приоритет: medium.

### Issue #7503 (14 коммент.) — Flickering bar on full screen
- Суть: мерцающая полоса в полноэкранном режиме.
- Применимо: общий рендер fullscreen.
- Класс: visual
- Приоритет: medium.

### Issue #5468 (14 коммент.) — option key not working on mac
- Суть: клавиша Option не работает как ожидается на macOS.
- Применимо: macOS — платформа в фокусе.
- Класс: functional
- Приоритет: medium/high.

### Issue #5212 (14 коммент.) — mouse click on onlyterm, but activate the lower level window. (keyboard input always work)
- Суть: клик мышью активирует окно, находящееся под onlyterm, а не сам onlyterm.
- Применимо: общая логика фокуса/z-order.
- Класс: functional
- Приоритет: medium.

### Issue #5138 (14 коммент.) — Graphic glitch when window size is small
- Суть: графический глюк при маленьком размере окна (edge case).
- Применимо: общий рендер-код.
- Класс: visual
- Приоритет: medium.

### Issue #3328 (14 коммент.) — Some windows don't update their colour scheme when system theme changes
- Суть: не все окна подхватывают смену системной темы.
- Применимо: общая логика auto light/dark switching.
- Класс: functional
- Приоритет: medium.

### Issue #2779 (14 коммент.) — Cursor flashing / teleporting on typing in a nushell prompt on windows
- Суть: курсор "мигает"/телепортируется при вводе в nushell на Windows.
- Применимо: Windows — платформа в фокусе, общий код позиционирования курсора.
- Класс: visual
- Приоритет: medium.

### Issue #7520 (13 коммент.) — Quit Opencode will close OnlyTerm tab
- Суть: выход из стороннего инструмента (Opencode) неожиданно закрывает вкладку onlyterm — вероятно, из-за обработки некой escape-последовательности.
- Применимо: общая обработка управляющих последовательностей терминала.
- Класс: functional
- Приоритет: low/medium.

### Issue #5260 (13 коммент.) — Cannot input many unicode characters on windows on some applications
- Суть: невозможность ввода многих unicode-символов в некоторых приложениях на Windows.
- Применимо: Windows — платформа в фокусе, IME/ввод unicode.
- Класс: functional
- Приоритет: medium.

### Issue #4933 (13 коммент.) — There are key combinations that don't work with Helix, such as "Ctrl + º"
- Суть: некоторые сочетания клавиш не работают с Helix.
- Применимо: общая логика кодирования клавиш.
- Класс: functional
- Приоритет: medium.

### Issue #4700 (13 коммент.) — Performance gap between Onlyterm and Windows Terminal on Windows 11 VM
- Суть: заметный разрыв в производительности с Windows Terminal.
- Применимо: Windows — платформа в фокусе.
- Класс: perf
- Приоритет: medium/high.

### Issue #4161 (13 коммент.) — Error after manually closing last tab on ssh domain
- Применимо: N/A — подсистема SSH-domain удалена целиком.

### Issue #3803 (13 коммент.) — default hyperlink_rules is wrong for Markdown link format
- Суть: встроенные правила гиперссылок неверно работают для markdown-ссылок.
- Применимо: общая логика парсинга гиперссылок (config).
- Класс: functional
- Приоритет: medium.

### Issue #3687 (13 коммент.) — Onlyterm crashes with display scale > 1 when client-side-decorations enabled.
- Суть: краш при scale>1 с включёнными CSD.
- Применимо: похожий паттерн scale-related краша, как #2445 — вероятно общий код обработки масштаба в связке с decorations.
- Класс: crash
- Приоритет: high.

### Issue #6192 (12 коммент.) — ls --hyperlink shows error when clicked
- Суть: клик по гиперссылке из `ls --hyperlink` вызывает ошибку.
- Применимо: общая обработка OSC8/file:// гиперссылок.
- Класс: functional
- Приоритет: medium.

### Issue #5193 (12 коммент.) — On key-down, the shift modifier is lost in key input encoding (between raw_key_event_impl and key_event_impl)
- Суть: потеря модификатора Shift между низкоуровневой и высокоуровневой обработкой клавиш.
- Применимо: общий код кодирования клавиш.
- Класс: functional
- Приоритет: medium/high.

### Issue #4317 (12 коммент.) — When closing tab or application, current focus window in tmux is closed on mac
- Суть: закрытие вкладки/приложения на mac неожиданно закрывает не тот tmux-window.
- Применимо: macOS — платформа в фокусе, интеграция с tmux.
- Класс: functional
- Приоритет: medium.

### Issue #4225 (12 коммент.) — cant map ctrl+alt+s, ctrl+alt+l etc....
- Суть: невозможность забиндить некоторые сочетания Ctrl+Alt+буква.
- Применимо: общая логика key binding/кодирования.
- Класс: functional
- Приоритет: medium.

### Issue #4078 (12 коммент.) — AA artifacts in some powerline characters with custom_block_glyphs = true
- Суть: артефакты антиалиасинга на некоторых powerline-глифах.
- Применимо: наш рендер-стек изменился (tiny-skia) — нужна проверка актуальности.
- Класс: visual
- Приоритет: medium.

### Issue #3609 (12 коммент.) — Duplicated key-inputs (characters, commands)
- Суть: дублирование ввода клавиш/команд.
- Применимо: общая логика обработки клавиатурных событий; повторяющаяся тема (см. также #6725 на Wayland).
- Класс: functional
- Приоритет: high.

### Issue #3083 (12 коммент.) — ProxyCommand not working on Windows
- Применимо: N/A — подсистема SSH-клиента (ProxyCommand) удалена.

### Issue #1923 (12 коммент.) — Scroll history is missing lines
- Суть: пропадают строки в истории скролла — потеря данных.
- Применимо: ядро term/scrollback — общий код.
- Класс: functional
- Приоритет: high.

### Issue #6838 (8 коммент.) — Onlyterm sending weird key sequences to Neovim over ssh
- Суть: неверные последовательности клавиш, отправляемые в neovim (пользователь работает через обычный ssh-клиент внутри терминала, не наш встроенный SSH-домен).
- Применимо: общая логика кодирования клавиш — не относится к удалённой подсистеме SSH-домена.
- Класс: functional
- Приоритет: medium.

### Issue #6823 (11 коммент.) — `window_content_alignment` not rendering `r` the same when `vertical = "Center"`
- Суть: несогласованный рендер при вертикальном центрировании.
- Класс: visual
- Применимо: общий код layout.
- Приоритет: low/medium.

### Issue #6086 (11 коммент.) — Using INTEGRATED_BUTTONS decorations on Linux, the minimize button is nonfunctional
- Суть: неработающая кнопка minimize у собственной фичи INTEGRATED_BUTTONS.
- Применимо: INTEGRATED_BUTTONS — общая фича onlyterm-gui, не специфична для одного WM.
- Класс: functional
- Приоритет: medium.

### Issue #5524 (11 коммент.) — When the `Mission Control` is turned on, not smooth
- Суть: подтормаживания при работе с Mission Control на macOS.
- Применимо: macOS — платформа в фокусе.
- Класс: perf
- Приоритет: medium.

### Issue #5119 (11 коммент.) — starship git status icon (arrow up, arrow down) not rendering properly
- Суть: неверный рендер иконок nerd-font в prompt.
- Применимо: общий рендер глифов/fallback шрифтов.
- Класс: visual
- Приоритет: medium.

### Issue #5056 (11 коммент.) — Some characters, such as "﷽" doesn't display properly (its super small).
- Суть: некоторые символы рендерятся слишком мелкими.
- Применимо: общая логика шейпинга/масштабирования глифов.
- Класс: visual
- Приоритет: low/medium.

### Issue #5007 (11 коммент.) — Command for setting user vars is being printed before each command
- Суть: служебная команда shell-integration "протекает" в вывод.
- Применимо: общий shell-integration код.
- Класс: functional
- Приоритет: medium.

### Issue #6664 (10 коммент.) — Runtime color_scheme changes does not always apply to all active panes
- Суть: смена цветовой схемы на лету применяется не ко всем панелям.
- Применимо: общий config-engine.
- Класс: functional
- Приоритет: medium.

### Issue #6637 (10 коммент.) — U+2028 doesn't render as space in neovim, causing alignment issues
- Суть: неверная обработка ширины U+2028, ломающая выравнивание.
- Применимо: общая логика ширины unicode-символов (term core).
- Класс: functional
- Приоритет: medium.

### Issue #5142 (10 коммент.) — resizing in mux domains has issues
- Суть: проблемы при ресайзе в mux-доменах (локальный unix-domain mux сохранён).
- Класс: functional
- Приоритет: medium.

### Issue #4912 (10 коммент.) — imgcat panic when invoked from powershell under WSL paths
- Суть: паника `onlyterm imgcat` при вызове из PowerShell с WSL-путями.
- Применимо: Windows/WSL — платформа в фокусе, общая фича imgcat.
- Класс: crash
- Приоритет: medium/high.

### Issue #4881 (10 коммент.) — onlyterm no longer respects window_background_opacity if gpu is in use with other program
- Суть: регрессия прозрачности фона при конкуренции за GPU с другим приложением.
- Применимо: общий рендер-код (GPU-композитинг).
- Класс: visual
- Приоритет: medium.

### Issue #4874 (10 коммент.) — Font rendering of ligatures (and possibly other features?) is sometimes inconsistent
- Суть: непоследовательный рендер лигатур.
- Применимо: наш шейпинг теперь на rustybuzz — стоит перепроверить.
- Класс: visual
- Приоритет: medium.

### Issue #4851 (10 коммент.) — font_size != 12 vs initial_cols + initial_rows vs external monitor
- Суть: неверный расчёт размера окна из font_size+cols/rows на внешнем мониторе.
- Класс: functional
- Приоритет: medium.

### Issue #4826 (10 коммент.) — Onlyterm wgpu VRAM memory leak
- Суть: утечка видеопамяти в wgpu-рендерере.
- Применимо: wgpu используется у нас и сейчас.
- Класс: leak
- Приоритет: high.

### Issue #4527 (10 коммент.) — Unexpected extra window when opening a workspace in a domain.
- Суть: лишнее окно при открытии workspace в домене.
- Применимо: общая логика mux/workspace.
- Класс: functional
- Приоритет: medium.

### Issue #4456 (10 коммент.) — failed to connect to Socket("gui-sock-...")
- Суть: ошибка подключения к локальному unix-сокету gui-sock (внутренний IPC, не SSH).
- Применимо: общий код локального IPC.
- Класс: functional
- Приоритет: medium/high.

### Issue #4145 (10 коммент.) — win32_system_backdrop Acrylic not working
- Суть: Acrylic-фон не работает на Windows (см. также #6111).
- Применимо: Windows — платформа в фокусе.
- Класс: visual
- Приоритет: medium.

### Issue #4055 (10 коммент.) — ctrl-space tra[pped] (усечённый заголовок)
- Суть: (проверено тело) Ctrl+Space обрабатывается неверно в связке с режимом "capture" в neovim (Ctrl+V) на Windows.
- Применимо: общая логика кодирования клавиш.
- Класс: functional
- Приоритет: low/medium.

### Issue #2699 (10 коммент.) — Cannot create files over SFTP `LibSsh(Sftp(SftpError(2)))`
- Применимо: N/A — SFTP/SSH-клиент удалён целиком.

### Issue #2536 (10 коммент.) — Window goes out of fullscreen when focus shifts
- Суть: непрошеный выход из полноэкранного режима при смене фокуса.
- Класс: functional
- Приоритет: medium.

### Issue #2387 (10 коммент.) — Onlyterm seems to forget the current keymap randomly on Gnome Wayland
- Применимо: Wayland/GNOME-специфичная раскладка клавиатуры.
- Класс: functional
- Приоритет: low (Wayland/GNOME backend-специфично, не в фокусе).

### Issue #7150 (9 коммент.) — NixOS: head of main built with nix flake crashes with EGL error
- Применимо: NixOS-специфичная сборка/EGL-загрузка — не в фокусе.

### Issue #6783 (9 коммент.) — portable-pty 0.9.0 doesn't work on windows
- Суть: критическая регрессия создания PTY на Windows в portable-pty 0.9.0.
- Применимо: Windows — платформа в фокусе, ядро функциональности (spawn процессов).
- Класс: crash/functional
- Приоритет: high.

### Issue #6435 (9 коммент.) — Onlyterm occasionally interprets single clicks as double clicks (or triple lcicks)
- Суть: ошибочное распознавание количества кликов (click-timing).
- Применимо: общая логика обработки мыши.
- Класс: functional
- Приоритет: medium/high.

### Issue #5942 (9 коммент.) — OnlyTerm freezes after keyboard repeat
- Суть: зависание после автоповтора клавиши.
- Класс: hang
- Приоритет: high.

### Issue #5341 (9 коммент.) — font fallback for U+2387 is inconsistent with native macOS apps
- Суть: несогласованный выбор fallback-шрифта на macOS.
- Применимо: напрямую пересекается с недавним изменением `onlyterm-font: skip unreadable fallback font candidates` — стоит проверить регрессию/улучшение.
- Класс: visual
- Приоритет: medium.

### Issue #5083 (9 коммент.) — Neovim Always Scrolled Down When Launched Using Onlyterm CLI
- Суть: neovim при запуске из `onlyterm cli` всегда проскроллен вниз.
- Класс: functional
- Приоритет: medium.

### Issue #4512 (9 коммент.) — macOS Sonoma Copy Issues From Onlyterm
- Суть: проблемы с копированием на macOS Sonoma.
- Применимо: macOS — платформа в фокусе.
- Класс: functional
- Приоритет: medium.

### Issue #4121 (9 коммент.) — set-working-directory ignored when starting
- Суть: игнорируется рабочая директория при старте.
- Класс: functional
- Приоритет: medium.

### Issue #3936 (9 коммент.) — window_decoration mode "RESIZE" still shows title bar in X11 (Gnome 44, Fedora 38)
- Суть: флаг RESIZE трактуется как RESIZE|TITLE — похоже на тот же корень, что и #6920.
- Применимо: возможно общий баг парсинга флагов декораций (config), а не X11-специфика.
- Класс: functional
- Приоритет: medium.

### Issue #3747 (9 коммент.) — Quick key chords send wrong keys in neovim
- Суть: быстрые последовательности клавиш кодируются неверно.
- Класс: functional
- Приоритет: medium.

### Issue #7715 (8 коммент.) — background colour does not appear to apply correctly to ICH
- Суть: неверный цвет фона при Insert Character (ICH) — баг в ядре эмуляции терминала.
- Применимо: общий код term/.
- Класс: functional/visual
- Приоритет: medium.

### Issue #7275 (8 коммент.) — High CPU usage and freezing when scrolling
- Суть: высокая загрузка CPU и фризы при скролле.
- Класс: perf/hang
- Приоритет: high.

### Issue #6920 (8 коммент.) — Onlyterm interpret config.window_decoration option value "RESIZE" as "RESIZE|TITLE"
- Суть: баг парсинга/интерпретации флагов конфигурации декораций окна — общий код, не завязан на конкретный WM (см. также #3936, #6578).
- Класс: functional
- Приоритет: medium/high.

### Issue #6485 (8 коммент.) — Border problems with the rounded corners on macOS.
- Суть: проблемы рендера скруглённых углов на macOS.
- Применимо: macOS — платформа в фокусе.
- Класс: visual
- Приоритет: medium.

### Issue #6359 (8 коммент.) — `window_background_opacity` doesn't work on WebGpu
- Суть: прозрачность фона не работает во фронтенде WebGpu.
- Применимо: WebGpu используется у нас.
- Класс: functional
- Приоритет: medium.

### Issue #6351 (6 коммент.) — Integrated Buttons: Hide button not working
- Суть: не работает кнопка Hide в общей фиче Integrated Buttons.
- Класс: functional
- Приоритет: medium.

### Issue #6275 (8 коммент.) — Window Displacement Issue in Fullscreen Mode When Terminal Loses Focus
- Суть: смещение окна в fullscreen при потере фокуса.
- Класс: functional
- Приоритет: medium.

### Issue #5902 (8 коммент.) — SpawnTab isn't going to wsl home directory
- Суть: SpawnTab не открывает домашнюю директорию WSL.
- Применимо: WSL — Windows-смежная фича, в фокусе.
- Класс: functional
- Приоритет: medium.

### Issue #5866 (8 коммент.) — Chained dead keys keys are not supported
- Суть: не поддерживаются цепочки dead-keys (составной ввод диакритики).
- Класс: functional
- Приоритет: medium.

### Issue #5518 (8 коммент.) — Onlyterm panic: called `Option::unwrap()` on a `None` value
- Суть: паника из-за unwrap на None — классический класс Rust-багов.
- Класс: crash
- Приоритет: high.

### Issue #5503 (8 коммент.) — WSL - `default_cwd` UNC path only works in the first tab opened
- Суть: default_cwd с UNC-путём WSL работает только в первой вкладке.
- Применимо: WSL — Windows-смежная фича.
- Класс: functional
- Приоритет: medium.

### Issue #5366 (8 коммент.) — Mouse freezes, skips, and stutters over the onlyterm window, recovers as soon as it leaves.
- Суть: мышь подвисает/дёргается именно над окном onlyterm.
- Применимо: общий event loop обработки мыши.
- Класс: hang/perf
- Приоритет: high.

### Issue #5166 (8 коммент.) — The `windows.toast_notification()` function is not working properly.
- Суть: неисправная функция toast-уведомлений на Windows.
- Применимо: Windows — платформа в фокусе.
- Класс: functional
- Приоритет: medium.

### Issue #4708 (8 коммент.) — Onlyterm runs but doesn't display a window
- Суть: процесс запускается, но окно не отображается (startup-функциональный сбой).
- Класс: functional/crash
- Приоритет: high.

### Issue #4364 (8 коммент.) — Can't start.
- Суть: (проверено тело) на Windows дочерний процесс (cmd.exe/powershell.exe) "не выходит чисто", ошибка запуска.
- Применимо: Windows — платформа в фокусе, спавн процессов.
- Класс: crash/functional
- Приоритет: medium.

### Issue #4290 (8 коммент.) — poll_for_queued_event: X(Match(RequestError when front_end="WebGpu"
- Суть: ошибка протокола X11 в связке с WebGpu.
- Применимо: WebGpu общий, но триггер X11-специфичен.
- Класс: crash
- Приоритет: medium.

### Issue #4250 (8 коммент.) — font size affects window size
- Суть: изменение размера шрифта нежелательно меняет размер окна (повторяющаяся тема, см. #5122, #7932).
- Класс: functional
- Приоритет: medium.

### Issue #4186 (8 коммент.) — Trackpad scrolling with less causes screen blinking
- Суть: мерцание экрана при скролле в `less`.
- Класс: visual
- Приоритет: medium.

### Issue #2897 (8 коммент.) — emoji presentation seems to only occasionally work
- Суть: непоследовательный рендер emoji.
- Класс: visual
- Приоритет: medium.

### Issue #2744 (8 коммент.) — path search finds unix shell script before .cmd or .ps1 script on windows
- Суть: неверный порядок разрешения PATH на Windows (находит unix-скрипт раньше .cmd/.ps1).
- Применимо: Windows — платформа в фокусе.
- Класс: functional
- Приоритет: medium.

### Issue #837 (8 коммент.) — a single tab impacts perf of all tabs when running a heavy program.
- Суть: тяжёлая нагрузка в одной вкладке тормозит все остальные — архитектурная проблема производительности.
- Класс: perf
- Приоритет: high — старая (#837), фундаментальная проблема масштабируемости.

### Issue #7427 (7 коммент.) — Crash (segfault) with no apparent reason on mac
- Суть: сегфолт без явной причины на macOS.
- Класс: crash
- Приоритет: high.

### Issue #7271 (7 коммент.) — High GPU usage, per window, after upgrading to macOS Tahoe
- Суть: рост нагрузки на GPU после апгрейда на macOS Tahoe.
- Класс: perf
- Приоритет: medium/high.

### Issue #7118 (7 коммент.) — Almost daily crash (closed after press any key) on MacOS
- Суть: частый (почти ежедневный) краш на macOS.
- Класс: crash
- Приоритет: high.

### Issue #7101 (7 коммент.) — Onlyterm crashing after last update
- Суть: краш-регрессия после обновления (детали не ясны из заголовка).
- Класс: crash
- Приоритет: medium.

### Issue #7000 (7 коммент.) — Onlyterm is haunted
- Суть: (проверено тело) через день-два работы поведение мыши/selection самопроизвольно меняется (Cell → Line) без видимой причины — похоже на порчу состояния биндингов.
- Применимо: возможно общий код обработки состояния мышиных биндингов, KDE/Wayland — лишь площадка обнаружения.
- Класс: functional (state corruption)
- Приоритет: medium.

### Issue #6828 (7 коммент.) — tmux -CC does not react to `tmux split-window`
- Суть: tmux control-mode (-CC) не реагирует на split-window.
- Применимо: интеграция с tmux -CC — общий код, не относится к удалённому SSH-клиенту.
- Класс: functional
- Приоритет: medium.

### Issue #6806 (7 коммент.) — tmux -CC causing latency with remote VMs
- Суть: задержки в control-mode tmux при удалённых VM.
- Класс: perf
- Приоритет: medium.

### Issue #6179 (7 коммент.) — `onlyterm connect --new-tab` still opens new window
- Суть: флаг --new-tab не соблюдается при connect к mux-домену.
- Класс: functional
- Приоритет: medium.

### Issue #5817 (7 коммент.) — mux_enable_ssh_agent=true disables private key authentication w/ OpenSSH_for_Windows
- Применимо: N/A — SSH-агент/SSH-клиент удалён целиком.

### Issue #5560 (7 коммент.) — Text cursor flickers / jumps around when updating terminal
- Суть: курсор мерцает/прыгает при обновлении экрана.
- Класс: visual
- Приоритет: medium.

### Issue #5496 (7 коммент.) — Onlyterm ocassionally hangs when I try to close a pane
- Суть: периодическое зависание при закрытии панели.
- Класс: hang
- Приоритет: high.

### Issue #4916 (7 коммент.) — Unable to type a caret ('^') with kitty keyboard enabled
- Суть: невозможность ввести caret с включённым kitty-keyboard протоколом.
- Класс: functional
- Приоритет: medium.

### Issue #4509 (7 коммент.) — When reattaching to tmux, onlyterm prints the current version number
- Суть: "утечка" версии в вывод при переподключении к tmux.
- Класс: functional
- Приоритет: low.

### Issue #4484 (7 коммент.) — Pane focus changing constantly despite no input
- Суть: самопроизвольная постоянная смена фокуса панели без ввода.
- Класс: functional
- Приоритет: high (навязчивый UX-баг).

### Issue #4396 (7 коммент.) — Weztem failed to run
- Суть: неудачный запуск (мало деталей в заголовке).
- Класс: crash
- Приоритет: medium.

### Issue #4390 (7 коммент.) — Switching panes in multiple directions at once causes the focused pane to rapidly switch when any key is pressed
- Суть: гонка при навигации между панелями по нескольким направлениям.
- Класс: functional
- Приоритет: medium.

### Issue #4236 (7 коммент.) — Bracketed paste not working in Helix
- Суть: не работает bracketed paste в Helix.
- Класс: functional
- Приоритет: medium.

### Issue #4205 (7 коммент.) — portable-pty removes custom paths from PATH on windows
- Суть: portable-pty затирает пользовательские пути из PATH на Windows.
- Применимо: Windows — платформа в фокусе.
- Класс: functional
- Приоритет: high.

### Issue #4061 (7 коммент.) — Key repeat gets stuck after pressing two keys in fast succession (again)
- Суть: залипание автоповтора клавиш при быстром нажатии двух клавиш — регрессия ("again").
- Класс: functional
- Приоритет: high.

### Issue #3968 (7 коммент.) — Paste not working when `onlyterm connect unix_domain` from local WSL2
- Суть: не работает вставка при подключении к unix_domain из WSL2.
- Применимо: локальный mux (unix domain) сохранён.
- Класс: functional
- Приоритет: medium.

### Issue #3411 (7 коммент.) — Pre-edit/on-the-spot IME not working
- Суть: не работает pre-edit IME (важно для CJK-ввода).
- Класс: functional
- Приоритет: medium/high.

### Issue #2431 (7 коммент.) — Font rendering issues with default font settings; depend on zoom
- Суть: проблемы рендера шрифта, зависящие от zoom.
- Класс: visual
- Приоритет: medium.

### Issue #2414 (7 коммент.) — Clicking on unfocused onlyterm reports a mouse drag to client software
- Суть: клик по нефокусированному окну ошибочно репортится клиентскому ПО как drag.
- Класс: functional
- Приоритет: medium.

### Issue #2070 (7 коммент.) — Reduce cpu usage
- Суть: общая жалоба/пожелание снизить нагрузку CPU (повторяет тему #2665, #6855).
- Класс: perf
- Приоритет: medium.

### Issue #7497 (6 коммент.) — Dynamic window size only works for the first window
- Суть: динамический размер окна работает только для первого окна.
- Класс: functional
- Приоритет: medium.

### Issue #7326 (6 коммент.) — `onlyterm start` doesn't start window on top and focused sometimes on macos
- Суть: окно иногда не получает фокус/не выходит наверх при старте на macOS.
- Класс: functional
- Приоритет: medium.

### Issue #7090 (6 коммент.) — OnlyTerm shell integration conflicts with other terminal programs (onlyterm.sh)
- Суть: скрипт shell-интеграции конфликтует с другими терминальными программами.
- Класс: functional
- Приоритет: medium.

### Issue #6754 (3 коммент.) — Mouse scroll increase not working
- Суть: не работает увеличение скорости скролла мыши.
- Класс: functional
- Приоритет: low.

### Issue #6265 (6 коммент.) — Cannot get Onlyterm Terminal Transparent/Acrylic BG on Windows
- Суть: не удаётся получить прозрачный/Acrylic фон на Windows.
- Применимо: Windows — платформа в фокусе.
- Класс: functional
- Приоритет: medium.

### Issue #6232 (6 коммент.) — Incorrect cursor position when prompt has iTerm style images inlined
- Суть: неверная позиция курсора при инлайн-изображениях (iTerm image protocol).
- Класс: functional
- Приоритет: medium.

### Issue #6163 (6 коммент.) — Copy mode deselects when copying in tmux
- Суть: copy mode сбрасывает выделение при копировании внутри tmux.
- Класс: functional
- Приоритет: medium.

### Issue #6128 (6 коммент.) — Weztem crashing on text selection
- Суть: краш при выделении текста.
- Класс: crash
- Приоритет: high.

### Issue #5790 (6 коммент.) — WebGPU frontend not working with window opacity
- Суть: прозрачность окна не работает во фронтенде WebGPU (дублирует тему #6359).
- Класс: functional
- Приоритет: medium.

### Issue #5523 (6 коммент.) — chinese punctuation position render too high up
- Суть: неверная вертикальная позиция китайской пунктуации.
- Класс: visual
- Приоритет: medium.

### Issue #4992 (6 коммент.) — random artifacts when using opengl with intel iris xe graphics on windows
- Суть: случайные визуальные артефакты с Intel Iris Xe + OpenGL на Windows.
- Применимо: Windows — платформа в фокусе.
- Класс: visual
- Приоритет: medium.

### Issue #4825 (6 коммент.) — Crash with wgpu on Fedora 39
- Суть: краш wgpu-рендерера (общий код), триггер — Linux/Fedora.
- Класс: crash
- Приоритет: medium/high.

### Issue #4768 (6 коммент.) — No fonts contain glyphs for those codepoints: \u{1d4d2}
- Суть: не находится fallback-шрифт для некоторых кодпоинтов — связано с логикой фолбэка шрифтов.
- Применимо: пересекается с недавним коммитом по font fallback.
- Класс: visual
- Приоритет: medium.

### Issue #4467 (6 коммент.) — Wrong unicode display after font resize
- Суть: неверное отображение unicode после ресайза шрифта.
- Класс: visual
- Приоритет: medium.

### Issue #4412 (6 коммент.) — Background blur not working
- Суть: не работает размытие фона (macOS-фича).
- Класс: functional
- Приоритет: medium.

### Issue #3771 (6 коммент.) — Memory leak and high CPU usage with actions.RotatePanes with certain pane layouts in `onlyterm connect unix`
- Суть: утечка памяти и высокая нагрузка CPU при RotatePanes в определённых layout'ах, при подключении к unix-домену.
- Применимо: локальный mux (unix domain) сохранён.
- Класс: leak/perf
- Приоритет: high.

### Issue #2422 (6 коммент.) — Kitty image protocol fails to delete images after resize by placement id
- Суть: не удаляются картинки Kitty image protocol по placement id после ресайза.
- Класс: functional
- Приоритет: medium.

### Issue #7807 (5 коммент.) — Kitty Unicode Rendering Weird Behaviour
- Суть: странности рендера unicode в связке с kitty keyboard/graphics.
- Класс: visual
- Приоритет: medium.

### Issue #7302 (5 коммент.) — Cursor jumps one line up when at the bottom of the terminal window
- Суть: курсор прыгает на строку вверх у нижней границы терминала.
- Класс: visual
- Приоритет: medium.

### Issue #7249 (5 коммент.) — Segfault on macOS Tahoe (26.0)
- Суть: сегфолт на актуальной версии macOS.
- Класс: crash
- Приоритет: high.

### Issue #7159 (5 коммент.) — "bold dimmed" aka 1;2m aka starship hostname not rendering as bold
- Суть: неверная обработка комбинации SGR bold+dim.
- Применимо: общий код term (SGR).
- Класс: visual
- Приоритет: medium.

### Issue #7158 (5 коммент.) — The Onlyterm application crashes when changing the configuration while the application is running.
- Суть: краш при live-reload конфигурации.
- Класс: crash
- Приоритет: high.

### Issue #7132 (5 коммент.) — [colorscheme] rose-pine-moon does not have cursor selection highlight
- Суть: отсутствует поле выделения курсора в конкретной цветовой схеме — не собственно код-баг, скорее отсутствие defaults/данные схемы.
- Класс: functional
- Приоритет: low.

### Issue #7126 (5 коммент.) — Odd rendering of border corners.
- Суть: странный рендер углов рамки.
- Класс: visual
- Приоритет: medium.

### Issue #7117 (5 коммент.) — "Unrecognized tmux cc line" error for %unlinked-window-renamed (when using multiple sessions)
- Суть: ошибка парсинга protocol-строки tmux control mode.
- Класс: functional
- Приоритет: medium.

### Issue #7057 (5 коммент.) — Cmd-N when using tmux -CC creates a new tab rather than a new window
- Суть: Cmd-N создаёт вкладку вместо окна в tmux -CC.
- Класс: functional
- Приоритет: low/medium.

### Issue #6978 (5 коммент.) — Using onlyterm nightly + PowerShell 5.1 with window_background_opacity < 1.0 results in broken transparency
- Суть: сломанная прозрачность в связке nightly+PowerShell 5.1.
- Применимо: Windows — платформа в фокусе.
- Класс: visual
- Приоритет: medium.

### Issue #6869 (5 коммент.) — Buggy rendering when resizing panes
- Суть: глючный рендер при ресайзе панелей (повторяющаяся тема).
- Класс: visual
- Приоритет: medium.

### Issue #6853 (4 коммент.) — onlyterm sends a weird key sequence after quitting yazi over ssh
- Суть: неверная последовательность клавиш после выхода из yazi (обычный пользовательский ssh, не встроенный клиент).
- Класс: functional
- Приоритет: low/medium.

### Issue #6567 (5 коммент.) — Resizing/title bar issues with multi-monitor setup
- Суть: проблемы ресайза/title bar в мультимониторной конфигурации.
- Класс: functional
- Приоритет: medium.

### Issue #6543 (5 коммент.) — Scrollback rendering issue on SSHMUX + default unix domain
- Суть: (проверено тело) баг воспроизводится строго в связке unix-domain (default_domain) + вкладка к SSHMUX-домену.
- Применимо: N/A — репродукция требует домена SSHMUX, который удалён целиком; без него баг в описанном виде не воспроизводим (возможные скрытые проблемы в самом unix-domain коде не подтверждены этим issue).

### Issue #6436 (5 коммент.) — [Bug]: Noticable delay on split pane navigation
- Суть: заметная задержка при навигации между панелями.
- Класс: perf
- Приоритет: medium.

### Issue #5917 (5 коммент.) — onlyterm with custom config disables OSC52 copy to clipboard
- Суть: пользовательский конфиг неожиданно отключает OSC52.
- Класс: functional
- Приоритет: medium.

### Issue #5913 (5 коммент.) — Weird graphical rectangle artifacts on the terminal
- Суть: странные прямоугольные артефакты рендера.
- Класс: visual
- Приоритет: medium.

### Issue #5892 (5 коммент.) — Kitty image protocol visual glitches when updating placement location
- Суть: визуальные глюки Kitty image protocol при обновлении расположения.
- Класс: visual
- Приоритет: medium.

### Issue #5843 (5 коммент.) — Does scaling multiple times create a lot of white space?
- Суть: повторное масштабирование создаёт лишнее пустое пространство.
- Класс: visual
- Приоритет: low/medium.

### Issue #5831 (5 коммент.) — Prompt duplicating when resizing `bash.exe` under Windows
- Суть: дублирование промпта при ресайзе bash.exe на Windows.
- Применимо: Windows — платформа в фокусе.
- Класс: visual/functional
- Приоритет: medium.

### Issue #5819 (5 коммент.) — Overlay issue when tabs at bottom
- Суть: проблема оверлея при табах снизу.
- Класс: visual
- Приоритет: low/medium.

### Issue #5555 (5 коммент.) — Setting `macos_window_background_blur` above 0 leads to high GPU usage
- Суть: высокая нагрузка GPU при включении размытия фона на macOS.
- Класс: perf
- Приоритет: medium.

### Issue #5432 (5 коммент.) — Closing Tab with separate process running causes terminal to stop responding
- Суть: закрытие вкладки с работающим дочерним процессом вызывает зависание.
- Класс: hang
- Приоритет: high.

### Issue #5029 (5 коммент.) — Can't bind ALT|SHIFT on macOS
- Суть: невозможно забиндить ALT|SHIFT на macOS.
- Класс: functional
- Приоритет: low/medium.

### Issue #4967 (5 коммент.) — Caps lock as modifier triggers viewport scroll
- Суть: Caps Lock как модификатор неожиданно триггерит скролл.
- Класс: functional
- Приоритет: low/medium.

### Issue #4915 (5 коммент.) — Reduction in display number and size makes onlyterm unusable
- Суть: изменение числа/размера мониторов делает onlyterm неюзабельным.
- Класс: functional
- Приоритет: medium.

### Issue #4899 (5 коммент.) — onlyterm cli set-window-title does not change the displayed window title
- Суть: команда cli set-window-title не работает.
- Класс: functional
- Приоритет: medium.

### Issue #4857 (5 коммент.) — Inconsistent crashing with Tiling WM (sway) when resizing with neovim open
- Суть: нестабильный краш при ресайзе с neovim (ещё один экземпляр паттерна scale/resize-краша).
- Класс: crash
- Приоритет: medium/high.

### Issue #4785 (5 коммент.) — Onlyterm may send different keys than kitty with kitty protocol enabled
- Суть: расхождение с эталонной реализацией kitty keyboard protocol.
- Класс: functional
- Приоритет: medium.

### Issue #4488 (5 коммент.) — Plugin unsupported URL protocol error
- Суть: ошибка загрузки плагина по неподдерживаемому протоколу URL.
- Применимо: система плагинов сохранена, но мигрировала с mlua на rhai — нужна проверка актуальности под rhai.
- Класс: functional
- Приоритет: medium.

### Issue #4394 (5 коммент.) — karabiner rules not playing well (caps lock == escape key alone/control if chord)
- Суть: конфликт с ремаппером Karabiner (macOS) — может указывать на слишком строгую обработку модификаторов.
- Класс: functional
- Приоритет: low.

### Issue #4278 (5 коммент.) — Intermittent input lag on Windows 11 only with WebGpu front-end
- Суть: периодические задержки ввода на Windows 11 именно с WebGpu.
- Применимо: Windows — платформа в фокусе, WebGpu общий.
- Класс: perf
- Приоритет: medium.

### Issue #4259 (5 коммент.) — On french keyboard layout, `Alt-²` wrongly sends `Alt-bquote`
- Суть: неверное сопоставление клавиш для французской раскладки.
- Класс: functional
- Приоритет: low/medium.

### Issue #4133 (5 коммент.) — force_reverse_video_cursor is not applied when set on window:get_config_overrides
- Суть: config override не применяется для конкретной опции.
- Применимо: config API (актуально и под rhai).
- Класс: functional
- Приоритет: low/medium.

### Issue #4129 (5 коммент.) — Incorrect keycode for Del with enable_kitty_keyboard = true
- Суть: неверный код клавиши Del в kitty keyboard protocol.
- Класс: functional
- Приоритет: low/medium.

### Issue #3726 (5 коммент.) — Mouse pointer not shown when windows are maximized
- Суть: не отображается курсор мыши при развёрнутом окне.
- Класс: functional
- Приоритет: low/medium.

### Issue #3593 (5 коммент.) — \033[=0;1u not working as expected
- Суть: некорректная обработка escape-последовательности kitty keyboard protocol.
- Класс: functional
- Приоритет: low/medium.

### Issue #3498 (5 коммент.) — Editing file mixed with Chinese and English in neovim in zellij will eliminate texts
- Суть: потеря текста при смешанном CJK+ASCII вводе в neovim под zellij.
- Применимо: общий рендер/ширина CJK-символов.
- Класс: functional/visual
- Приоритет: medium.

### Issue #3396 (5 коммент.) — dragging window between monitors on macos is glitchy/laggy
- Суть: глючное/тормозное перетаскивание окна между мониторами на macOS (см. также #1983).
- Класс: visual/perf
- Приоритет: medium.

### Issue #2162 (5 коммент.) — Weird line wrapping behaviour
- Суть: странное поведение переноса строк — баг в ядре обработки текста.
- Класс: functional
- Приоритет: medium/high.

### Issue #7953 (4 коммент.) — Kitty image placement reposition + DECSTBM scroll crashes OnlyTerm (SIGABRT via quad-count blowup)
- Суть: точно диагностированный краш (SIGABRT) из-за переполнения количества quad'ов при репозиции картинки + скролл-регион (DECSTBM).
- Применимо: общий код рендера (буфер quad'ов), напрямую применимо к нашему рендер-пайплайну.
- Класс: crash
- Приоритет: high — чётко описанный root cause, легко воспроизводим.

### Issue #7898 (4 коммент.) — Do not send `\n` + EOF when unix pty is dropped
- Суть: некорректная последовательность байт при закрытии unix-pty.
- Класс: functional
- Приоритет: medium.

### Issue #7784 (4 коммент.) — Bracketed Paste "defanged" transformation is not thorough
- Суть: неполная санитизация bracketed paste (потенциально security-адjacent).
- Класс: functional
- Приоритет: medium.

### Issue #7753 (4 коммент.) — Very slow startup on windows 11
- Суть: очень медленный старт на Windows 11 (см. также #7782 с точным root cause).
- Применимо: Windows — платформа в фокусе.
- Класс: perf
- Приоритет: high.

### Issue #7388 (4 коммент.) — Complete freeze when switching windows on a mux server
- Суть: полное зависание при переключении окон на mux-сервере (локальный mux).
- Класс: hang
- Приоритет: high.

### Issue #7363 (4 коммент.) — Onlyterm using 22 GB of RAM?
- Суть: экстремальная утечка памяти (22 ГБ).
- Класс: leak
- Приоритет: high.

### Issue #7337 (4 коммент.) — Cursor somehow keep moving when changing workspace
- Суть: самопроизвольное движение курсора при смене workspace.
- Класс: functional
- Приоритет: low/medium.

### Issue #7291 (4 коммент.) — Seffault on MacOS Tahoe after resuming from the lock-screen
- Суть: сегфолт после выхода из lock screen на macOS.
- Класс: crash
- Приоритет: high.

### Issue #7285 (4 коммент.) — ERROR onlyterm_gui > Io error in BlobLease: Permission denied (os error 13)
- Суть: ошибка доступа к файлам кеша глифов (BlobLease) — повторяющаяся тема с #5422 и #6426.
- Применимо: общий код кеширования шрифтовых blob'ов.
- Класс: functional
- Приоритет: medium.

### Issue #7113 (4 коммент.) — Bottom gap visible even with zero window_padding
- Суть: видимый зазор снизу даже при нулевом паддинге (см. также #6834).
- Класс: visual
- Приоритет: low/medium.

### Issue #7060 (4 коммент.) — Odd character rendering with nerd font
- Суть: странный рендер символов nerd font.
- Класс: visual
- Приоритет: low/medium.

### Issue #6982 (3 коммент.) — Nightly: Up/down arrow keys stopped working
- Суть: регрессия — стрелки вверх/вниз перестали работать в nightly.
- Класс: functional
- Приоритет: high (регрессия базовой функциональности).

### Issue #6940 (4 коммент.) — Delete Key shows `~` symbol instead of deleting.
- Суть: клавиша Delete вставляет `~` вместо удаления.
- Класс: functional
- Приоритет: medium.

### Issue #6855 (3 коммент.) — Onlyterm uses a lot of CPU on Linux
- Суть: высокая нагрузка CPU на Linux (общая тема, см. #2665, #7275).
- Класс: perf
- Приоритет: medium/high.

### Issue #6824 (4 коммент.) — key_tables bindings not working when added a modifier
- Суть: биндинги в key_tables не работают при добавлении модификатора.
- Класс: functional
- Приоритет: medium.

### Issue #6766 (2 коммент.) — Links highlighted on wrong pane relative to other pane mouse position
- Суть: подсветка гиперссылки в неверной панели относительно позиции мыши в другой панели.
- Класс: functional
- Приоритет: low/medium.

### Issue #6684 (4 коммент.) — Color error during `apt update` in CJK kanguage
- Суть: ошибка цвета при выводе apt update с CJK-локалью.
- Класс: visual
- Приоритет: low/medium.

### Issue #6666 (4 коммент.) — Resizing pane when neovim is open and connected to a unix domain
- Суть: проблемы ресайза панели с neovim через unix-domain mux.
- Класс: functional
- Приоритет: medium.

### Issue #6662 (4 коммент.) — WebGPU stops from launching on Windows
- Суть: WebGPU перестаёт запускаться на Windows.
- Применимо: Windows — платформа в фокусе.
- Класс: crash
- Приоритет: medium/high.

### Issue #6371 (4 коммент.) — Slow when scrolling down a file while using nvim
- Суть: тормоза скролла в nvim (повторяющаяся тема).
- Класс: perf
- Приоритет: medium.

### Issue #6226 (4 коммент.) — run_child_process causes error in config but not in debug overlay
- Суть: несогласованное поведение функции конфига run_child_process в разных контекстах.
- Применимо: config API (rhai) — стоит перепроверить после миграции с mlua.
- Класс: functional
- Приоритет: medium.

### Issue #5849 (4 коммент.) — After enabling unix socket some tab titles are flashing
- Суть: мерцание заголовков вкладок после включения unix-сокета (mux).
- Класс: visual
- Приоритет: low/medium.

### Issue #5596 (4 коммент.) — shaping issue with 1f575,1f575,200d in that sequence
- Суть: баг шейпинга ZWJ-последовательности emoji.
- Применимо: наш шейпинг теперь rustybuzz — стоит перепроверить.
- Класс: visual
- Приоритет: low/medium.

### Issue #5579 (4 коммент.) — when environment variable ONLYTERM_CONFIG_FILE is specified, onlyterm.config_dir is not part of modules search path
- Суть: баг разрешения пути модулей конфигурации.
- Применимо: актуально и для модульной системы rhai.
- Класс: functional
- Приоритет: low/medium.

### Issue #5559 (4 коммент.) — Long delay updating GUI, periodic key repeats
- Суть: большая задержка обновления GUI при периодическом автоповторе.
- Класс: perf
- Приоритет: medium.

### Issue #5552 (4 коммент.) — Onlyterm changes current working directory on nvim lsp attach
- Суть: неверное отслеживание cwd (OSC7) при работе LSP в nvim.
- Класс: functional
- Приоритет: low/medium.

### Issue #5522 (4 коммент.) — `onlyterm imgcat` crashes when running in tmux
- Суть: краш imgcat внутри tmux.
- Класс: crash
- Приоритет: medium.

### Issue #5122 (4 коммент.) — Modifying font size using CTRL +/- modifies window size instead
- Суть: изменение размера шрифта меняет размер окна вместо количества колонок/строк (повторяющаяся тема).
- Класс: functional
- Приоритет: medium.

### Issue #5000 (4 коммент.) — `unicode_version` option not always set
- Суть: опция unicode_version не всегда выставляется (DECRQM/терминфо).
- Класс: functional
- Приоритет: low.

### Issue #4978 (4 коммент.) — Problem with modifier keys
- Суть: неуточнённая проблема с модификаторами.
- Класс: functional
- Приоритет: low.

### Issue #4887 (4 коммент.) — webgpu: Onlyterm titlebar cannot be hidden anymore
- Суть: регрессия — заголовок окна нельзя скрыть при WebGpu.
- Класс: functional
- Приоритет: medium.

### Issue #4878 (4 коммент.) — Panes can be resized to zero and negative sizes
- Суть: панели можно сжать до нуля/отрицательных размеров — edge case, потенциально ведущий к краш/паникам ниже по стеку.
- Класс: functional
- Приоритет: medium/high.

### Issue #4870 (4 коммент.) — Background Image not working
- Суть: не работает фоновое изображение.
- Класс: functional
- Приоритет: low/medium.

### Issue #4558 (4 коммент.) — Emojis are not rendered correctly (size and baseline)
- Суть: неверный размер/базовая линия emoji.
- Класс: visual
- Приоритет: medium.

### Issue #4502 (4 коммент.) — window_background_opacity does not work with WebGpu frontend after upgrading NVIDIA drivers
- Суть: регрессия прозрачности после апдейта драйверов NVIDIA с WebGpu.
- Класс: visual/functional
- Приоритет: medium.

### Issue #4459 (4 коммент.) — far2l interface is disturbed
- Суть: искажения при рендере TUI-приложения far2l — вопрос точности эмуляции терминала.
- Класс: visual/functional
- Приоритет: low.

### Issue #4358 (4 коммент.) — Onlyterm keeps switching keyboard layout
- Суть: самопроизвольное переключение раскладки клавиатуры.
- Класс: functional
- Приоритет: medium.

### Issue #3893 (4 коммент.) — `config.line_height` cause weird behaviour
- Суть: странности рендера при нестандартном line_height (повторяющаяся тема, см. #1957, #6785).
- Класс: visual
- Приоритет: medium.

### Issue #3886 (4 коммент.) — Blinking cursor is broken on transparent background
- Суть: мигающий курсор ломается при прозрачном фоне.
- Класс: visual
- Приоритет: low/medium.

### Issue #3841 (4 коммент.) — current_working_dir doesnot work on Windows
- Суть: не работает current_working_dir на Windows.
- Применимо: Windows — платформа в фокусе.
- Класс: functional
- Приоритет: medium.

### Issue #3809 (4 коммент.) — Dead-Keys on Ctrl + Alt + Shift (AltGr + Shift) not working
- Суть: не работают dead-keys в комбинации AltGr+Shift.
- Класс: functional
- Приоритет: low.

### Issue #3770 (4 коммент.) — Shift not respected when caps lock is active
- Суть: Shift игнорируется при активном Caps Lock.
- Класс: functional
- Приоритет: low/medium.

### Issue #3601 (4 коммент.) — Dead key followed by unicode char ignores the dead key
- Суть: dead-key игнорируется, если следом идёт unicode-символ.
- Класс: functional
- Приоритет: low.

### Issue #3598 (4 коммент.) — Window management buttons still persist
- Суть: кнопки управления окном не скрываются как ожидается.
- Класс: functional
- Приоритет: low.

### Issue #3511 (4 коммент.) — Split pane's cwd does not match source pane's cwd
- Суть: cwd нового сплита не наследуется от исходной панели.
- Класс: functional
- Приоритет: medium.

### Issue #3013 (4 коммент.) — panes argument to format-tab-title only contains zoomed pane
- Суть: неполный список панелей в API format-tab-title при zoom.
- Класс: functional
- Приоритет: low.

### Issue #2910 (4 коммент.) — Mouse selection with Shift inconsistent
- Суть: непоследовательное выделение мышью с Shift.
- Класс: functional
- Приоритет: low/medium.

### Issue #2723 (4 коммент.) — termwiz: default SGR 8 bit color encoding doesn't work in PowerShell
- Суть: баг кодирования 8-битного SGR-цвета в termwiz (общий core-крейт) при работе с PowerShell.
- Применимо: termwiz — общий крейт, используется везде.
- Класс: functional
- Приоритет: medium.

### Issue #2524 (4 коммент.) — FTXUI (tui library) canvas example runs slower than windows terminal
- Суть: заметный разрыв производительности рендера по сравнению с Windows Terminal.
- Класс: perf
- Приоритет: medium.

### Issue #2274 (4 коммент.) — Screen sharing over Discord causes display issues and crashes
- Суть: краш/визуальные проблемы при захвате экрана через Discord (взаимодействие с GPU screen capture).
- Класс: crash
- Приоритет: medium.

### Issue #1957 (4 коммент.) — Unexpected transformed braille when increasing or decreasing line_height
- Суть: искажение brail-символов при изменении line_height (тема line_height повторяется).
- Класс: visual
- Приоритет: medium.

### Issue #1908 (4 коммент.) — width of unicode chars affecting lines in tmux pane to the right
- Суть: неверный расчёт ширины unicode-символов, ломающий соседние панели в tmux.
- Применимо: общий код расчёта ширины unicode (term core).
- Класс: functional
- Приоритет: medium.

### Issue #1673 (4 коммент.) — Kitty image protocol does not display images on Windows
- Суть: не отображаются картинки Kitty image protocol на Windows.
- Применимо: Windows — платформа в фокусе.
- Класс: functional
- Приоритет: medium.

### Issue #1594 (4 коммент.) — Onlyterm serial uses 100% CPU
- Суть: 100% загрузка CPU в режиме serial-порта.
- Класс: perf
- Приоритет: medium.

### Issue #1494 (4 коммент.) — Cursor Color not working anymore
- Суть: регрессия — цвет курсора перестал применяться.
- Класс: functional
- Приоритет: medium.

### Issue #7932 (3 коммент.) — adjust_window_size_when_changing_font_size = false not working
- Суть: опция не соблюдается (тема font-size/window-size, см. #4250, #5122).
- Класс: functional
- Приоритет: medium.

### Issue #7782 (3 коммент.) — Extremely slow startup on Windows: 15-50 seconds delay in portable_pty::cmdbuilder and local gui-sock setup
- Суть: точно локализованная причина медленного старта на Windows — cmdbuilder + настройка gui-sock.
- Применимо: Windows — платформа в фокусе, чёткий root cause.
- Класс: perf
- Приоритет: high.

### Issue #7703 (3 коммент.) — Crashed when machine sleeps in full screen mode
- Суть: краш при уходе машины в сон в fullscreen.
- Класс: crash
- Приоритет: medium/high.

### Issue #7439 (3 коммент.) — Windows build creates gui-sock-* Unix socket files when launched via onlyterm start --cwd from Explorer context menu
- Суть: некорректное создание файлов unix-сокета при запуске из контекстного меню Explorer.
- Применимо: Windows — платформа в фокусе.
- Класс: functional
- Приоритет: low/medium.

### Issue #7437 (3 коммент.) — Kitty Keyboard doesn't restore properly
- Суть: состояние kitty keyboard protocol не восстанавливается корректно.
- Класс: functional
- Приоритет: low/medium.

### Issue #7436 (3 коммент.) — Nightly on Windows 11 launch fails with "LoadLibrary failed with error 126: The specified module could not be found."
- Суть: сбой запуска на Windows 11 из-за отсутствующей DLL.
- Применимо: Windows — платформа в фокусе.
- Класс: crash
- Приоритет: medium/high.

### Issue #7428 (3 коммент.) — Flickering on every keystroke on external monitor
- Суть: мерцание при каждом нажатии клавиши на внешнем мониторе.
- Класс: visual
- Приоритет: medium.

### Issue #7408 (3 коммент.) — Screen dimensions are ignored on new windows
- Суть: игнорируются размеры экрана для новых окон.
- Класс: functional
- Приоритет: medium.

### Issue #7240 (3 коммент.) — Apple Color Emoji too small
- Суть: слишком мелкий рендер Apple Color Emoji.
- Класс: visual
- Приоритет: low/medium.

### Issue #7230 (3 коммент.) — onlyterm gets into a high-event/high-power-consumption state until all windows are closed
- Суть: приложение застревает в состоянии высокого энергопотребления.
- Класс: perf
- Приоритет: medium/high.

### Issue #7209 (3 коммент.) — Incorrect keyboard layout on WSL2
- Суть: неверная раскладка клавиатуры под WSL2.
- Применимо: WSL — Windows-смежная фича.
- Класс: functional
- Приоритет: low/medium.

### Issue #7097 (3 коммент.) — kitty keyboard state not propagated over mux connection
- Суть: состояние kitty keyboard protocol не передаётся через mux-соединение (локальный mux).
- Класс: functional
- Приоритет: low/medium.

### Issue #6885 (3 коммент.) — window going crazy (text flickering, tabs switching on their own) when reconnecting to multiplexer shared with Linux
- Суть: серьёзные визуальные глюки/гонка при переподключении к мультиплексору (общий mux-код).
- Класс: visual/functional
- Приоритет: medium/high.

### Issue #6736 (3 коммент.) — When searching for Chinese, sometimes there may be problems with the search box being misplaced and unable to locate the search results correctly
- Суть: неверное позиционирование поиска для китайского текста.
- Класс: functional
- Приоритет: low/medium.

### Issue #6415 (3 коммент.) — Multiplexer / mux screen redraw rendering issue when scrolling in LESS / man pages
- Суть: проблема перерисовки при скролле в less/man через mux.
- Класс: functional
- Приоритет: medium.

### Issue #6233 (3 коммент.) — Opening new window from Gnome launcher crashes all windows on 200% scaling
- Суть: краш всех окон при масштабе 200% (ещё один экземпляр scale-краш паттерна).
- Класс: crash
- Приоритет: medium.

### Issue #6187 (3 коммент.) — Invisible window after maximizing
- Суть: невидимое окно после разворачивания.
- Класс: functional
- Приоритет: medium.

### Issue #6145 (3 коммент.) — --new-tab option in onlyterm connect not working
- Суть: не работает опция --new-tab (родственно #6179).
- Класс: functional
- Приоритет: low/medium.

### Issue #5835 (3 коммент.) — Almost no UI rendered
- Суть: почти ничего не рендерится (тяжёлый визуальный сбой, детали неясны).
- Класс: crash/visual
- Приоритет: medium.

### Issue #5453 (3 коммент.) — Custom DPI in config is ignored after first window resize
- Суть: заданный вручную DPI теряется после первого ресайза.
- Класс: functional
- Приоритет: medium.

### Issue #5422 (3 коммент.) — Removal of BlobLeases halts rendering of entire terminal
- Суть: удаление BlobLease полностью останавливает рендер терминала (повторяющаяся тема кеша шрифтовых blob'ов, см. #7285, #6426).
- Применимо: общий код кеширования шрифтовых glyph-blob'ов.
- Класс: hang
- Приоритет: high.

### Issue #5357 (3 коммент.) — High CPU usage and laggy on Fedora 40
- Суть: высокая нагрузка CPU и тормоза на Linux (общая тема).
- Класс: perf
- Приоритет: medium.

### Issue #5280 (3 коммент.) — An Investigation into Unresponsiveness and Performance Regression in OnlyTerm related to Font shaping
- Суть: детальное расследование деградации отзывчивости, связанной с шейпингом шрифтов.
- Применимо: напрямую относится к нашему шейпинг-стеку (rustybuzz), ценный источник root cause.
- Класс: perf
- Приоритет: high.

### Issue #5234 (3 коммент.) — Choppy Scrolling in Neovim
- Суть: рваный скролл в Neovim (повторяющаяся тема).
- Класс: perf
- Приоритет: medium.

### Issue #5200 (3 коммент.) — onlyterm Multiplexing connect server error.
- Суть: ошибка подключения к mux-серверу (детали не ясны).
- Класс: functional
- Приоритет: low/medium.

### Issue #5117 (3 коммент.) — Some panes don't resize properly when reattaching to a domain
- Суть: неверный ресайз некоторых панелей при переподключении к домену.
- Класс: functional
- Приоритет: medium.

### Issue #4944 (3 коммент.) — Onlyterm freezes while spawning in a nested X session
- Суть: зависание при запуске во вложенной X-сессии (Xephyr).
- Класс: hang
- Приоритет: medium.

### Issue #4611 (3 коммент.) — Onlyterm sending extra characters to vim
- Суть: лишние символы отправляются в vim.
- Класс: functional
- Приоритет: medium.

### Issue #4608 (3 коммент.) — Dubious interaction between search and copy mode when search is not empty
- Суть: сомнительное взаимодействие поиска и copy mode.
- Класс: functional
- Приоритет: low.

### Issue #4549 (3 коммент.) — first window is maximized, the others are not
- Суть: только первое окно получает состояние maximized.
- Класс: functional
- Приоритет: low/medium.

### Issue #4521 (3 коммент.) — Windows nightly responds '0' (mode not recognized) to DECRQM 2027
- Суть: неверный ответ на DECRQM для режима Unicode Core (2027).
- Применимо: общий код term (режимы DECRQM).
- Класс: functional
- Приоритет: low/medium.

### Issue #4447 (3 коммент.) — Weird Sonoma start window positions
- Суть: странное позиционирование окна при старте на macOS Sonoma.
- Класс: functional
- Приоритет: low.

### Issue #4435 (3 коммент.) — Color schemes placed in alternative location can't be found
- Суть: цветовые схемы в альтернативном расположении не находятся.
- Класс: functional
- Приоритет: low/medium.

### Issue #4323 (3 коммент.) — Glitchy rendering when resizing
- Суть: глючный рендер при ресайзе (повторяющаяся тема).
- Класс: visual
- Приоритет: medium.

### Issue #4234 (3 коммент.) — Capital Cyrillic letters are turned into "~42" sequences
- Суть: заглавные кириллические буквы кодируются как escape-последовательности "~42" — актуально для русскоязычных пользователей.
- Класс: functional
- Приоритет: medium.

### Issue #4126 (2 коммент.) — onlyterm colors are somehow dimmed
- Суть: приглушённые цвета (гамма/цветовой профиль).
- Класс: visual
- Приоритет: low.

### Issue #4112 (3 коммент.) — mpv --vo=kitty constantly flashes status message by default
- Суть: постоянное мерцание статусной строки kitty graphics protocol.
- Класс: functional
- Приоритет: low.

### Issue #4110 (3 коммент.) — Input lag on HiDPI 4K when maximizing window
- Суть: задержка ввода на HiDPI 4K при разворачивании окна.
- Класс: perf
- Приоритет: medium.

### Issue #4041 (3 коммент.) — Onlyterm -> MUX -> tmux => garbled text
- Суть: искажение текста в цепочке mux → tmux.
- Класс: visual/functional
- Приоритет: medium/high.

### Issue #4029 (3 коммент.) — Running `onlyterm imgcat` too rapidly on any large enough image causes errors
- Суть: ошибки при частом вызове imgcat с большими изображениями (вероятно гонка/лимит ресурсов).
- Класс: functional
- Приоритет: medium.

### Issue #3934 (3 коммент.) — Injected keypresses of modifier keys result in `^@`
- Суть: программно инжектированные модификаторы дают неверный байт (актуально для accessibility/автоматизации, см. также #7174).
- Класс: functional
- Приоритет: medium.

### Issue #3732 (3 коммент.) — NFD strings are always NFC-normalized regardless of `normalize_output_to_unicode_nfc = false`
- Суть: опция нормализации unicode игнорируется.
- Класс: functional
- Приоритет: medium.

### Issue #3660 (3 коммент.) — `KP_` (numeric keypad) keys not working
- Суть: не работают клавиши цифровой клавиатуры.
- Класс: functional
- Приоритет: medium.

### Issue #3575 (3 коммент.) — Font rendering on external monitor is not correct (macOS)
- Суть: неверный рендер шрифта на внешнем мониторе (macOS).
- Класс: visual
- Приоритет: medium.

### Issue #3320 (3 коммент.) — Some emojis appear to overlap each other
- Суть: наложение emoji друг на друга.
- Класс: visual
- Приоритет: low.

### Issue #3276 (3 коммент.) — High CPU usage when hovering over tabs
- Суть: высокая нагрузка CPU при наведении на вкладки (лишний цикл перерисовки).
- Класс: perf
- Приоритет: medium.

### Issue #2984 (3 коммент.) — Switching workspaces opens windows when it shouldn't
- Суть: неожиданное открытие окон при переключении workspace.
- Класс: functional
- Приоритет: medium.

### Issue #2907 (3 коммент.) — Onlyterm window glitches and lags behind cursor when moved between 2 windows on 150% window scale
- Суть: визуальное отставание/глюки при 150% масштабе (снова тема scale-багов).
- Класс: visual
- Приоритет: medium.

### Issue #2902 (3 коммент.) — onlyterm imgcat with vifm
- Суть: проблема совместимости imgcat с vifm.
- Класс: functional
- Приоритет: low.

### Issue #2796 (3 коммент.) — After a while, windows lose their title bar and resize on activation
- Суть: со временем окна теряют title bar и меняют размер при активации (повторяющаяся тема потери декораций).
- Класс: functional
- Приоритет: medium.

### Issue #2560 (3 коммент.) — Long tab titles with runs of 2 or more consecutive spaces can overflow the tab bounds
- Суть: переполнение границ вкладки при длинных заголовках с пробелами (баг измерения текста).
- Класс: visual
- Приоритет: low.

### Issue #2511 (3 коммент.) — Termwiz bugs
- Суть: (проверено тело) `pool_input`/парсер клавиатурного ввода в termwiz шлёт модификатор и символ двумя отдельными событиями вместо одного объединённого (Shift+A → два события вместо KeyEvent{Char('A'), SHIFT}); аналогично для Ctrl+j/h.
- Применимо: termwiz — общий core-крейт, используется во всех бэкендах.
- Класс: functional
- Приоритет: medium/high.

### Issue #2196 (3 коммент.) — win32 input mode: not processing Ctrl + Alt keyboard combinations properly
- Суть: неверная обработка Ctrl+Alt в win32-input-mode.
- Применимо: Windows — платформа в фокусе.
- Класс: functional
- Приоритет: medium.

### Issue #1906 (3 коммент.) — reconcile normalizing SHIFT modifier in key events on X11
- Суть: нормализация модификатора SHIFT в клавиатурных событиях на X11 (общий код X11-бэкенда, не WM-специфика).
- Класс: functional
- Приоритет: low/medium.

### Issue #1735 (3 коммент.) — High CPU Usage when using software rendering
- Суть: высокая нагрузка CPU в режиме программного рендера.
- Применимо: напрямую относится к нашему CPU-рендер пути (tiny-skia).
- Класс: perf
- Приоритет: medium/high.

### Issue #7847 (2 коммент.) — Crash when using pane:move_to_new_window when single pane in window
- Суть: краш при вызове API-функции на единственной панели в окне.
- Класс: crash
- Приоритет: medium.

### Issue #7527 (2 коммент.) — mux: Unbounded PDU memory allocation causes OOM crashes and stack overflow
- Суть: неограниченная аллокация памяти под PDU в mux-протоколе приводит к OOM/переполнению стека — DoS-класс баг.
- Применимо: общий код mux-протокола (используется локальным unix-domain mux).
- Класс: crash
- Приоритет: high.

### Issue #7524 (2 коммент.) — detect_password_input doesn't display lock icon
- Суть: не отображается иконка замка при детекте ввода пароля.
- Класс: functional
- Приоритет: low.

### Issue #7490 (2 коммент.) — OnlyTerm silently ignores unknown config keys, which leads to confusing behavior
- Суть: неизвестные ключи конфига молча игнорируются без предупреждения.
- Применимо: актуально для конфига на rhai.
- Класс: functional
- Приоритет: low/medium.

### Issue #7389 (2 коммент.) — Cursor flickers on helix editor when some UI elements are drawn
- Суть: мерцание курсора при отрисовке некоторых UI-элементов helix.
- Класс: visual
- Приоритет: low.

### Issue #7386 (2 коммент.) — ALT + Arrow doesnt send anything
- Суть: комбинация Alt+стрелка не отправляет ничего.
- Класс: functional
- Приоритет: low/medium.

### Issue #7358 (2 коммент.) — Panicked when using IME completion
- Суть: паника при завершении ввода через IME.
- Класс: crash
- Приоритет: medium/high.

### Issue #7322 (2 коммент.) — Terminal control issues when window is near right edge
- Суть: проблемы управления окном у правого края экрана.
- Класс: functional
- Приоритет: low.

### Issue #7264 (2 коммент.) — Windows client crashes while spawning new session on mux server
- Суть: краш Windows-клиента при спавне сессии на mux-сервере.
- Применимо: Windows — платформа в фокусе.
- Класс: crash
- Приоритет: medium/high.

### Issue #7257 (2 коммент.) — `Option+Cmd+H` shortcut causes unresponsiveness instead of hiding other applications
- Суть: зависание вместо ожидаемого системного действия (скрытие приложений) на macOS.
- Класс: hang
- Приоритет: medium.

### Issue #7255 (2 коммент.) — Sluggish input after a while when running on Tahoe
- Суть: замедление ввода со временем работы на новой macOS.
- Класс: perf
- Приоритет: medium.

### Issue #7239 (2 коммент.) — Emacs sshx freeze in OnlyTerm
- Суть: зависание при работе emacs (в т.ч. через пользовательский ssh) — общий код обработки TUI-вывода.
- Класс: hang
- Приоритет: medium.

### Issue #7218 (2 коммент.) — Pane sometimes seems to disconnect from underlying output
- Суть: панель иногда перестаёт получать вывод от процесса — потенциально серьёзный функциональный сбой.
- Класс: hang/functional
- Приоритет: medium/high.

### Issue #7215 (2 коммент.) — macOS: onlyterm.mux.spawn_window not functioning as expected
- Суть: некорректная работа config API spawn_window на macOS.
- Класс: functional
- Приоритет: low.

### Issue #7197 (2 коммент.) — Left status bar overlaps with window buttons with `"INTEGRATED_BUTTONS"`
- Суть: наложение статус-бара на кнопки в общей фиче Integrated Buttons.
- Класс: visual
- Приоритет: low/medium.

### Issue #7164 (2 коммент.) — Unable to rebind shift+enter to terminal newline on WSL
- Суть: невозможность перебиндить Shift+Enter на WSL.
- Класс: functional
- Приоритет: low.

### Issue #7163 (2 коммент.) — Crash on Ctrl^N (new window) on macOS
- Суть: краш при базовом действии "новое окно" на macOS.
- Класс: crash
- Приоритет: high.

### Issue #7135 (2 коммент.) — nightly build: clipboard integration is not working in copy mode in WSL2
- Суть: не работает буфер обмена в copy mode под WSL2.
- Класс: functional
- Приоритет: low/medium.

### Issue #7134 (2 коммент.) — nightly build: title bar is not visible on WSL2 or GNOME (Ubuntu 25.04)
- Суть: невидимая title bar на WSL2 (актуально)/Gnome (не в фокусе).
- Класс: functional
- Приоритет: low/medium.

### Issue #7042 (2 коммент.) — CharSelect's doesn't render a drop down for me.
- Суть: не рендерится выпадающий список в CharSelect UI.
- Класс: functional
- Приоритет: low.

### Issue #7039 (2 коммент.) — display area not use space shrinked by using windows small taskbar buttions
- Суть: область отображения не учитывает изменение размера панели задач Windows.
- Класс: functional
- Приоритет: low.

### Issue #7036 (2 коммент.) — Stray '\' character appears after TUI app (yazi) exits on Windows with PowerShell
- Суть: лишний символ после выхода из TUI-приложения (некорректное восстановление состояния терминала).
- Класс: functional
- Приоритет: low/medium.

### Issue #6994 (2 коммент.) — Ctrl+Shift+, not generating proper escape sequence with Kitty keyboard protocol
- Суть: неверная escape-последовательность для Ctrl+Shift+, в kitty protocol.
- Класс: functional
- Приоритет: low.

### Issue #6988 (2 коммент.) — Title bar does not change back to Onlyterm and stays as program name
- Суть: динамический заголовок не сбрасывается обратно.
- Класс: functional
- Приоритет: low.

### Issue #6949 (2 коммент.) — onlyterm.default_wsl_domains() do not work.
- Суть: не работает функция конфига для WSL-доменов.
- Применимо: WSL — Windows-смежная фича.
- Класс: functional
- Приоритет: low/medium.

### Issue #6936 (2 коммент.) — Under macOS, `onlyterm.gui.screens()` does not work
- Суть: не работает config API screens() на macOS.
- Класс: functional
- Приоритет: low.

### Issue #6912 (2 коммент.) — VS16 doesn't seem to set the glyph width correctly
- Суть: variation selector-16 не корректно меняет ширину глифа (эмодзи-презентация).
- Класс: functional
- Приоритет: medium.

### Issue #6863 (2 коммент.) — OnlyTerm window flashing
- Суть: мерцание окна при некотором событии.
- Класс: visual
- Приоритет: low/medium.

### Issue #6851 (2 коммент.) — window padding is not applied to a splitted pane
- Суть: паддинг окна не применяется к сплит-панели.
- Класс: visual
- Приоритет: low/medium.

### Issue #6834 (2 коммент.) — Bottom line hidden when maximized
- Суть: нижняя строка скрыта при развороте окна (см. также #7113).
- Класс: visual
- Приоритет: low/medium.

### Issue #6791 (2 коммент.) — Arabic ligature takes two cells when it should take one cell.
- Суть: неверная ширина для арабской лигатуры (RTL text shaping).
- Класс: functional
- Приоритет: medium.

### Issue #6787 (1 коммент.) — Unicode combining characters don't render when input directly
- Суть: не рендерятся составные unicode-символы при прямом вводе.
- Класс: functional
- Приоритет: low/medium.

### Issue #6786 (2 коммент.) — Stuck when switching input method using im-select
- Суть: зависание при переключении IME через im-select.
- Класс: hang
- Приоритет: medium.

### Issue #6785 (2 коммент.) — Text rendering and character baseline inconsistent when changing line_height
- Суть: непоследовательный рендер базовой линии при изменении line_height (повтор темы #1957, #3893).
- Класс: visual
- Приоритет: low/medium.

### Issue #6781 (2 коммент.) — Images do not render in WSL with OnlyTerm (no size information available)
- Суть: изображения не рендерятся в WSL (нет информации о размере).
- Применимо: WSL — Windows-смежная фича.
- Класс: functional
- Приоритет: low/medium.

### Issue #6764 (2 коммент.) — tmux -CC mouse reporting issue
- Суть: проблема репортинга мыши в tmux control mode.
- Класс: functional
- Приоритет: low.

### Issue #6676 (2 коммент.) — Fullscreen OnlyTerm shows windows title bar momentarily when switching apps
- Суть: кратковременное появление title bar в fullscreen на Windows при переключении приложений.
- Применимо: Windows — платформа в фокусе.
- Класс: visual
- Приоритет: low.

### Issue #6591 (2 коммент.) — The name of the Command Palette Item is not right
- Суть: неверное название пункта в Command Palette (косметический баг).
- Класс: functional
- Приоритет: low.

### Issue #6578 (2 коммент.) — client side decorations shown when TITLE is not present in window_decorations (nixos-24.11)
- Суть: похоже на тот же корень, что и #6920/#3936 — неверный парсинг флагов декораций окна.
- Применимо: вероятно общий код парсинга конфигурации декораций.
- Класс: functional
- Приоритет: medium.

### Issue #6499 (2 коммент.) — `onlyterm start --` or `onlyterm -e` panic if `PATHEXT` has an empty entry (`;;`)
- Суть: паника при парсинге PATHEXT с пустым элементом — чётко описанный root cause, легко фиксится.
- Применимо: Windows — платформа в фокусе.
- Класс: crash
- Приоритет: medium/high.

### Issue #6489 (2 коммент.) — Immediate crash after upgrade to macOS 15.2 when setting large initial_{rows,cols}
- Суть: краш при больших значениях initial_rows/cols после апдейта macOS.
- Класс: crash
- Приоритет: high.

### Issue #6426 (2 коммент.) — %APPDATA%/local/wezterm/wezterm-blob-lease-* directory taking up too much space (15 GB at one point)
- Суть: разрастание дискового кеша BlobLease до 15 ГБ (повторяющаяся тема, см. #7285, #5422).
- Применимо: общий код кеширования шрифтовых glyph-blob'ов — похоже на реальную утечку/отсутствие очистки.
- Класс: leak
- Приоритет: high.

### Issue #6397 (2 коммент.) — Issues rotating panes when connected to mux-server
- Суть: проблемы при вращении панелей на mux-сервере.
- Класс: functional
- Приоритет: low/medium.

### Issue #6335 (2 коммент.) — background with parallax: black horizontal line between tiled images
- Суть: чёрная полоса между тайловыми фоновыми изображениями (parallax).
- Класс: visual
- Приоритет: low.

### Issue #6332 (2 коммент.) — Uppercase Greek letters are lowercase in Kitty mode 9
- Суть: неверный регистр греческих букв в kitty mode 9.
- Класс: functional
- Приоритет: low.

### Issue #6330 (2 коммент.) — update-status stops being called after exiting from shell in the last pane of a workspace
- Суть: перестаёт вызываться хук update-status (актуально и для rhai event hooks).
- Класс: functional
- Приоритет: low/medium.

### Issue #6319 (2 коммент.) — Quick Select Mode includes tokens surrounding a URL
- Суть: неверные границы токенов в Quick Select для URL.
- Класс: functional
- Приоритет: low.

### Issue #6309 (2 коммент.) — Window size changes when Mac awakes from sleep with certain configurations
- Суть: непрошеный ресайз при выходе из сна на macOS (повтор темы #4633).
- Класс: functional
- Приоритет: low/medium.

### Issue #6287 (2 коммент.) — password number only accepted with num pad
- Суть: ввод цифр пароля принимается только с numpad.
- Класс: functional
- Приоритет: low.

### Issue #6223 (2 коммент.) — When the titlebar is disabled and the window is maximized, the taskbar, which is automatically hidden, does not appear
- Суть: авто-скрытая панель задач Windows не появляется при развороте окна без titlebar.
- Применимо: Windows — платформа в фокусе.
- Класс: functional
- Приоритет: low.

### Issue #6218 (2 коммент.) — cwd flag not working
- Суть: не работает флаг cwd в cli.
- Класс: functional
- Приоритет: low/medium.

### Issue #6202 (2 коммент.) — `onlyterm start` launches in background on macOS
- Суть: запуск в фоне вместо переднего плана на macOS.
- Класс: functional
- Приоритет: low/medium.

### Issue #6125 (2 коммент.) — Strange terminal output in WSL on onlyterm with tmux using yazi
- Суть: искажённый вывод в специфичной комбинации WSL+tmux+yazi.
- Класс: functional
- Приоритет: low.

### Issue #6094 (2 коммент.) — perform_action of CloseCurrentTab can crash onlyterm in certain sequences/timings
- Суть: краш при определённой последовательности/тайминге вызова CloseCurrentTab — гонка.
- Класс: crash
- Приоритет: medium/high.

### Issue #6077 (1 коммент.) — onlyterm segfault on macbook air m2
- Суть: сегфолт на Apple Silicon (M2).
- Класс: crash
- Приоритет: medium/high.

### Issue #6052 (2 коммент.) — resizing window does not resize panes proportionally
- Суть: непропорциональный ресайз панелей при ресайзе окна.
- Класс: functional
- Приоритет: low/medium.

### Issue #6049 (2 коммент.) — in non-local domain, "move pane" instead clones the pane
- Суть: "переместить панель" в не-локальном домене вместо этого клонирует её — потеря/дублирование данных.
- Класс: functional
- Приоритет: medium.

### Issue #6006 (2 коммент.) — Ctrl+Scrolllock crashes Onlyterm
- Суть: конкретное сочетание клавиш стабильно вызывает краш — легко воспроизводимо.
- Класс: crash
- Приоритет: medium/high.

### Issue #5986 (2 коммент.) — 1337;SetUserVar is output each time a shell command runs in Neovim's builtin terminal launched from tmux
- Суть: служебная последовательность "протекает" в вывод (повтор темы #5007).
- Класс: functional
- Приоритет: low.

### Issue #5920 (2 коммент.) — audible_bell = "Disabled" - not working on windows?
- Суть: опция отключения звукового сигнала не работает на Windows.
- Применимо: Windows — платформа в фокусе.
- Класс: functional
- Приоритет: low.

### Issue #5832 (2 коммент.) — User vars become unset after detach and re-attach to multiplexer Unix domain
- Суть: сброс пользовательских переменных после detach/reattach к unix-domain mux.
- Класс: functional
- Приоритет: low/medium.

### Issue #5793 (2 коммент.) — Clipboard in the terminal tab lags behind system's clipboard
- Суть: рассинхронизация буфера обмена с системным.
- Класс: functional
- Приоритет: low/medium.

### Issue #5745 (2 коммент.) — Onlyterm freezes after being resized
- Суть: зависание после ресайза (повтор темы зависаний при ресайзе).
- Класс: hang
- Приоритет: high.

### Issue #5741 (2 коммент.) — Can't ignore F20 key
- Суть: невозможно игнорировать клавишу F20.
- Класс: functional
- Приоритет: low.

### Issue #5570 (2 коммент.) — dim effect from helix editor is showing bright in onlyterm when combined with reversed colour effect
- Суть: неверная комбинация SGR dim+reverse.
- Класс: visual
- Приоритет: low.

### Issue #5478 (2 коммент.) — Colors not picked up by escape codes when colors are set from event
- Суть: цвета, заданные из rhai/lua-события, не подхватываются escape-кодами.
- Применимо: событийный API конфигурации (актуально под rhai).
- Класс: functional
- Приоритет: low/medium.

### Issue #5509 (2 коммент.) — onlyterm can't not use Ctrl+D to disconnect remote ssh session which using zsh
- Суть: Ctrl+D не обрабатывается как ожидается при работе внутри пользовательской ssh-сессии (не наш удалённый клиент, а обработка сигналов/EOF в целом).
- Класс: functional
- Приоритет: low.

### Issue #5455 (2 коммент.) — tmux crashes when using onlyterm and open nvim on remote computer via SSH
- Суть: краш самого tmux, вызванный поведением onlyterm (tmux -CC интеграция) при работе с nvim через обычный ssh пользователя.
- Класс: functional/crash
- Приоритет: medium.

### Issue #6228 (7 коммент.) — Regression in macOS Sequoia
- Суть: (проверено тело) регрессия ввода/рендера половинчатых катакана-глифов через CharSelect на macOS Sequoia — глифы не показываются, сильно лагает; воспроизводится и без конфига.
- Применимо: macOS — платформа в фокусе, IME/шрифтовой рендер.
- Класс: visual/functional
- Приоритет: medium.

### Issue #6882 (3 коммент.) — Navigation keys broken out of the box
- Суть: базовые клавиши навигации не работают "из коробки" (мало деталей, но серьёзный симптом).
- Класс: functional
- Приоритет: medium.

### Issue #7546 (4 коммент.) — Variable font (Berkeley Mono Variable) displaying as italic/oblique
- Суть: неверный выбор оси variable-шрифта — рендерится как italic вместо normal.
- Применимо: наш шрифтовой стек (rustybuzz/swash) обрабатывает variable fonts — стоит перепроверить обработку осей.
- Класс: visual
- Приоритет: medium.

### Issue #7794 (4 коммент.) — RightAlt (AltGr) keycode 113 incorrectly mapped to LeftArrow on French (fr) X11 keyboard layout
- Суть: неверное сопоставление кода клавиши AltGr на французской раскладке X11.
- Применимо: общий код X11-бэкенда (не WM-специфика), таблица кодов клавиш.
- Класс: functional
- Приоритет: low/medium.

### Issue #7017 (3 коммент.) — Failure to update or redraw display on WebGpu with nvidia 575.57.08
- Суть: сбой перерисовки на WebGpu с конкретной версией драйвера NVIDIA.
- Применимо: WebGpu — общий фронтенд.
- Класс: visual
- Приоритет: low/medium.

### Issue #7410 (3 коммент.) — Redundant links are only highlighted when the mouse is over the topmost one
- Суть: подсветка гиперссылки работает только для верхней панели при перекрытии.
- Класс: functional
- Приоритет: low.

### Issue #7419 (3 коммент.) — Visual Text Jump Bug?
- Суть: текст "прыгает" визуально (мало деталей).
- Класс: visual
- Приоритет: low.

### Issue #7212 (3 коммент.) — Cannot type "ç" using dead keys in OnlyTerm
- Суть: невозможность ввести "ç" через dead-keys.
- Класс: functional
- Приоритет: low.

### Issue #6776 (6 коммент.) — Starship not work on a Linux machine which install onlyterm via ssh
- Суть: starship prompt не работает корректно — вероятно проблема определения возможностей терминала (terminfo/capability detection).
- Класс: functional
- Приоритет: low/medium.

### Issue #6728 (3 коммент.) — Under MacOS and two monitors and onlyterm is full screen, sometimes after obtaining focus, the cursor is still hollow
- Суть: курсор остаётся "полым" (unfocused-стиль) после получения фокуса, macOS.
- Класс: visual
- Приоритет: low.

### Issue #6518 (5 коммент.) — NEO keyboard layout, right modifier 3 doesn't work OOTB
- Суть: не работает третий модификатор немецкой раскладки NEO "из коробки".
- Класс: functional
- Приоритет: low.

### Issue #6303 (3 коммент.) — OnlyTerm behaves differently when launched via a shortcut
- Суть: разное поведение при запуске через ярлык vs напрямую (разное окружение/аргументы).
- Класс: functional
- Приоритет: low.

### Issue #6150 (3 коммент.) — Cannot get correct numbers in emacs(terminal) when using programmer's Dvorak layout
- Суть: неверные цифры в emacs при раскладке Programmer Dvorak.
- Класс: functional
- Приоритет: low.

### Issue #6115 (5 коммент.) — Split vertical has incorrect hotkey hint in menu
- Суть: неверная подсказка горячей клавиши в меню для Split vertical (косметический баг).
- Класс: functional
- Приоритет: low.

### Issue #6112 (3 коммент.) — MacOS - CapsLock, Shift, Control, fn, option, cmd not logged by debug view
- Суть: debug-оверлей не логирует часть модификаторов на macOS.
- Класс: functional
- Приоритет: low.

### Issue #6087 (3 коммент.) — Cannot insert numbers (shift + ") in Kakoune (sent text looks messed up)
- Суть: неверная отправка символов при Shift+" в конкретном редакторе — общая логика кодирования клавиш.
- Класс: functional
- Приоритет: low.

### Issue #5516 (3 коммент.) — Cursor doesn't blink in tmux session
- Суть: не мигает курсор в сессии tmux.
- Класс: visual
- Приоритет: low.

### Issue #5491 (3 коммент.) — ssh causes issues with TUI apps drawing correctly
- Суть: проблемы отрисовки TUI-приложений в пользовательских ssh-сессиях (не наш удалённый клиент — рендер терминала в целом).
- Класс: functional
- Приоритет: low/medium.

### Issue #5460 (3 коммент.) — Emoji is incorrectly rendered
- Суть: неверный рендер emoji (общая тема).
- Класс: visual
- Приоритет: low.

### Issue #5370 (4 коммент.) — CygwinBash+ssh(145msping)+tmux -> Super slow tmux pane resize
- Суть: очень медленный ресайз панели tmux в специфичном Windows/Cygwin+ssh+tmux стеке.
- Класс: perf
- Приоритет: low/medium.

### Issue #5158 (4 коммент.) — thin line when opacity and shadow enabled
- Суть: тонкая паразитная линия при включённых opacity+shadow.
- Класс: visual
- Приоритет: low.

### Issue #5098 (6 коммент.) — When doing `onlyterm start` with `--config-file`, window opens behind main window
- Суть: окно открывается позади основного при указании --config-file.
- Класс: functional
- Приоритет: low/medium.

### Issue #5089 (3 коммент.) — `prefer_to_spawn_tabs` doesn't work
- Суть: опция prefer_to_spawn_tabs не соблюдается.
- Класс: functional
- Приоритет: low.

### Issue #4925 (3 коммент.) — MacOS fullscreen overide opacity
- Суть: переопределение прозрачности в fullscreen на macOS работает неверно.
- Класс: visual
- Приоритет: low.

## Неважное — feature requests / окружение-специфичные / нерелевантные (список без анализа)

#2788 Removing title bar not working correctly with tile manager
#5442 Pi5/Wayland WebGpu Display is Garbled
#6877 Cant use onlyterm for some reason
#6953 Seems Onlyterm mouse streak is wrong, maybe affected by Deskflow

#5074 Winget installation flagged as a trojan by windows security
#3958 OnlyTerm won't launch on Chromebook
#3142 Scrolling too fast in Wayland
#6081 OnlyTerm has no icon under Wayland when using fractional scaling (fractional-scale-v1)
#1701 linux+wayland+nvidia = wrong colors
#6025 Onlyterm window not movable via tab bar on fedora
#2687 flatpak release: Unable to set cursor to left_ptr: cursor not found
#5604 Can't open onlyterm under wayland
#4962 Onlyterm does not have window decorations/titlebar on Mutter/Wayland
#6831 Onlyterm crashes on startup on Wayland
#6926 Onlyterm wont start on Hyprland
#3083 (см. важное — N/A) — исключено выше
#4855 i3/awesomewm: Giving a terminal focus recopies the selection to the primary clipboard
#3892 How do I force TLSDomain to accept self signed certificate
#3751 Unable to set cursor in Gnome Wayland
#5382 Onlyterm fails to launch because of egl_lib
#4948 Panic with Wayland
#6725 Duplicate keystrokes in a Wayland session
#6673 window_decorations = "NONE" not working with enable_wayland = true
#3154 Switching between light and dark colour schemes on GNOME/Wayland makes OnlyTerm's title bar disappear
#1742 Wayland doesn't expose user's configured mouse cursor theme/style
#6699 Explicit Sync only supported on dmabuf buffers
#6355 Why does onlyterm attempt connections to api.github.com on startup?
#7150 NixOS: head of main built with nix flake crashes with EGL error
#2387 Onlyterm seems to forget the current keymap randomly on Gnome Wayland
#6762 Display issue inside an Ubuntu 24.04 docker container
#6463 Initial window size inconsistent on Sway
#6296 Title Bar is missing under Mutter
#6191 Weird issue when using onlyterm on gnome wayland
#5284 onlyterm doesn't redraw (or lags) on Plasma 6 + Wayland + Nvidia with config.enable_wayland=true
#7342 Error starting WT with egl_bindings.rs not getting loaded
#7070 Fail to start on Wayland/Hyprland fractional scaled monitors
#5895 (см. важное) — исключено выше
#5931 No wm_class on Gnome Wayland
#5879 window::os::wayland::connection > Unable to resolve appearance using xdg-desktop-portal
#3986 launching onlyterm in gnome sometimes don't get focused upon launch
#1732 Wayland: tab bar appears jittery when resizing
#1115 briefly gives incorrect size at launch (tiling window manager)
#6703 Onlyterm opens as a small cute vertical line with KDE fractional scaling
#6698 Nix Github Action / Cachix issue
#5933 uncomfortable privacy issue
#3050 wayland: fails to start when no pointer is present
#2933 default_gui_startup_args has no effect when launching with desktop file
#1869 Numlock not properly detected under Mutter Wayland (not XWayland)
#7553 Toast notifications on KDE plasma do not expire
#6338 appimage crash on fedora 41 (nightly works!)
#6318 maximized window overflows bottom of screen when using Wayland CSD
#6270 Protocol error when opening new tab on GNOME
#6815 Onlyterm crashes on startup with WebGPU frontend and Wayland fractional scaling
#6788 Huge fonts with MACOS_FORCE_SQUARE_CORNERS on WebGpu (but not on OpenGL)
#6618 can't launch onlyterm on sway
#6284 Shortcut keys for decrease font size fixed?
#2570 Bizarre alt-tab behaviour on NixOS
#2864 Vim theme
#7911 The rendering of the window is inconsistent with its "actual position"
#7886 Wayland tiling sizing issues
#7713 winget install wez.wezterm.nightly always fails with "Installer hash does not match"
#7623 wez.wezterm.nightly appears mispackaged in winget: resolves to stable 20240203 and fails hash validation
#7392 Startup error message on ubuntu 25.10
#6996 the title and bar have some bit problem at ubuntu 22.04
#6992 pure wayland mode is broken
#6946 tauri uses portable_pty with cmd window
#6910 Onlyterm's victor mono bold isn't as bold as other terminals' bold
#6617 WebGpu and Wayland Crash (Nvidia?) - Protocol Error 7 on object @0, dmabufs
#6211 Docs: augment-command-palette docs do not show how to add multiple commands
#5915 WebGPU not working on latest MESA v24.1.5
#5875 font not crispy on swaywm with 200% scaling
#5473 Onlyterm WebGpu doesn't work with llvmpipe driver
#7392 (дублировано выше)
#3552 process information doesn't work in flatpak: All new pane splits open in home directory
#3936 (см. важное) — исключено выше
#7011 AUR package onlyterm-git not failing to compile - feature edition2024 is required
#4036 linuxbrew is based on appimage which requires recent-ubuntu-compatible glibc
#6778 Installation via linuxbrew fails: "The following formula cannot be installed from bottle and must be built from source."
#2796 (см. важное) — исключено выше
#4704 flatpak: "can only use default prog commands with serial tty implementations" when launching serial console
#4681 Regression: Unable to set cursor to xterm: cursor not found
#2422 (см. важное) — исключено выше
#4225 (см. важное) — исключено выше
#4394 (см. важное, низкий приоритет) — исключено выше

## Отдельно — N/A: подсистемы, удалённые из форка (SSH-клиент / TLS-mux / mlua)

Ниже issues, применимость которых нулевая, поскольку затронутая подсистема отсутствует в нашем дереве кода (без анализа тела):

#4161 Error after manually closing last tab on ssh domain
#3083 ProxyCommand not working on Windows
#2699 Cannot create files over SFTP `LibSsh(Sftp(SftpError(2)))`
#5817 mux_enable_ssh_agent=true disables private key authentication w/ OpenSSH_for_Windows
#2985 Ssh with Certificate Authentication
#3892 How do I force TLSDomain to accept self signed certificate
#6650 `config.ssh_domains` configuration change doesn't get picked up
#7540 SSHMUX get confused and rapidly redraws and moves panes eventually leading to crash
#6543 Scrollback rendering issue on SSHMUX + default unix domain
#5439 High CPU usage on Windows, happening in onlyterm_ssh / WSAPoll
#4375 `ProxyCommand` to proxy-jump into the server is not working
#3539 ssh domains are not always reattached
#4295 onlyterm ssh fails to use Yubikey for authentication when using FIDO2
#5755 Doesn't ignore non-implemented ssh config sections
#5498 Trips on inline comment in SSH config
#6343 ssh via proxycommand / proxyJump
#6975 Missing ssh token support --- %C, %i, %d, %T
#7648 ssh_domains fails to authenticate with IdentityFile containing spaces on Windows
#6216 onlyterm ssh fails with "Too many authentication failures"
#5543 onlyterm cli tlscreds generates a certificate that does not use FQDN
#4802 Tables assigned to onlyterm.GLOBAL become "userdata" (специфика mlua, поведение под rhai другое — требует отдельной проверки, не переносится напрямую)
