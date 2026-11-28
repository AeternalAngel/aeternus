# Compatibility tests

These tests run real programs in a pseudo-terminal connected to Aeternus's parser and screen
(`Parser` + `Term`), with no window. See the "Compatibility tests" section of
[CONTRIBUTING.md](../../CONTRIBUTING.md) for how to run them.

| Module        | What it runs                                                                                    |
| ------------- | ----------------------------------------------------------------------------------------------- |
| `shells.rs`   | bash, zsh, fish and dash: commands, line editing, history, Tab, resize, Ctrl+C/Ctrl+D           |
| `terminfo.rs` | `tput` against the `xterm-aeternus` entry: keys, SGR, cursor save/restore, autowrap             |
| `tmux.rs`     | tmux: status line, panes, mouse, truecolor, undercurl, OSC 52, resize                           |
| `editors.rs`  | Neovim and Vim: alternate screen, bracketed paste, cursor shapes, mouse, kitty keyboard, resize |
| `tui.rs`      | less and htop                                                                                   |
| `vttest.rs`   | vttest menus 1 (cursor movement, autowrap) and 6 (DSR and DA reports)                           |
| `unicode.rs`  | character widths, wide characters at the margin and half-overwritten, Neovim editing wide text  |

Keys, clicks and pastes go through the same encoders the window uses (`input::key_bytes`,
`input::mouse_report`, `input::paste_bytes`).

## Character widths

Programs place the cursor using the C library's `wcwidth`, so a terminal that disagrees with it
draws text in a different column than the program expects. The table shows the columns Aeternus
uses, the columns glibc 2.44 reports (`wc -L`), and what kitty does. The `unicode.rs` tests
check the Aeternus column and the glibc column, so a glibc update that changes a width fails the
test and this table has to be updated.

| Text                                | Aeternus  | glibc | kitty        | Notes                                        |
| ----------------------------------- | --------- | ----- | ------------ | -------------------------------------------- |
| `😀` emoji                          | 2         | 2     | 2            |                                              |
| `中` CJK                            | 2         | 2     | 2            |                                              |
| `é` as `e` + U+0301                 | 1         | 1     | 1            | The mark is stored with the base cell.       |
| `👨‍👩‍👧` ZWJ sequence               | 2         | 6     | 2            | One wide cell holding the whole sequence.    |
| `👍🏽` emoji + skin tone            | 2         | 4     | 2            | The modifier joins the previous wide emoji.  |
| `🇧🇷` flag (two regional indicators) | 2 (1 + 1) | 2     | 2 (one cell) | Two narrow cells; same total width as glibc. |
| `❤️` U+2764 + VS16                   | 1         | 1     | 2            | VS16 does not widen the base.                |

Aeternus measures each code point with `unicode-width` and only joins code points that `wcwidth`
counts as zero width, plus a wide character right after a ZWJ and a skin tone right after a wide
emoji. That matches glibc except for ZWJ sequences and skin tones, where Aeternus follows kitty:
programs that add up `wcwidth` (readline, zle) count more columns than the terminal draws and
get out of step with the screen after such a sequence, while programs that measure grapheme clusters (Neovim, tested with 0.12) line up. foot
with its default `grapheme-width-method=wcswidth` follows glibc instead. VS16 is the opposite case:
kitty widens `❤️` to two columns, but Aeternus keeps glibc's width so readline and `ls` stay
aligned. Aeternus does not implement mode 2027 (grapheme cluster mode), so programs cannot ask for
the other behaviour.

## What these tests do not cover

A green run means the parser and the screen model behave as these programs expect. It does not
mean the terminal works end to end. Not covered:

- **Rendering.** Nothing is drawn: the wgpu renderer, glyph atlas, colors on screen, cursor
  drawing, damage tracking and images (sixel and kitty graphics are only parsed, not shown).
- **Window input.** Tests start from `KeyInput` and cell coordinates, so the translation of winit
  events is skipped: keyboard layouts, dead keys, compose, IME, modifier tracking, pixel-to-cell
  mouse mapping, scroll wheel and touchpad.
- **Fonts.** Font lookup and fallback, ligatures, emoji fonts and how wide clusters and flags
  actually look.
- **The rest of the app.** Preferences, configuration reloading, the system clipboard (OSC 52 is
  checked only up to the request reaching the terminal), selection, scrollback UI, search,
  notifications and window management.
- **Real user setups.** Programs run without configuration files, with `C.UTF-8` and at fixed
  sizes; prompt frameworks, plugins and other locales are not exercised. Results also depend on
  the installed program versions.
