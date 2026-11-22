<div align="center">
  <img src="docs/aeternus-white.png#gh-dark-mode-only" alt="Aeternus" width="600">
  <img src="docs/aeternus-black.png#gh-light-mode-only" alt="Aeternus" width="600">

  <p>A GPU-accelerated terminal emulator for Linux, written in Rust (winit + wgpu).</p>

  <a href="https://github.com/AeternalAngel/aeternus/actions/workflows/ci.yml"><img src="https://github.com/AeternalAngel/aeternus/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/AeternalAngel/aeternus/releases"><img src="https://img.shields.io/github/v/release/AeternalAngel/aeternus?include_prereleases&sort=semver" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/AeternalAngel/aeternus" alt="MIT License"></a>
</div>

<div align="center">
  <img src="docs/terminal.png" alt="Aeternus screenshot" width="800">
</div>

## Features

- **Fast rendering** on the GPU (Vulkan, or OpenGL ES as a fallback) with a glyph atlas and vsync.
- **Tabs**, an optional **menu bar** and a **Preferences** panel with a built-in color picker.
- **Cursor trail** animation inspired by kitty.
- **Background transparency** (`window.opacity`), with solid text.
- **Text:** truecolor, 256 colors, bold/italic/strikethrough/blink, underline styles (double, curly, dotted, dashed) and colors, wide (CJK) characters, color emoji, combining accents, skin-tone modifiers and ZWJ emoji sequences, DEC line drawing.
- **Links:** `Ctrl+click` opens URLs and OSC 8 hyperlinks.
- **Compatibility:** alternate screen, bracketed paste, mouse reporting (1000/1002/1003/SGR), focus events, synchronized output (2026), XTGETTCAP, DECRQSS, kitty keyboard protocol (disambiguate level), cursor shapes, mouse pointer shapes (OSC 22), OSC 4/7/8/10/11/12/52, tab stops, insert/origin modes and more.
- **Terminfo:** ships an `xterm-aeternus` entry that declares styled and colored underlines, synchronized output and truecolor, with a fallback to `xterm-256color` when it isn't installed.
- **Shell integration:** prompt marks (OSC 133) to jump between prompts and select a command's output.
- **Desktop notifications** from programs (OSC 9, 99 and 777).
- **Daily use:** 10k-line scrollback with reflow on resize and search, word/line selection, clipboard and primary selection, zoom, fullscreen, visual bell.
- **Colors in kitty's format:** the same names as kitty (`foreground`, `selection_background`, `color0`…`color255`), kitty's default 16-color palette, and kitty `.conf` themes load directly.
- **Text rendering** with gamma-correct blending like kitty, tunable with `text_composition_strategy` using the same gamma and contrast curve as kitty.
- **Config:** a commented `config.toml`, reloaded automatically when it changes, with every shortcut configurable.
- **`aet` command** to control the running window from the shell.

## Install

Requires a Vulkan (or OpenGL ES 3) capable GPU driver and Wayland or X11. Release binaries need glibc 2.36 or newer (Debian 12, Ubuntu 23.04, Fedora 37 or later); on older systems, build from source.

