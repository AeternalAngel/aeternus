# Aeternus

A GPU-accelerated terminal emulator written in Rust (winit + wgpu) for Linux and Windows.

🇧🇷 [Leia em português](README.pt-BR.md)

![Aeternus screenshot](docs/screenshot.png)

> **Status:** version 1.1, now with Windows support. Expect bugs and missing features.

## Install

Download the binary for your platform from [Releases](https://github.com/AeternalAngel/aeternus/releases).

### Linux (x86_64)

```bash
tar xzf aeternus-1.1-linux-x86_64.tar.gz
./aeternus
```

Requires a Vulkan-capable GPU driver and Wayland or X11. The binary needs glibc 2.44 or newer (Arch and other rolling releases); on other distros, build from source.

### Windows (x86_64)

Extract `aeternus-1.1-windows-x86_64.zip` and run `aeternus.exe`. Requires Windows 10 or newer and a GPU driver with Vulkan or DirectX 12 support.

The default shell is `cmd.exe` (changeable in `config.toml`). The default font is a Nerd Font; if it isn't installed, a system font is used as fallback. Installing the font (or editing `font.family` in the config) gives better results.

### Building from source

```bash
git clone https://github.com/AeternalAngel/aeternus.git
cd aeternus
cargo build --release
```

The binary will be at `target/release/aeternus` (Linux) or `target/release/aeternus.exe` (Windows).

## Features

- **Rendering:** wgpu with a glyph atlas and a single draw call; vsync on.
- **Fonts:** cosmic-text with automatic fallback (Nerd Font by default), plus wide characters and color emoji.
- **Compatibility:** 16/256/truecolor, alternate screen, bracketed paste, mouse reporting (1000/1002/1003/SGR), DECSCUSR cursor shapes, OSC title/focus/52.
- **Daily use:** 10k-line scrollback, word/line selection, system and primary clipboard, font zoom (Ctrl+wheel).
- **Config:** generated on first run and hot-reloaded on save (or Ctrl+Shift+R). Errors show in a banner and the previous config is kept.
  - Linux: `~/.config/aeternus/config.toml`
  - Windows: `%APPDATA%\aeternus\config.toml`

## Performance

| | Aeternus | kitty |
|---|---|---|
| `cat` of 300k colored lines | 0.24 s | 0.32 s |
| `cat` of 3M lines | 0.81 s | 1.31 s |

Single runs on Linux, taken before wide character support; to be redone.

## Not supported yet

Composed emoji, live DPI changes, OSC 8 links, inline images, tabs/splits, line reflow on resize, DEC charsets, configurable keybindings.

## License

[MIT](LICENSE) © 2026 Gabriel (AeternalAngel)
