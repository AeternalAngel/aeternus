# Aeternus

A GPU-accelerated terminal emulator written in Rust (winit + wgpu).

🇧🇷 [Leia em português](README.pt-BR.md)

![Aeternus screenshot](docs/screenshot.png)

> **Status:** first public release (1.0). Expect bugs and missing features.

## Install

Download the binary from [Releases](https://github.com/AeternalAngel/aeternus/releases):

```bash
tar xzf aeternus-1.0-linux-x86_64.tar.gz
./aeternus
```

Requires Linux (x86_64), a Vulkan-capable GPU driver, and Wayland or X11. The binary needs glibc 2.44 or newer; on other distros, build from source:

```bash
git clone https://github.com/AeternalAngel/aeternus.git
cd aeternus
cargo build --release
```

## Features

- **Rendering:** wgpu with a glyph atlas and a single draw call; vsync on.
- **Fonts:** cosmic-text with automatic fallback (Nerd Font by default), plus wide characters and color emoji.
- **Compatibility:** 16/256/truecolor, alternate screen, bracketed paste, mouse reporting (1000/1002/1003/SGR), DECSCUSR cursor shapes, OSC title/focus/52.
- **Daily use:** 10k-line scrollback, word/line selection, system and primary clipboard, font zoom (Ctrl+wheel).
- **Config:** `~/.config/aeternus/config.toml`, generated on first run and hot-reloaded on save (or Ctrl+Shift+R). Errors show in a banner and the previous config is kept.

## Performance

| | Aeternus | kitty |
|---|---|---|
| `cat` of 300k colored lines | 0.24 s | 0.32 s |
| `cat` of 3M lines | 0.81 s | 1.31 s |

Single runs, taken before wide character support; to be redone.

## Not supported yet

Composed emoji, live DPI changes, OSC 8 links, inline images, tabs/splits, line reflow on resize, DEC charsets, configurable keybindings.

## License

[MIT](LICENSE) © 2026 Gabriel (AeternalAngel)
