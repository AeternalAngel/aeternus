# Aeternus

Emulador de terminal acelerado por GPU, escrito em Rust (winit + wgpu), para Linux e Windows.

🇺🇸 [Read in English](README.md)

![Captura de tela do Aeternus](docs/screenshot.png)

> **Status:** versão 1.2.1. Espere bugs e funcionalidades faltando. Veja o [CHANGELOG](CHANGELOG.md).

## Instalação

Baixe o binário da sua plataforma em [Releases](https://github.com/AeternalAngel/aeternus/releases).

### Linux (x86_64)

```bash
tar xzf aeternus-1.2.1-linux-x86_64.tar.gz
./aeternus
```

Requer driver de GPU com Vulkan e Wayland ou X11. O binário precisa de glibc 2.35 ou mais nova (ex.: Debian 12, Ubuntu 23.04, Fedora 37 ou posteriores); em distros mais antigas, compile do código-fonte.

### Windows (x86_64)

Extraia o `aeternus-1.2.1-windows-x86_64.zip` e execute o `aeternus.exe`. Requer Windows 10 ou mais novo e um driver de GPU com suporte a Vulkan ou DirectX 12.

O shell padrão é o `cmd.exe` (mudável no `config.toml`). A fonte padrão é uma Nerd Font; se ela não estiver instalada, o renderizador usa uma fonte do sistema como fallback. Instalar a fonte (ou editar `font.family` na configuração) melhora o resultado.

### Compilando do código-fonte

```bash
git clone https://github.com/AeternalAngel/aeternus.git
cd aeternus
cargo build --release
```

O binário estará em `target/release/aeternus` (Linux) ou `target/release/aeternus.exe` (Windows).

## Recursos

- **Renderização:** wgpu com atlas de glifos e uma única chamada de desenho; vsync ligado.
- **Fontes:** cosmic-text com fallback automático (Nerd Font por padrão), além de caracteres largos. Emoji são desenhados em monocromático.
- **Compatibilidade:** 16/256/truecolor (incluindo subparâmetros de SGR, ex. `38:2::r:g:b`), tela alternativa, bracketed paste, mouse reporting (1000/1002/1003/SGR), formatos de cursor via DECSCUSR, saída sincronizada (DEC 2026), DECRQM, XTVERSION, OSC de título/foco, OSC 4/10/11/12 (definição/reset de cor) e OSC 52 (só escrita no clipboard; desative com `terminal.osc52 = false`).
- **Uso diário:** scrollback de 10 mil linhas, seleção por palavra/linha, clipboard do sistema e seleção primária, zoom da fonte (Ctrl+roda).
- **Config:** gerada na primeira execução e recarregada com Ctrl+Shift+R. Se o arquivo tiver erros, a config anterior é mantida e os erros vão para o stderr no Linux ou para o `aeternus.log` ao lado do `config.toml` no Windows. `terminal.shell` aceita argumentos (ex. `"bash -l"`), `terminal.working_directory` define o diretório inicial e `terminal.osc52` (padrão `true`) permite que programas escrevam no clipboard via OSC 52.
  - Linux: `~/.config/aeternus/config.toml`
  - Windows: `%APPDATA%\aeternus\config.toml`
- **Linha de comando:** `--version` mostra a versão e sai; `--config <caminho>` usa esse arquivo em vez do padrão; `--bench` roda o benchmark do parser.

## Desempenho

| | Aeternus | kitty |
|---|---|---|
| `cat` de 300 mil linhas coloridas | 0,24 s | 0,32 s |
| `cat` de 3 milhões de linhas | 0,81 s | 1,31 s |

Execuções únicas no Linux, anteriores ao suporte a caracteres largos; vale refazer.

## Ainda não suportado

Emoji coloridos, emoji compostos, recarga automática da config ao salvar, leitura do clipboard via OSC 52 (mesmo sob opção), mudança de DPI em tempo real, links OSC 8, imagens inline, abas/divisões, reflow ao redimensionar, charsets DEC, atalhos configuráveis.

## Licença

[MIT](LICENSE) © 2026 Gabriel (AeternalAngel)
