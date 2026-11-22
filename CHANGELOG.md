# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).


## [Unreleased]

### Added

- Scrollback search: `Ctrl+Shift+F` or Edit > Find opens a search bar in the top right corner. Matches in the history and on screen are highlighted as you type, and the bar shows the selected match and the total (for example `3/17`). `Enter` or `Up` goes to the previous (older) match, `Shift+Enter` or `Down` to the next, `Ctrl+Backspace` clears the query and `Esc` closes the bar. Matches can continue across wrapped lines, and the search ignores case unless the query has an uppercase letter. On the alternate screen only the visible screen is searched. The shortcut is the new `search` keybinding.
- Underline styles: `CSI 4:2 m` (double), `4:3` (curly), `4:4` (dotted) and `4:5` (dashed) are now drawn as such instead of as a single underline. `CSI 21 m` draws a double underline, as in kitty.
- Underline color: `CSI 58 m` (indexed or RGB, with `;` or `:`) sets the underline color and `CSI 59 m` resets it to the text color. Previously the color was parsed and discarded.
- `xterm-aeternus` terminfo entry, based on `xterm-256color` plus styled and colored underlines (`Smulx`, `Setulc`, `Su`), synchronized output (`Sync`), truecolor (`Tc`), strikethrough, cursor color and cursor shape reset. The packages and `install.sh` install it, and the shell now gets `TERM=xterm-aeternus` when the entry is found (setting `TERMINFO` when it is only next to the binary, as in the AppImage or a `~/.local` install), falling back to `TERM=xterm-256color` otherwise. The new `terminal.term` option overrides it.
- Shell integration (OSC 133): prompts marked by the shell can be reached with `Ctrl+Shift+Z` and `Ctrl+Shift+X` (View > Previous Prompt / Next Prompt), and `Ctrl+Shift+G` (Edit > Select Command Output) selects the output of the last command, going one command back each time it is pressed again. The marks survive reflow. fish 4.0 marks its prompts by itself; the README has snippets for zsh and bash. The new keybindings are `prev_prompt`, `next_prompt` and `select_output`.
- Desktop notifications: OSC 9, OSC 777 (`notify;title;body`) and kitty's OSC 99 (title and body, chunks, base64, urgency and `o=unfocused`) are shown with `notify-send`. ConEmu's `OSC 9;4` progress sequences are ignored. The new `terminal.notifications` option turns them off.
- Mouse pointer shape (OSC 22): programs can change the pointer over the terminal using CSS or X11 cursor names, with kitty's push (`>`), pop (`<`) and query (`?`) forms.
- XTGETTCAP (`DCS + q`): programs can query terminfo capabilities from the terminal, which also works over SSH where the entry may not be installed. The answers come from the bundled `xterm-aeternus` entry (now declaring `colors#256` itself), plus `TN`, `Co` and `RGB`.
- DECRQSS (`DCS $ q`): the terminal reports the current SGR attributes (`m`), scroll region (`r`) and cursor style (` q`), and answers `DCS 0 $ r` for anything else. Neovim uses the SGR answer to detect truecolor.
- Blinking text: cells with `CSI 5 m` or `CSI 6 m` now blink at the cursor blink rate while the window is focused. Blinking stops, with the text shown, when the window loses focus.

## [1.4.2] - 2026-11-20

### Fixed

