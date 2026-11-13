# Aeternus

Emulador de terminal acelerado por GPU, escrito em Rust (winit + wgpu), para Linux.

🇺🇸 [Read in English](README.md)

![Captura de tela do Aeternus](docs/screenshot.png)

> **Status:** versão 1.4. Espere bugs e funcionalidades faltando. Veja o [CHANGELOG](CHANGELOG.md).

## Instalação

Baixe o binário da sua plataforma em [Releases](https://github.com/AeternalAngel/aeternus/releases).

### Linux (x86_64)

```bash
tar xzf aeternus-1.4-linux-x86_64.tar.gz
./aeternus
```

O arquivo traz o `aeternus` e o CLI auxiliar `aet`; coloque o `aet` em algum diretório do `PATH` para usá-lo no shell.

Requer driver de GPU com Vulkan e Wayland ou X11. O binário precisa de glibc 2.35 ou mais nova (ex.: Debian 12, Ubuntu 23.04, Fedora 37 ou posteriores); em distros mais antigas, compile do código-fonte.

### Compilando do código-fonte

```bash
git clone https://github.com/AeternalAngel/aeternus.git
cd aeternus
cargo build --release
```

Os binários estarão em `target/release/aeternus` e `target/release/aet`.

## Recursos

- **Renderização:** wgpu com atlas de glifos e uma única chamada de desenho; vsync ligado.
- **Fontes:** cosmic-text com fallback automático (Nerd Font por padrão), além de caracteres largos e emoji coloridos. Reage a mudanças de DPI em tempo real (ex.: mover a janela para outra tela).
- **Compatibilidade:** 16/256/truecolor (incluindo subparâmetros de SGR, ex. `38:2::r:g:b`), tela alternativa, bracketed paste, mouse reporting (1000/1002/1003/SGR), formatos de cursor via DECSCUSR, saída sincronizada (DEC 2026), DECRQM, XTVERSION, OSC de título/foco, OSC 4/10/11/12 (definição/reset de cor) e OSC 52 (só escrita no clipboard; desative com `terminal.osc52 = false`).
- **Uso diário:** scrollback de 10 mil linhas com reflow ao redimensionar, seleção por palavra/linha, clipboard do sistema e seleção primária, zoom da fonte (Ctrl+roda).
- **Config:** gerada na primeira execução e recarregada automaticamente quando o arquivo muda (ou com Ctrl+Shift+R). Se o arquivo tiver erros, a config anterior é mantida e os erros vão para o stderr. `terminal.shell` aceita argumentos (ex. `"bash -l"`), `terminal.working_directory` define o diretório inicial e `terminal.osc52` (padrão `true`) permite que programas escrevam no clipboard via OSC 52.
  - Caminho da config: `~/.config/aeternus/config.toml` (ou `$XDG_CONFIG_HOME/aeternus/config.toml`)
- **Atalhos:** configuráveis através de uma seção `[keybindings]` (`copy`, `paste`, `paste_primary`, `reload_config`, `zoom_in`, `zoom_out`, `zoom_reset`, `scroll_page_up`, `scroll_page_down`, `scroll_to_top`, `scroll_to_bottom`), cada um aceitando uma lista de combinações de tecla (ex. `["ctrl+shift+c"]`).
- **Toolbar:** `window.toolbar = true` mostra a barra de menus (Arquivo, Editar, Ver, Ajuda) abaixo da moldura nativa da janela.
- **Abas:** quantos shells quiser na mesma janela (`Ctrl+Shift+T`, o botão `+` ou o menu Arquivo). A tira de abas aparece com duas ou mais abas e mostra o título de cada uma, botão de fechar e realce de hover; `Ctrl+Tab` circula entre as abas e o clique do meio fecha uma. Quando as abas não cabem, a roda do mouse rola a tira (uma aba por vez), surgem botões `‹`/`›` na direita (com o `+` fixo) e a tira acompanha a aba ativa.
- **Comandos `aet`:** o CLI `aet` conversa com a instância em execução via socket Unix em `$XDG_RUNTIME_DIR` e mira a janela focada mais recentemente: `aet version` imprime a versão no terminal ativo (o prompt do shell volta sozinho), `aet pref` abre as Preferências (útil com a toolbar desligada) e `aet reload` recarrega o `config.toml`.
- **Preferências:** botão de engrenagem na barra ou o menu Editar abre o overlay de configurações. O modo `Simples` mantém só o essencial, com seletor de cores embutido (matiz/saturação/brilho, pré-visualização, leitura hex/RGB); `Avançado` expõe todas as opções agrupadas por seção. Salvar grava o `config.toml` (mantendo os comentários) e aplica na hora.
- **Tela cheia:** `F11` ou o menu Ver alterna tela cheia sem moldura.
- **Linha de comando:** `--version` mostra a versão e sai; `--config <caminho>` usa esse arquivo em vez do padrão; `--bench` roda o benchmark do parser.

## Desempenho

refazendo.

## Ainda não suportado

Emoji compostos, leitura do clipboard via OSC 52 (mesmo sob opção), links OSC 8, imagens inline, divisões, charsets DEC.

## Licença

[MIT](LICENSE) © 2026 Gabriel (AeternalAngel)
