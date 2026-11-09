# Changelog

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
