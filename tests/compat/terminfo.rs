use std::process::Command;

use aeternus::input::{key_bytes, KeyInput, KeyModes};
use winit::keyboard::{Key, KeyLocation, ModifiersState, NamedKey};

use crate::harness::{term_env, uses_aeternus_terminfo, Session};
use crate::require;

fn cap(name: &str) -> Option<Vec<u8>> {
    let env = term_env();
    let mut cmd = Command::new("tput");
    cmd.arg("-T").arg(&env.term).arg(name);
    if let Some(dir) = &env.terminfo {
        cmd.env("TERMINFO", dir);
    }
    let out = cmd.output().ok()?;
    (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout)
}

fn sent(key: NamedKey, mods: ModifiersState) -> Vec<u8> {
    let input = KeyInput { key: Key::Named(key), base: None, text: None, location: KeyLocation::Standard };
    let modes = KeyModes { app_cursor: true, app_keypad: true, kitty: 0 };
    key_bytes(&input, mods, modes).unwrap()
}

#[test]
fn key_capabilities_match_what_the_terminal_sends() {
    require!("tput", "tic");
    if !uses_aeternus_terminfo() {
        eprintln!("skipped: xterm-aeternus could not be compiled");
        return;
    }
    let none = ModifiersState::empty();
    let shift = ModifiersState::SHIFT;
    let ctrl = ModifiersState::CONTROL;
    let keys = [
        ("kcuu1", NamedKey::ArrowUp, none),
        ("kcud1", NamedKey::ArrowDown, none),
        ("kcuf1", NamedKey::ArrowRight, none),
        ("kcub1", NamedKey::ArrowLeft, none),
        ("khome", NamedKey::Home, none),
        ("kend", NamedKey::End, none),
        ("kich1", NamedKey::Insert, none),
        ("kdch1", NamedKey::Delete, none),
        ("kpp", NamedKey::PageUp, none),
        ("knp", NamedKey::PageDown, none),
        ("kbs", NamedKey::Backspace, none),
        ("kf1", NamedKey::F1, none),
        ("kf2", NamedKey::F2, none),
        ("kf3", NamedKey::F3, none),
        ("kf4", NamedKey::F4, none),
        ("kf5", NamedKey::F5, none),
        ("kf6", NamedKey::F6, none),
        ("kf7", NamedKey::F7, none),
        ("kf8", NamedKey::F8, none),
        ("kf9", NamedKey::F9, none),
        ("kf10", NamedKey::F10, none),
        ("kf11", NamedKey::F11, none),
        ("kf12", NamedKey::F12, none),
        ("kLFT", NamedKey::ArrowLeft, shift),
        ("kRIT", NamedKey::ArrowRight, shift),
        ("kHOM", NamedKey::Home, shift),
        ("kEND", NamedKey::End, shift),
        ("kDC", NamedKey::Delete, shift),
        ("kf13", NamedKey::F1, shift),
        ("kf17", NamedKey::F5, shift),
        ("kf25", NamedKey::F1, ctrl),
        ("kUP5", NamedKey::ArrowUp, ctrl),
        ("kDN5", NamedKey::ArrowDown, ctrl),
        ("kLFT5", NamedKey::ArrowLeft, ctrl),
        ("kRIT5", NamedKey::ArrowRight, ctrl),
    ];
    let mut bad = Vec::new();
    for (name, key, mods) in keys {
        let Some(want) = cap(name) else { continue };
        let got = sent(key, mods);
        if got != want {
            bad.push(format!(
                "{name}: terminfo {:?}, terminal sends {:?}",
                String::from_utf8_lossy(&want),
                String::from_utf8_lossy(&got)
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn programs_see_the_entry_and_its_extensions() {
    require!("sh", "tput", "infocmp");
    let s = Session::spawn(&["sh"], 80, 24);
    s.type_str("echo \"term=$TERM colors=$(tput colors) cols=$(tput cols) lines=$(tput lines)\"\n");
    s.wait_text(&format!("term={} colors=256 cols=80 lines=24", term_env().term));
    if uses_aeternus_terminfo() {
        s.type_str("for c in Tc Su Smulx Setulc Sync Ss Se Cs Cr; do infocmp -x | grep -q \"[[:space:]]$c[=,]\" && printf '%s ' $c; done; echo end\n");
        s.wait_text("Tc Su Smulx Setulc Sync Ss Se Cs Cr end");
    }
}

#[test]
fn capabilities_render_as_declared() {
    require!("sh", "tput");
    let s = Session::spawn(&["sh"], 80, 24);
    s.type_str("clear; tput bold; printf B; tput sgr0; tput smul; printf U; tput sgr0; tput rev; printf R; tput sgr0; tput setaf 1; printf F; tput setab 4; printf G; tput sgr0; tput smacs; printf qx; tput rmacs; tput cup 5 10; printf here; echo\n");
    s.wait_row(5, "          here");
    s.settle();
    let (r, c) = s.find("BURFG").expect("styled text");
    use aeternus::cell::{flags, Color};
    assert_ne!(s.cell(r, c).flags & flags::BOLD, 0);
    assert_ne!(s.cell(r, c + 1).flags & flags::UNDERLINE, 0);
    assert_ne!(s.cell(r, c + 2).flags & flags::INVERSE, 0);
    assert_eq!(s.cell(r, c + 3).fg, Color::Indexed(1));
    assert_eq!(s.cell(r, c + 4).bg, Color::Indexed(4));
    assert_eq!(s.cell(r, c + 5).ch, '─');
    assert_eq!(s.cell(r, c + 6).ch, '│');
    assert_eq!(s.find("here"), Some((5, 10)));
    if uses_aeternus_terminfo() {
        s.type_str(
            "clear; tput Smulx 3; printf C; tput sgr0; tput Setulc 0x102030; printf L; tput sgr0; echo\n",
        );
        s.wait_text("CL");
        let (r, c) = s.find("CL").unwrap();
        assert_eq!(s.cell(r, c).flags & flags::UNDERLINE_STYLE, flags::UNDERLINE_CURLY);
        assert_eq!(s.with(|t| t.underline_color(&t.grid.row(r)[c + 1])), Some(Color::Rgb(0x10, 0x20, 0x30)));
    }
}

#[test]
fn saves_restores_and_wraps_as_declared() {
    require!("sh", "tput");
    let s = Session::spawn(&["sh"], 80, 24);
    s.type_str("clear; tput cup 3 7; tput sc; tput cup 10 0; printf moved; tput rc; printf back; tput cup 12 0; printf '%085d' 0; tput cup 20 0; echo done\n");
    s.wait_row(20, "done");
    s.settle();
    assert_eq!(s.find("back"), Some((3, 7)), "{}", s.dump());
    assert!(s.with(|t| t.grid.row(12).iter().all(|c| c.ch == '0')), "{}", s.dump());
    assert_eq!(s.row(13), "00000", "{}", s.dump());
    s.type_str("tput am && echo has-am; tput sc | od -An -c | tr -d ' '\n");
    s.wait_line("has-am");
    s.wait_line("0337");
}
