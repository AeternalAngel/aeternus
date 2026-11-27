# Contributing to Aeternus

Thanks for taking the time to help. Bug reports, ideas and pull requests are all welcome.

## Reporting bugs

Open an issue with the bug report form. Please include the output of `aeternus --version`, your
distribution, Wayland or X11, the GPU and the relevant lines from
`~/.local/state/aeternus/aeternus.log`.

Security problems should be reported privately, see [SECURITY.md](SECURITY.md).

## Building

You need a recent stable Rust toolchain (the minimum supported version is in `Cargo.toml`,
`rust-version`).

```bash
cargo build            # debug build
cargo run              # run it
cargo build --release  # optimized build in target/release
```

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same commands, plus a build with the minimum supported Rust version, `cargo deny`
and validation of the desktop entry, the AppStream metadata and `install.sh`.

### Compatibility tests

`tests/compat` runs real programs (bash, zsh, fish, dash, tmux, Neovim, Vim, less, htop and
vttest) in a pseudo-terminal connected to Aeternus's parser and screen, with no window, and checks
what they draw and how they react to keys, mouse clicks, pastes and resizes. Each program runs
without its configuration files and with a temporary `HOME`. Tests for programs that are not
installed are skipped; set `AETERNUS_COMPAT_REQUIRE=1` to make them fail instead, as CI does.

```bash
cargo test --test compat
```

Please also:

- keep pull requests focused on one change;
- add a test when you fix a parser or terminal bug (see the tests at the end of `src/term.rs`);
- add a line to the `## [Unreleased]` section of [CHANGELOG.md](CHANGELOG.md) for user-visible
  changes.

## Code layout

| Path | What lives there |
| --- | --- |
| `src/parser.rs` | VT escape sequence parser |
| `src/term.rs` | Terminal state: grid, scrollback, modes, selection |
| `src/input.rs` | Keyboard and mouse encoding |
| `src/render/` | wgpu renderer, menus and tabs, preferences, cursor trail |
| `src/app.rs` | Window, event loop, tabs and the `aet` socket |
| `src/config.rs` | `config.toml` loading and saving |
| `src/bin/aet.rs` | The `aet` companion command |
| `packaging/` | Desktop entry, AppStream metadata, icons, man pages, completions, AUR |

## Fuzzing

`fuzz/` contains a [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) target for the parser:

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run parser
```

## License

By contributing you agree that your work is released under the [MIT License](LICENSE).
