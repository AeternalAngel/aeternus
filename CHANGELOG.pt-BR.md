# Histórico de mudanças

> Este arquivo guarda o histórico em português até a versão 1.3 e não será mais atualizado.
> A partir da 1.4 a interface e o histórico são só em inglês: veja o [CHANGELOG.md](CHANGELOG.md).

## 1.3 - 2026-11-12

### Adicionado

- Adicionado reflow ao redimensionar: linhas quebradas são rejuntadas e requebradas na nova largura usando a flag `WRAPLINE` já existente, em vez de serem cortadas ou preenchidas.
- Adicionada atualização de DPI em tempo real: mover a janela para uma tela com escala diferente reajusta a fonte e a grade na hora.
- Adicionado desenho de emoji coloridos: o atlas de glifos agora guarda dados RGBA, então glifos coloridos do cosmic-text são desenhados com a própria cor em vez de tingidos pela cor do texto.
- Adicionados atalhos configuráveis através de uma nova seção `[keybindings]` na configuração (`copy`, `paste`, `paste_primary`, `reload_config`, `zoom_in`, `zoom_out`, `zoom_reset`, `scroll_page_up`, `scroll_page_down`, `scroll_to_top`, `scroll_to_bottom`), cada um aceitando uma lista de combinações de tecla.
- Adicionada recarga automática do `config.toml` quando ele muda no disco, verificada a cada 1 segundo; Ctrl+Shift+R continua funcionando também.

### Alterado

- Alterado o redimensionamento na tela principal para reflowar o texto em vez de só cortar ou preencher; a tela alternativa (apps em tela cheia) e redimensionamentos só de altura não são afetados.

## 1.2.1 - 2026-11-10

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

## 1.2 - 2026-11-09

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

## 1.1 - 2026-11-08

Suporte inicial ao Windows.