- Powerline separators (U+E0B0 to U+E0BF) no longer leave stray pixels above or below prompt segments in themes like oh-my-posh or starship. They are now drawn by the terminal to fill the cell exactly, instead of using the font's glyph, whose height did not match the cell.
- A dead key followed by space (`´`, `~`, `^`, `` ` ``) now inserts the accent itself, as in kitty. Space previously always sent a plain space, dropping the accent.

## [1.4.1] - 2026-11-17

### Changed

- The default ANSI palette (`color0` to `color15`) is now kitty's default palette, both built in and in the configuration template. Configurations that set these colors keep them.
- Glyph advances inside multi-glyph clusters (combining marks, emoji sequences) are snapped to whole pixels.

### Fixed

- The gamma value of `font.text_composition_strategy` had no effect on light text on dark backgrounds. It now follows kitty's curve: the adjustment is weighted by the luminance difference between the text and the cell background, so `"1.7 30"` looks the same as in kitty. The default `"platform"` is unchanged.
- Text in the tab strip, menus and Preferences was placed at fractional pixel positions; it is now aligned to whole pixels.

## [1.4] - 2026-11-15

### Added

- Tabs: multiple shells in one window. The tab strip appears once there are two or more tabs, with each tab showing its own title, a close button and a `+` button. Default shortcuts: `Ctrl+Shift+T` (new tab), `Ctrl+Shift+W` (close tab), `Ctrl+Tab`/`Ctrl+PageDown` and `Ctrl+Shift+Tab`/`Ctrl+PageUp` (switch); middle-click closes a tab. New tabs open in the working directory of the current tab (OSC 7, or the shell's current directory).
- Scrollable tab strip: when tabs don't fit, the mouse wheel scrolls the strip, `‹`/`›` buttons appear on the right edge and the strip follows the active tab.
- Toolbar: `window.toolbar = true` shows a menu bar (File, Edit, View, Help) with a settings button on the right. The menus show the shortcuts actually configured in `[keybindings]`.
- Preferences panel with Simple and Advanced modes, grouped sections, animated toggles, `Tab`/`Shift+Tab` navigation, `Enter` to save, `Esc` to cancel and paste into text fields. Saving writes `config.toml` while keeping its comments and applies the changes immediately.
- Color picker for color fields (hue, saturation and brightness, current vs. new preview, hex and RGB readouts).
- Colors use kitty's names and formats: `[colors]` accepts `foreground`, `background`, `selection_background`, `selection_foreground`, `cursor`, `cursor_text_color` and `color0` to `color255`, and `colors.theme` loads a kitty theme file (`.conf`, with `include` support) directly. Keys in `[colors]` override the theme, and the theme file is watched for changes. The previous `selection`, `normal` and `bright` keys still work.
- Gamma-correct (linear) text blending like kitty, which makes light text on dark backgrounds fuller and smoother. `font.text_composition_strategy` takes kitty's values: `"platform"` (default), `"legacy"` (the previous look) or `"<gamma> <contrast>"`.
- OSC 4 works for all 256 palette entries, and color queries return the configured palette.
- Background transparency: `window.opacity` (0.0 to 1.0, default 1.0), also in Preferences and applied live. As in kitty, only the default background becomes transparent; text, the cursor, the selection and cells with an explicit background color stay solid. Blur is up to the compositor (for example a Hyprland `blur` rule).
- Cursor trail inspired by kitty: `cursor.trail` (milliseconds the cursor must rest before a move animates; `0` disables it), `cursor.trail_decay = [fast, slow]`, `cursor.trail_start_threshold` (cells) and `cursor.trail_color`. Also available in Preferences.
- `aet` companion command shipped next to `aeternus`: `aet version`, `aet pref` and `aet reload` act on the most recently focused window. `aet --help` and `aet --version` are available.
- "About Aeternus" prints the Aeternus art and version into the active terminal.
- Clickable links: `Ctrl+click` opens URLs (`http`, `https`, `file`, `ftp`, `mailto`, `ssh`) and OSC 8 hyperlinks with `xdg-open`; use `Ctrl+Shift+click` when the program captures the mouse. Hyperlinks are underlined and the pointer changes over links while `Ctrl` is held.
- Combining marks (e.g. decomposed accents), emoji skin-tone modifiers and ZWJ emoji sequences are kept in the same cell and rendered as one cluster; copying keeps them.
- DEC special graphics charset (`ESC ( 0`, `ESC ) 0`, SI/SO) for line drawing, and the UK charset.
- Escape sequences: REP (`CSI b`), tab stops (HTS, TBC, CHT `CSI I`, CBT `CSI Z`), insert mode (IRM, `CSI 4 h`), newline mode (LNM, `CSI 20 h`), origin mode (DECOM, `CSI ? 6 h`), DECALN (`ESC # 8`), DECSTR (`CSI ! p`), ANSI DECRQM (`CSI Ps $ p`), mode 1048, `CSI 14 t`/`CSI 16 t` pixel size reports, OSC 4 color queries and SGR `4:0` to turn underline off.
- Kitty keyboard protocol, "disambiguate escape codes" level (push, pop, set and query with `CSI > u`, `CSI < u`, `CSI = u`, `CSI ? u`); the main and alternate screens keep separate stacks.
- Bell: BEL flashes the window (`bell.visual`) and requests attention when the window is not focused (`bell.urgent`).
- `terminal.smart_copy_paste` (default `false`): when on, `Ctrl+C` copies if there is a selection and `Ctrl+V` pastes.
- New keybinding actions: `new_tab`, `close_tab`, `next_tab`, `prev_tab`, `fullscreen` and `preferences`. Every built-in shortcut now goes through `[keybindings]`, and an empty list (`[]`) disables an action.
- `F11` or View > Fullscreen toggles borderless fullscreen.
- Command line: `-h`/`--help`, `-V`/`--version` and `-c`/`--config`. Unknown arguments are reported instead of ignored.
- Log file at `~/.local/state/aeternus/aeternus.log` (`$XDG_STATE_HOME`). Fatal startup errors (no GPU, no display, shell not found) also show a desktop notification.
- `TERM_PROGRAM=aeternus` and `TERM_PROGRAM_VERSION` are exported to the shell, and the PTY reports its size in pixels.
- Packaging: `install.sh` (user or system install, `--uninstall`, `DESTDIR`), desktop entry and AppStream metadata with the `io.github.AeternalAngel.Aeternus` ID, man pages for `aeternus` and `aet`, bash/zsh/fish completions and AUR `PKGBUILD`s. Releases now ship `.tar.gz` (with the desktop entry, icons and installer), `.deb`, `.rpm` and AppImage for x86_64 and aarch64.

### Changed

- The interface, messages, the configuration template and the `aet` output are now in English.
- The default font is `monospace`, resolved through fontconfig to the system's monospace font; a warning is logged when the configured font is not installed.
- `terminal.shell` is parsed like a shell command line, so quotes and paths with spaces work (e.g. `"\"/opt/my shell/zsh\" -l"`).
- `Ctrl+C` and `Ctrl+V` are sent to the program by default (they no longer copy or paste); see `terminal.smart_copy_paste`.
- Shortcuts bound to scrolling (`Shift+PageUp` and friends) are passed to the program on the alternate screen instead of being swallowed.
- The window uses the `io.github.AeternalAngel.Aeternus` app ID, so it matches its desktop entry and icon.
- `config.toml` is saved atomically (temporary file and rename), keeps its permissions and follows symlinks.
- When `XDG_RUNTIME_DIR` is not set, the `aet` socket goes to a private per-user directory instead of `/tmp`.
- `CSI A`/`B`/`E`/`F` stop at the scroll region margins, as in xterm.
- Primary device attributes report VT220 with ANSI color (`CSI ? 62 ; 22 c`).
- Dependencies updated: wgpu 30, cosmic-text 0.19, portable-pty 0.9, unicode-width 0.2, toml 1, toml_edit 0.25, pollster 1.
- Release builds use `opt-level = 3`, thin LTO, one codegen unit and are stripped (the binary went from about 18 MB to 12 MB).
- The renderer tries a software adapter before giving up when no hardware GPU is found, and honors wgpu environment overrides such as `WGPU_BACKEND=gl`.
- The window surface always uses an 8-bit (or 10-bit) non-sRGB format, so colors are shown exactly as configured on drivers that also offer 16-bit float surfaces.
- The code is split into a library and modules (`app`, `input`, `ipc`, `render/*`) and the minimum supported Rust version is 1.90.

### Fixed

- Closed tabs and shells that exited on their own were left as zombie processes until Aeternus quit.
- Nerd Font icons and other symbols wider than a cell (common with the non-Mono "NF" font variants and with fallback fonts) overlapped the neighboring characters. Like kitty, such symbols are now scaled down and centered in their cell, or use two cells when followed by a space; powerline separators keep their natural shape.
- Output printed right after startup was pushed into the scrollback when the window was resized (for example by a tiling compositor), because the reflow counted the empty rows below the cursor as content.
- The `aet` socket and the `.last` file were left behind when the last shell exited, and after a crash.
- The app no longer calls `std::process::exit`; it leaves the event loop and tears down tabs, the GPU and the socket in order.
- `aet version` printed a fixed `1.4.0` instead of the real version.
- The "About" art left the text color changed for the shell output that followed it.
- Built-in shortcuts ignored the `[keybindings]` configuration and could not be disabled or remapped.
- Zero-width characters (combining accents, ZWJ, variation selectors) were dropped.
- Erasing part of a line (`CSI K`, `CSI J`) over half of a wide character left the other half behind.
- Reflow kept stale wrap markers, so text could join the wrong line after several resizes.
- Pasting stripped `ESC` but not the 8-bit CSI (`U+009B`) from bracketed paste.
- The menu panel could open partly outside a narrow window.
- The Preferences text fields showed the beginning of long values instead of the end being typed.
- The toolbar height did not follow display scale changes, and the grid was not resized right after a scale change.
- The PTY reader thread busy-waited (`yield_now`) while the renderer held the terminal; it now parks.
- Window-less errors (no GPU adapter, no display, invalid shell) panicked instead of reporting the problem.

### Removed

- Windows support: platform code (ConPTY-side `cmd.exe` fallback, `%APPDATA%` config path, `aeternus.log`, DWM/dark-mode helpers, `windows_subsystem`), the `cfg(windows)` dependency, CI job and release packaging are gone. Aeternus is Linux-only (Wayland/X11).
- The Portuguese part of this file moved to [CHANGELOG.pt-BR.md](CHANGELOG.pt-BR.md).

## [1.3] - 2026-11-12

### Added

- Added reflow on resize: wrapped lines are rejoined and rewrapped to fit the new width, using the existing `WRAPLINE` tracking, instead of being cropped or padded.
- Added live DPI change handling: moving the window to a display with a different scale factor rescales the font and grid immediately.
- Added colored emoji rendering: the glyph atlas now stores RGBA data, so color glyphs from cosmic-text are drawn with their own colors instead of being tinted by the text color.
- Added configurable keybindings through a new `[keybindings]` config section (`copy`, `paste`, `paste_primary`, `reload_config`, `zoom_in`, `zoom_out`, `zoom_reset`, `scroll_page_up`, `scroll_page_down`, `scroll_to_top`, `scroll_to_bottom`), each accepting a list of key combinations.
- Added automatic reload of `config.toml` when it changes on disk, polled once a second; Ctrl+Shift+R still works too.

### Changed

- Changed resizing on the main screen to reflow text instead of just cropping or padding it; the alternate screen (full-screen apps) and height-only resizes are unaffected.

## [1.2.1] - 2026-11-10

### Added

- `terminal.osc52` config option (default `true`) to enable or disable OSC 52 clipboard writes.
- `--version` and `--config <path>` command line options.
- Application keypad mode (`ESC =` / `ESC >`).
- Ctrl+Space sends NUL (0x00) and Ctrl+/ sends 0x1f.

### Fixed

- Glyphs no longer get wrong texture coordinates when the glyph atlas resets in the middle of a frame.
- Synchronized output (DEC 2026) is forced off after 150 ms, so a program that never turns it off can't freeze the screen.
- OSC 52 payloads up to 1 MiB of decoded text are accepted; larger ones are ignored instead of being truncated.
- Overwriting, erasing, inserting or deleting over half of a wide character clears the other half, and resizing no longer leaves half of a pair.
- Background, selection and block cursor cover both cells of a wide character.
- XTVERSION reports the actual version instead of a fixed "aeternus 1.1".
- On Windows, config errors are written to `aeternus.log` next to `config.toml`, since release builds have no console.
- README no longer claims hot-reload on save, an error banner or color emoji.

## [1.2] - 2026-11-09

### Added

- `terminal.shell` accepts arguments (e.g. `"bash -l"`).
- `terminal.working_directory` config option to set the starting directory.
- Synchronized output mode (DEC private mode 2026).
- DECRQM (`CSI ? Ps $ p`) and XTVERSION (`CSI > q`) responses.
- OSC 4/10/11/12 color set and reset, and OSC 52 clipboard write.
- SGR sub-parameters separated by `:` (e.g. `4:3`, `38:2::r:g:b`); SGR 58/59 are parsed and ignored instead of breaking parameter alignment.
- Wide character (CJK, emoji) width handling, with selection and copy skipping the continuation cell.

### Fixed

- Segmentation fault on exit, caused by GPU/clipboard teardown racing with the PTY reader thread.
- Alt+Left/Right now moves the cursor by word in the shell.
- Config path on Windows now follows `%APPDATA%\aeternus\config.toml` instead of the Linux path.
- Copy-on-select (releasing the left mouse button) no longer overwrites the clipboard on Windows.
- The glyph atlas resets and keeps working once full, instead of permanently blanking new glyphs.
- No console window flashes behind the app on Windows release builds.

## [1.1] - 2026-11-08

Initial Windows support.

[Unreleased]: https://github.com/AeternalAngel/aeternus/compare/v1.4.2...HEAD
[1.4.2]: https://github.com/AeternalAngel/aeternus/compare/v1.4.1...v1.4.2
[1.4.1]: https://github.com/AeternalAngel/aeternus/compare/v1.4.0...v1.4.1
[1.4]: https://github.com/AeternalAngel/aeternus/compare/v1.3.0...v1.4.0-pre1
[1.3]: https://github.com/AeternalAngel/aeternus/compare/v1.2.1...v1.3.0
[1.2.1]: https://github.com/AeternalAngel/aeternus/compare/v1.2.0...v1.2.1
[1.2]: https://github.com/AeternalAngel/aeternus/releases/tag/v1.2.0
