# Changelog

## 1.4

### Added

- Toolbar: `window.toolbar = true` shows a menu bar (Arquivo, Editar, Ver, Ajuda) below the native window frame, with a settings gear on the right that opens Preferences directly. Besides the existing actions, the menus add "Selecionar tudo", "Resetar terminal", "Tela cheia" (`F11`) and "Sobre o Aeternus".
- Tabs: multiple shells in one window. The tab strip appears under the toolbar once there are two or more tabs, with each tab showing its own title (OSC, e.g. the shell's cwd), a close button and a `+` button for a new tab. Shortcuts: `Ctrl+Shift+T` (new tab), `Ctrl+Shift+W` (close tab), `Ctrl+Tab`/`Ctrl+Shift+Tab` and `Ctrl+PageUp`/`Ctrl+PageDown` (switch), middle-click closes a tab; the File menu also hosts "Nova aba"/"Fechar aba". Each tab keeps its own scrollback, title and PTY; the active tab is resized with the window.
- Scrollable tab strip: when tabs don't fit, the mouse wheel over the strip scrolls it one tab at a time, `‹`/`›` buttons appear on the right edge while any tab stays hidden on either side (with hover state and a pinned `+` button) and the strip auto-follows the active tab on switch, create and close.
- Preferences overlay with a `Simples`/`Avançado` toggle in the header. The simple mode shows only the everyday options (font, toolbar, colors); the advanced mode shows everything, grouped in labeled sections (Fonte, Janela, Terminal, Cursor, Cores) inside cards, with toggles drawn as pills whose knob slides over ~120 ms while their theme-derived colors transition. Saving writes `config.toml` and applies the changes instantly; existing comments and formatting in the file are preserved.
- Color picker: color fields render as a swatch with the current color; clicking one opens an in-app overlay with a hue/saturation/brightness picker drawn with the renderer itself, current vs. new preview, hex and RGB readouts and Cancelar/Confirmar buttons (`Esc` cancels, `Enter` confirms). The hex field stays editable in advanced mode.
- The `aet` companion CLI, shipped in the release archive, talks to a running instance over a Unix socket in `$XDG_RUNTIME_DIR` and targets the most recently focused instance: `aet version` prints `aeternus 1.4.0` into the active terminal (the shell prompt returns normally), `aet pref` opens Preferences and `aet reload` reloads `config.toml`. Unknown or missing arguments print a usage help.
- "Sobre o Aeternus" prints the Aeternus ASCII art and version into the active terminal, with the prompt returning right below it. The art's accent color is derived from the configured background and its text is white on dark backgrounds and black on light ones; both are resolved every frame, so art already on screen recolors itself when the background changes.

### Removed

- Windows support: platform code (ConPTY-side `cmd.exe` fallback, `%APPDATA%` config path, `aeternus.log`, DWM/dark-mode helpers, `windows_subsystem`), the `cfg(windows)` dependency, CI job and release packaging are gone. Aeternus is Linux-only (Wayland/X11).

## 1.3

### Added

- Added reflow on resize: wrapped lines are rejoined and rewrapped to fit the new width, using the existing `WRAPLINE` tracking, instead of being cropped or padded.
- Added live DPI change handling: moving the window to a display with a different scale factor rescales the font and grid immediately.
- Added colored emoji rendering: the glyph atlas now stores RGBA data, so color glyphs from cosmic-text are drawn with their own colors instead of being tinted by the text color.
- Added configurable keybindings through a new `[keybindings]` config section (`copy`, `paste`, `paste_primary`, `reload_config`, `zoom_in`, `zoom_out`, `zoom_reset`, `scroll_page_up`, `scroll_page_down`, `scroll_to_top`, `scroll_to_bottom`), each accepting a list of key combinations.
- Added automatic reload of `config.toml` when it changes on disk, polled once a second; Ctrl+Shift+R still works too.

### Changed

- Changed resizing on the main screen to reflow text instead of just cropping or padding it; the alternate screen (full-screen apps) and height-only resizes are unaffected.

## 1.2.1

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

## 1.2

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

## 1.1

Initial Windows support.

---

# Histórico de mudanças

## 1.3

### Adicionado

- Adicionado reflow ao redimensionar: linhas quebradas são rejuntadas e requebradas na nova largura usando a flag `WRAPLINE` já existente, em vez de serem cortadas ou preenchidas.
- Adicionada atualização de DPI em tempo real: mover a janela para uma tela com escala diferente reajusta a fonte e a grade na hora.
- Adicionado desenho de emoji coloridos: o atlas de glifos agora guarda dados RGBA, então glifos coloridos do cosmic-text são desenhados com a própria cor em vez de tingidos pela cor do texto.
- Adicionados atalhos configuráveis através de uma nova seção `[keybindings]` na configuração (`copy`, `paste`, `paste_primary`, `reload_config`, `zoom_in`, `zoom_out`, `zoom_reset`, `scroll_page_up`, `scroll_page_down`, `scroll_to_top`, `scroll_to_bottom`), cada um aceitando uma lista de combinações de tecla.
- Adicionada recarga automática do `config.toml` quando ele muda no disco, verificada a cada 1 segundo; Ctrl+Shift+R continua funcionando também.

### Alterado

- Alterado o redimensionamento na tela principal para reflowar o texto em vez de só cortar ou preencher; a tela alternativa (apps em tela cheia) e redimensionamentos só de altura não são afetados.

## 1.2.1

### Adicionado

- Opção de configuração `terminal.osc52` (padrão `true`) para ligar ou desligar a escrita no clipboard via OSC 52.
- Opções de linha de comando `--version` e `--config <caminho>`.
- Modo de keypad de aplicação (`ESC =` / `ESC >`).
- Ctrl+Espaço envia NUL (0x00) e Ctrl+/ envia 0x1f.

### Corrigido

- Glifos não recebem mais coordenadas de textura erradas quando o atlas é reiniciado no meio de um frame.
- A saída sincronizada (DEC 2026) é desligada à força após 150 ms, para que um programa que nunca a desliga não congele a tela.
- Payloads de OSC 52 com até 1 MiB de texto decodificado são aceitos; maiores são ignorados em vez de truncados.
- Sobrescrever, apagar, inserir ou excluir sobre metade de um caractere largo limpa a outra metade, e redimensionar não deixa mais metade de um par.
- Fundo, seleção e cursor em bloco cobrem as duas células de um caractere largo.
- XTVERSION informa a versão real em vez de um "aeternus 1.1" fixo.
- No Windows, erros de configuração são gravados em `aeternus.log` ao lado do `config.toml`, já que builds release não têm console.
- O README não afirma mais recarga ao salvar, faixa de erros nem emoji colorido.

## 1.2

### Adicionado

- `terminal.shell` aceita argumentos (ex.: `"bash -l"`).
- Opção de configuração `terminal.working_directory` para definir o diretório inicial.
- Modo de saída sincronizada (modo privado DEC 2026).
- Respostas a DECRQM (`CSI ? Ps $ p`) e XTVERSION (`CSI > q`).
- Definição e redefinição de cores via OSC 4/10/11/12, e escrita no clipboard via OSC 52.
- Subparâmetros SGR separados por `:` (ex.: `4:3`, `38:2::r:g:b`); SGR 58/59 são lidos e ignorados em vez de quebrar o alinhamento dos parâmetros.
- Tratamento de largura de caracteres largos (CJK, emoji), com seleção e cópia ignorando a célula de continuação.

### Corrigido

- Falha de segmentação ao fechar, causada pela corrida entre o encerramento da GPU/clipboard e a thread de leitura do PTY.
- Alt+Esquerda/Direita agora move o cursor por palavra no shell.
- O caminho da configuração no Windows agora segue `%APPDATA%\aeternus\config.toml` em vez do caminho do Linux.
- O copiar-ao-selecionar (soltar o botão esquerdo do mouse) não sobrescreve mais o clipboard no Windows.
- O atlas de glifos é reiniciado e continua funcionando quando enche, em vez de deixar glifos novos em branco para sempre.
- Nenhuma janela de console pisca atrás do app em builds release no Windows.

## 1.1

Suporte inicial ao Windows.