Every [release](https://github.com/AeternalAngel/aeternus/releases) ships prebuilt packages for **x86_64** and **aarch64**:

| Format | x86_64 | aarch64 |
| --- | --- | --- |
| `.deb` (Debian, Ubuntu) | `amd64` | `arm64` |
| `.rpm` (Fedora, openSUSE) | `x86_64` | `aarch64` |
| AppImage | `x86_64` | `aarch64` |
| Tarball (any distribution) | `x86_64` | `aarch64` |
| `.pkg.tar.zst` (Arch Linux) | `x86_64` | (build it with `makepkg`) |

Pick the file that matches your architecture (`uname -m` tells you).

### Verify the download

Each release includes a `SHA256SUMS` file. Download it next to your package and run:

```bash
sha256sum -c SHA256SUMS --ignore-missing
```

### Debian / Ubuntu

```bash
sudo apt install ./aeternus_*_amd64.deb   # or _arm64.deb
```

### Fedora

```bash
sudo dnf install ./aeternus-*.x86_64.rpm  # or .aarch64.rpm
```

### openSUSE

```bash
sudo zypper install ./aeternus-*.x86_64.rpm  # or .aarch64.rpm
```

### AppImage

```bash
chmod +x aeternus-*-x86_64.AppImage   # or -aarch64
./aeternus-*-x86_64.AppImage
```

### Any distribution (tarball)

```bash
tar xzf aeternus-*-linux-x86_64.tar.gz   # or linux-aarch64
cd aeternus-*-linux-x86_64
./install.sh                    # installs to ~/.local (binaries, menu entry, icons, man pages, completions)
sudo ./install.sh --prefix /usr/local   # system-wide
./install.sh --uninstall        # remove it again
```

### Arch Linux

AUR registration is closed for now, so there is no AUR package yet; one is planned. Until then, pick one of these:

- Install the package attached to the release (x86_64):

  ```bash
  sudo pacman -U ./aeternus-bin-*-x86_64.pkg.tar.zst
  ```

- Build a package with `makepkg` from the PKGBUILDs in this repository, either `aeternus-bin` (downloads the prebuilt binary from the release, x86_64 or aarch64) or `aeternus` (compiles from source):

  ```bash
  git clone https://github.com/AeternalAngel/aeternus.git
  cd aeternus/packaging/aur/aeternus-bin   # or packaging/aur/aeternus
  makepkg -si
  ```

### From source

```bash
git clone https://github.com/AeternalAngel/aeternus.git
cd aeternus
./install.sh          # builds with cargo, then installs to ~/.local
```

Building needs Rust 1.90 or newer. For a system-wide install, run `cargo build --release` first and then `sudo ./install.sh --prefix /usr/local`. The binaries end up in `target/release/aeternus` and `target/release/aet`.

## Configuration

The file lives at `~/.config/aeternus/config.toml` (or `$XDG_CONFIG_HOME/aeternus/config.toml`) and is created with every option commented out on the first run. Changes are applied as soon as the file is saved; if it has errors, the previous configuration is kept and the errors are logged. Most options can also be changed in **Edit → Preferences**.

```toml
[font]
family = "JetBrainsMono Nerd Font"
size = 14.0

[window]
toolbar = true
opacity = 0.9             # background transparency

[terminal]
shell = "zsh -l"
smart_copy_paste = true   # Ctrl+C copies a selection, Ctrl+V pastes

[cursor]
shape = "bar"
trail = 3                 # enable the cursor trail
trail_decay = [0.1, 0.4]

[colors]
theme = "~/.config/kitty/current-theme.conf"   # any kitty theme works as is
background = "#1e1e2e"    # kitty names; anything here overrides the theme
color1 = "#f38ba8"

[keybindings]
new_tab = ["ctrl+shift+t", "ctrl+t"]
fullscreen = []           # disable F11
```

Use `aeternus --config <path>` to load a different file. Errors and warnings are written to `~/.local/state/aeternus/aeternus.log`.

## Shortcuts

| Action | Default | Config key |
| --- | --- | --- |
| Copy / paste | `Ctrl+Shift+C` / `Ctrl+Shift+V` | `copy`, `paste` |
| Paste primary selection | `Shift+Insert`, middle click | `paste_primary` |
| New tab / close tab | `Ctrl+Shift+T` / `Ctrl+Shift+W` | `new_tab`, `close_tab` |
| Next / previous tab | `Ctrl+Tab`, `Ctrl+PageDown` / `Ctrl+Shift+Tab`, `Ctrl+PageUp` | `next_tab`, `prev_tab` |
| Zoom in / out / reset | `Ctrl+Shift+=` / `Ctrl+Shift+-` / `Ctrl+Shift+0`, `Ctrl+wheel` | `zoom_in`, `zoom_out`, `zoom_reset` |
| Scroll history | `Shift+PageUp` / `Shift+PageDown` / `Shift+Home` / `Shift+End` | `scroll_page_up`, ... |
| Search the scrollback | `Ctrl+Shift+F`; then `Enter`/`Shift+Enter` for previous/next, `Esc` to close | `search` |
| Previous / next prompt | `Ctrl+Shift+Z` / `Ctrl+Shift+X` | `prev_prompt`, `next_prompt` |
| Select a command's output | `Ctrl+Shift+G` (again for the one before) | `select_output` |
| Reload config | `Ctrl+Shift+R` | `reload_config` |
| Fullscreen | `F11` | `fullscreen` |
| Preferences | (none) | `preferences` |
| Open link | `Ctrl+click` (`Ctrl+Shift+click` in mouse-aware apps) | |

## Shell integration

Jumping between prompts and selecting a command's output need the shell to mark its prompts with OSC 133. fish 4.0 and later do it on their own. For zsh, add to `~/.zshrc`:

```zsh
autoload -Uz add-zsh-hook
_aeternus_prompt() { print -n '\e]133;A\a' }
_aeternus_output() { print -n '\e]133;C\a' }
add-zsh-hook precmd _aeternus_prompt
add-zsh-hook preexec _aeternus_output
```

For bash 4.4 or newer, add to the end of `~/.bashrc`:

```bash
PS1='\[\e]133;A\a\]'"$PS1"
PS0=$'\e]133;C\a'
```

`Ctrl+Shift+G` selects the output of the last command, or of the last one on screen when scrolled back, and puts it in the primary selection; press it again to go one command back, and `Ctrl+Shift+C` to copy it.

Programs can show desktop notifications with OSC 9, OSC 777 (`notify;title;body`) or kitty's OSC 99. They are shown with `notify-send` (from libnotify), and `notifications = false` in `[terminal]` turns them off.

## Terminfo

Aeternus ships an `xterm-aeternus` terminfo entry, based on `xterm-256color`, that also declares styled and colored underlines (`Smulx`, `Setulc`), synchronized output (`Sync`) and truecolor (`Tc`), so programs like tmux and Neovim can use them. The packages and `install.sh` install it, and the shell gets `TERM=xterm-aeternus` whenever the entry is found; otherwise it falls back to `TERM=xterm-256color`. Set `term` in `[terminal]` to force a value.

Hosts you reach over SSH don't have the entry. Copy it once with:

```bash
infocmp -x xterm-aeternus | ssh host -- tic -x -
```

or keep `xterm-256color` for SSH with `SetEnv TERM=xterm-256color` in `~/.ssh/config`. With a user install (`~/.local`), the entry is found through `TERMINFO`, which `sudo` does not keep; install system-wide if programs run under `sudo` complain about an unknown terminal.

## The `aet` command

`aet` talks to the most recently focused Aeternus window over a socket in `$XDG_RUNTIME_DIR`:

```bash
aet version   # print the version in the active terminal
aet pref      # open Preferences
aet reload    # reload config.toml
```

## Performance

`aeternus --bench` measures the escape-sequence parser on 64 MB of input. On an Intel Xeon E3-1230 V2 (2012):

| Workload | Throughput |
| --- | --- |
| Plain text | 184 MB/s |
| Colored text | 125 MB/s |
| Full-screen TUI redraws | 118 MB/s |

## Not supported yet

Splits, inline images (sixel / kitty graphics), IME composition (pre-edit) for CJK input, regional-indicator flag emoji, the full kitty keyboard protocol (only the first level), OSC 52 clipboard reading.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Bugs and ideas go to [Issues](https://github.com/AeternalAngel/aeternus/issues); security problems to [SECURITY.md](SECURITY.md).

## Special Thanks <3

### Contributors
* **[henryiwakura](https://github.com/henryiwakura)** - Helped with many aspects of the project and added features I wouldn't have even known how to build.

### Inspiration & Support
* **[SertralinaS2](https://github.com/SertralinaS2)** - Kept me motivated to push through and not give up on the project.
* **[Kitty](https://github.com/kovidgoyal/kitty)** - The terminal I used for so long that inspired me to build my own.

## License

[MIT](LICENSE) © 2026 Gabriel (AeternalAngel)
