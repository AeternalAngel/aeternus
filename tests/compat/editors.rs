use aeternus::cell::{flags, Color};
use aeternus::term::{CursorShape, MouseMode};
use winit::keyboard::NamedKey;

use crate::harness::{uses_aeternus_terminfo, Session};
use crate::require;

fn nvim_version() -> (u32, u32) {
    let out = std::process::Command::new("nvim").arg("--version").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let v = text.split_whitespace().nth(1).unwrap_or("v0.0").trim_start_matches('v');
    let mut it = v.split('.').map(|n| n.parse().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0))
}

fn editor(cmd: &str) -> Session {
    let s = Session::spawn(
        &["sh", "-c", &format!("echo before-editor; {cmd}; echo after-editor; exec sleep 60")],
        80,
        24,
    );
    s.wait("the editor to take the alternate screen", |t| t.is_alt_screen());
    s.wait_text("~");
    s.settle();
    s
}

fn ex(s: &Session, cmd: &str) {
    s.key(NamedKey::Escape);
    s.type_str(&format!(":{cmd}"));
    s.key(NamedKey::Enter);
}

fn edits_and_restores_the_screen(cmd: &str) {
    let s = editor(cmd);
    s.wait_text("file.txt");
    s.type_str("ihello world");
    s.key(NamedKey::Escape);
    s.wait_row(0, "hello world");
    s.type_str("ofoo");
    s.key(NamedKey::ArrowLeft);
    s.type_str("X");
    s.key(NamedKey::Escape);
    s.wait_row(1, "foXo");
    ex(&s, "wq");
    s.wait_text("after-editor");
    assert!(!s.with(|t| t.is_alt_screen()));
    assert_eq!(s.row(0), "before-editor");
    assert_eq!(std::fs::read_to_string(s.home.join("file.txt")).unwrap(), "hello world\nfoXo\n");
}

fn pastes_without_autoindent(cmd: &str) {
    let s = editor(cmd);
    s.wait("bracketed paste to be enabled", |t| t.bracketed_paste);
    ex(&s, "set autoindent");
    s.type_str("i");
    s.send("\x1b[200~fn main() {\n    body();\n}\x1b[201~");
    s.key(NamedKey::Escape);
    s.wait_row(2, "}");
    assert_eq!(s.row(0), "fn main() {");
    assert_eq!(s.row(1), "    body();");
}

#[test]
fn nvim_edits_and_restores_the_screen() {
    require!("nvim", "sh");
    edits_and_restores_the_screen("nvim --clean -n file.txt");
}

#[test]
fn vim_edits_and_restores_the_screen() {
    require!("vim", "sh");
    edits_and_restores_the_screen("vim --clean -n file.txt");
}

#[test]
fn nvim_pastes_without_autoindent() {
    require!("nvim", "sh");
    pastes_without_autoindent("nvim --clean -n");
}

#[test]
fn vim_pastes_without_autoindent() {
    require!("vim", "sh");
    pastes_without_autoindent("vim --clean -n");
}

#[test]
fn nvim_sets_the_cursor_shape_per_mode() {
    require!("nvim", "sh");
    let s = editor("nvim --clean -n");
    s.wait("a block cursor in normal mode", |t| t.cursor_shape == CursorShape::Block);
    s.type_str("i");
    s.wait("a bar cursor in insert mode", |t| t.cursor_shape == CursorShape::Bar);
    s.key(NamedKey::Escape);
    s.type_str("R");
    s.wait("an underline cursor in replace mode", |t| t.cursor_shape == CursorShape::Underline);
    s.key(NamedKey::Escape);
    s.wait("a block cursor again", |t| t.cursor_shape == CursorShape::Block);
}

#[test]
fn nvim_uses_truecolor_and_styled_underlines() {
    require!("nvim", "sh");
    let s = editor("nvim --clean -n -c 'set termguicolors' -c 'hi Normal guifg=#102030 guibg=#405060' -c 'hi Bad gui=undercurl guisp=#ff0000' -c 'call matchadd(\"Bad\", \"oops\")'");
    s.type_str("ifine oops");
    s.key(NamedKey::Escape);
    s.wait_row(0, "fine oops");
    s.settle();
    let c = s.cell(0, 0);
    assert_eq!((c.fg, c.bg), (Color::Rgb(0x10, 0x20, 0x30), Color::Rgb(0x40, 0x50, 0x60)), "{}", s.dump());
    if uses_aeternus_terminfo() {
        let bad = s.cell(0, 5);
        assert_eq!(bad.flags & flags::UNDERLINE_STYLE, flags::UNDERLINE_CURLY);
        assert_eq!(s.with(|t| t.underline_color(&bad)), Some(Color::Rgb(255, 0, 0)));
    }
}

#[test]
fn nvim_enables_mouse_and_the_kitty_keyboard_protocol() {
    require!("nvim", "sh");
    let s = editor("nvim --clean -n");
    s.wait("mouse reporting", |t| t.mouse_mode != MouseMode::Off && t.mouse_sgr);
    let kitty = nvim_version() >= (0, 10);
    if kitty {
        s.wait("the kitty keyboard protocol", |t| t.kb_flags & 1 != 0);
    }
    s.type_str("ione\ntwo\nthree");
    s.key(NamedKey::Escape);
    s.wait_row(2, "three");
    s.send("\x1b[<0;3;1M\x1b[<0;3;1m");
    s.type_str("x");
    s.wait("the click to move the cursor onto the last letter", |t| {
        crate::harness::row_text(t.grid.row(0)) == "on"
    });
    ex(&s, "q!");
    s.wait_text("after-editor");
    assert_eq!(s.with(|t| t.kb_flags), 0);
    assert_eq!(s.with(|t| t.mouse_mode), MouseMode::Off);
}

#[test]
fn nvim_follows_resizes() {
    require!("nvim", "sh");
    let s = editor("nvim --clean -n");
    s.resize(100, 30);
    ex(&s, "echo 'size=' . &columns . 'x' . &lines");
    s.wait_row(29, "size=100x30");
}
