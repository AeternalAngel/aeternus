use winit::keyboard::{Key, KeyLocation, ModifiersState, NamedKey};

use crate::config::KeyCombo;

#[derive(Clone, Debug)]
pub struct KeyInput {
    pub key: Key,
    pub base: Option<char>,
    pub text: Option<String>,
    pub location: KeyLocation,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct KeyModes {
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub kitty: u8,
}

fn named_key_alias(key: &str) -> Option<NamedKey> {
    Some(match key {
        "pageup" => NamedKey::PageUp,
        "pagedown" => NamedKey::PageDown,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "insert" => NamedKey::Insert,
        "delete" => NamedKey::Delete,
        "enter" => NamedKey::Enter,
        "escape" => NamedKey::Escape,
        "tab" => NamedKey::Tab,
        "space" => NamedKey::Space,
        "backspace" => NamedKey::Backspace,
        "up" => NamedKey::ArrowUp,
        "down" => NamedKey::ArrowDown,
        "left" => NamedKey::ArrowLeft,
        "right" => NamedKey::ArrowRight,
        "f1" => NamedKey::F1,
        "f2" => NamedKey::F2,
        "f3" => NamedKey::F3,
        "f4" => NamedKey::F4,
        "f5" => NamedKey::F5,
        "f6" => NamedKey::F6,
        "f7" => NamedKey::F7,
        "f8" => NamedKey::F8,
        "f9" => NamedKey::F9,
        "f10" => NamedKey::F10,
        "f11" => NamedKey::F11,
        "f12" => NamedKey::F12,
        _ => return None,
    })
}

fn char_alias(key: &str) -> Option<&'static str> {
    Some(match key {
        "plus" => "+",
        "equal" => "=",
        "minus" => "-",
        "underscore" => "_",
        "zero" => "0",
        "paren_right" => ")",
        _ => return None,
    })
}

pub fn combo_matches(combo: &KeyCombo, key: &KeyInput, mods: ModifiersState) -> bool {
    if combo.shift != mods.shift_key()
        || combo.ctrl != mods.control_key()
        || combo.alt != mods.alt_key()
        || combo.logo != mods.super_key()
    {
        return false;
    }
    if let Some(named) = named_key_alias(&combo.key) {
        return key.key == Key::Named(named);
    }
    let want = char_alias(&combo.key).unwrap_or(&combo.key);
    match &key.key {
        Key::Character(s) => {
            s.as_str().eq_ignore_ascii_case(want)
                || key.base.is_some_and(|b| want.chars().count() == 1 && want.starts_with(b))
        }
        _ => false,
    }
}

pub fn mouse_report(
    sgr: bool,
    button: u8,
    row: usize,
    col: usize,
    release: bool,
    motion: bool,
    mods: ModifiersState,
) -> Option<Vec<u8>> {
    let mut cb = if release && !sgr { 3 } else { button };
    if motion {
        cb += 32;
    }
    if mods.alt_key() {
        cb += 8;
    }
    if mods.control_key() {
        cb += 16;
    }
    if sgr {
        let end = if release { 'm' } else { 'M' };
        Some(format!("\x1b[<{};{};{}{}", cb, col + 1, row + 1, end).into_bytes())
    } else {
        if col >= 222 || row >= 222 {
            return None;
        }
        Some(vec![0x1b, b'[', b'M', 32 + cb, 33 + col as u8, 33 + row as u8])
    }
}

fn modifier_param(mods: ModifiersState) -> u8 {
    1 + mods.shift_key() as u8
        + 2 * mods.alt_key() as u8
        + 4 * mods.control_key() as u8
        + 8 * mods.super_key() as u8
}

fn csi_u(code: u32, m: u8) -> Vec<u8> {
    if m > 1 {
        format!("\x1b[{code};{m}u").into_bytes()
    } else {
        format!("\x1b[{code}u").into_bytes()
    }
}

fn kitty_bytes(k: &KeyInput, mods: ModifiersState) -> Option<Vec<u8>> {
    let m = modifier_param(mods);
    let non_shift = mods.control_key() || mods.alt_key() || mods.super_key();
    match &k.key {
        Key::Named(NamedKey::Escape) => Some(csi_u(27, m)),
        Key::Named(NamedKey::Enter) if m > 1 => Some(csi_u(13, m)),
        Key::Named(NamedKey::Tab) if m > 1 => Some(csi_u(9, m)),
        Key::Named(NamedKey::Backspace) if m > 1 => Some(csi_u(127, m)),
        Key::Named(NamedKey::Space) if non_shift => Some(csi_u(32, m)),
        Key::Character(s) if non_shift => {
            let c = k.base.or_else(|| s.chars().next())?;
            let c = c.to_lowercase().next().unwrap_or(c);
            Some(csi_u(c as u32, m))
        }
        _ => None,
    }
}

pub fn key_bytes(k: &KeyInput, mods: ModifiersState, modes: KeyModes) -> Option<Vec<u8>> {
    if modes.kitty & 1 != 0 {
        if let Some(b) = kitty_bytes(k, mods) {
            return Some(b);
        }
    }
    let m = modifier_param(mods);
    let modified = m > 1;
    let csi_final = |c: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{}{}", m, c as char).into_bytes()
        } else if modes.app_cursor {
            vec![0x1b, b'O', c]
        } else {
            vec![0x1b, b'[', c]
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{};{}~", n, m).into_bytes()
        } else {
            format!("\x1b[{}~", n).into_bytes()
        }
    };
    let ss3 = |c: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{}{}", m, c as char).into_bytes()
        } else {
            vec![0x1b, b'O', c]
        }
    };
    let alt_only = mods.alt_key() && !mods.shift_key() && !mods.control_key() && !mods.super_key();
    if alt_only && modes.kitty == 0 {
        match &k.key {
            Key::Named(NamedKey::ArrowLeft) => return Some(vec![0x1b, b'b']),
            Key::Named(NamedKey::ArrowRight) => return Some(vec![0x1b, b'f']),
            _ => {}
        }
    }
    if modes.app_keypad && !modified && k.location == KeyLocation::Numpad {
        let code = match &k.key {
            Key::Named(NamedKey::Enter) => Some(b'M'),
            Key::Character(s) if s.len() == 1 => match s.as_bytes()[0] {
                d @ b'0'..=b'9' => Some(b'p' + (d - b'0')),
                b'.' => Some(b'n'),
                b',' => Some(b'l'),
                b'+' => Some(b'k'),
                b'-' => Some(b'm'),
                b'*' => Some(b'j'),
                b'/' => Some(b'o'),
                b'=' => Some(b'X'),
                _ => None,
            },
            _ => None,
        };
        if let Some(c) = code {
            return Some(vec![0x1b, b'O', c]);
        }
    }
    if let Key::Named(n) = &k.key {
        let special = match n {
            NamedKey::ArrowUp => Some(csi_final(b'A')),
            NamedKey::ArrowDown => Some(csi_final(b'B')),
            NamedKey::ArrowRight => Some(csi_final(b'C')),
            NamedKey::ArrowLeft => Some(csi_final(b'D')),
            NamedKey::Home => Some(csi_final(b'H')),
            NamedKey::End => Some(csi_final(b'F')),
            NamedKey::Insert => Some(tilde(2)),
            NamedKey::Delete => Some(tilde(3)),
            NamedKey::PageUp => Some(tilde(5)),
            NamedKey::PageDown => Some(tilde(6)),
            NamedKey::F1 => Some(ss3(b'P')),
            NamedKey::F2 => Some(ss3(b'Q')),
            NamedKey::F3 => Some(ss3(b'R')),
            NamedKey::F4 => Some(ss3(b'S')),
            NamedKey::F5 => Some(tilde(15)),
            NamedKey::F6 => Some(tilde(17)),
            NamedKey::F7 => Some(tilde(18)),
            NamedKey::F8 => Some(tilde(19)),
            NamedKey::F9 => Some(tilde(20)),
            NamedKey::F10 => Some(tilde(21)),
            NamedKey::F11 => Some(tilde(23)),
            NamedKey::F12 => Some(tilde(24)),
            _ => None,
        };
        if special.is_some() {
            return special;
        }
    }
    let mut out = match &k.key {
        Key::Named(n) => match n {
            NamedKey::Enter => b"\r".to_vec(),
            NamedKey::Backspace => {
                if mods.control_key() {
                    vec![0x08]
                } else {
                    vec![0x7f]
                }
            }
            NamedKey::Tab => {
                if mods.shift_key() {
                    b"\x1b[Z".to_vec()
                } else {
                    b"\t".to_vec()
                }
            }
            NamedKey::Escape => vec![0x1b],
            NamedKey::Space => {
                if mods.control_key() {
                    vec![0x00]
                } else {
                    b" ".to_vec()
                }
            }
            _ => k.text.as_ref()?.as_bytes().to_vec(),
        },
        Key::Character(s) if mods.control_key() && s.as_str() == " " => vec![0x00],
        Key::Character(s) if mods.control_key() && s.as_str() == "/" => vec![0x1f],
        Key::Character(s) if mods.control_key() && s.len() == 1 => {
            let b = s.as_bytes()[0];
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'['..=b'_' => vec![b & 0x1f],
                b'2' | b'@' => vec![0x00],
                b'3'..=b'7' => vec![b - b'3' + 0x1b],
                b'8' | b'?' => vec![0x7f],
                _ => match k.base {
                    Some(c @ 'a'..='z') => vec![c as u8 & 0x1f],
                    _ => k.text.as_ref()?.as_bytes().to_vec(),
                },
            }
        }
        Key::Character(_) if mods.control_key() => match k.base {
            Some(c @ 'a'..='z') => vec![c as u8 & 0x1f],
            _ => k.text.as_ref()?.as_bytes().to_vec(),
        },
        _ => k.text.as_ref()?.as_bytes().to_vec(),
    };
    if out.is_empty() {
        return None;
    }
    if mods.alt_key() {
        out.insert(0, 0x1b);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    fn ch(s: &str) -> KeyInput {
        KeyInput {
            key: Key::Character(SmolStr::new(s)),
            base: s.chars().next().map(|c| c.to_ascii_lowercase()),
            text: Some(s.to_string()),
            location: KeyLocation::Standard,
        }
    }
    fn named(n: NamedKey) -> KeyInput {
        KeyInput { key: Key::Named(n), base: None, text: None, location: KeyLocation::Standard }
    }
    fn mods(ctrl: bool, shift: bool, alt: bool) -> ModifiersState {
        let mut m = ModifiersState::empty();
        m.set(ModifiersState::CONTROL, ctrl);
        m.set(ModifiersState::SHIFT, shift);
        m.set(ModifiersState::ALT, alt);
        m
    }
    fn legacy() -> KeyModes {
        KeyModes::default()
    }

    #[test]
    fn plain_text() {
        assert_eq!(key_bytes(&ch("a"), mods(false, false, false), legacy()), Some(b"a".to_vec()));
    }

    #[test]
    fn ctrl_letters_and_symbols() {
        let m = mods(true, false, false);
        assert_eq!(key_bytes(&ch("c"), m, legacy()), Some(vec![3]));
        assert_eq!(key_bytes(&ch("v"), m, legacy()), Some(vec![0x16]));
        assert_eq!(key_bytes(&ch("["), m, legacy()), Some(vec![0x1b]));
        assert_eq!(key_bytes(&ch(" "), m, legacy()), Some(vec![0]));
        assert_eq!(key_bytes(&ch("/"), m, legacy()), Some(vec![0x1f]));
    }

    #[test]
    fn alt_prefixes_escape() {
        assert_eq!(key_bytes(&ch("x"), mods(false, false, true), legacy()), Some(b"\x1bx".to_vec()));
        assert_eq!(
            key_bytes(&named(NamedKey::ArrowLeft), mods(false, false, true), legacy()),
            Some(b"\x1bb".to_vec())
        );
    }

    #[test]
    fn arrows_respect_app_cursor_and_modifiers() {
        let none = mods(false, false, false);
        assert_eq!(key_bytes(&named(NamedKey::ArrowUp), none, legacy()), Some(b"\x1b[A".to_vec()));
        let app = KeyModes { app_cursor: true, ..legacy() };
        assert_eq!(key_bytes(&named(NamedKey::ArrowUp), none, app), Some(b"\x1bOA".to_vec()));
        assert_eq!(
            key_bytes(&named(NamedKey::ArrowUp), mods(true, false, false), app),
            Some(b"\x1b[1;5A".to_vec())
        );
    }

    #[test]
    fn function_and_editing_keys() {
        let none = mods(false, false, false);
        assert_eq!(key_bytes(&named(NamedKey::F1), none, legacy()), Some(b"\x1bOP".to_vec()));
        assert_eq!(key_bytes(&named(NamedKey::F5), none, legacy()), Some(b"\x1b[15~".to_vec()));
        assert_eq!(
            key_bytes(&named(NamedKey::Delete), mods(false, true, false), legacy()),
            Some(b"\x1b[3;2~".to_vec())
        );
        assert_eq!(
            key_bytes(&named(NamedKey::Tab), mods(false, true, false), legacy()),
            Some(b"\x1b[Z".to_vec())
        );
    }

    #[test]
    fn keypad_application_mode() {
        let k = KeyInput {
            key: Key::Character(SmolStr::new("5")),
            base: Some('5'),
            text: Some("5".into()),
            location: KeyLocation::Numpad,
        };
        let app = KeyModes { app_keypad: true, ..legacy() };
        assert_eq!(key_bytes(&k, mods(false, false, false), app), Some(b"\x1bOu".to_vec()));
        assert_eq!(key_bytes(&k, mods(false, false, false), legacy()), Some(b"5".to_vec()));
    }

    #[test]
    fn kitty_disambiguate() {
        let k = KeyModes { kitty: 1, ..legacy() };
        assert_eq!(
            key_bytes(&named(NamedKey::Escape), mods(false, false, false), k),
            Some(b"\x1b[27u".to_vec())
        );
        assert_eq!(key_bytes(&ch("c"), mods(true, false, false), k), Some(b"\x1b[99;5u".to_vec()));
        assert_eq!(key_bytes(&ch("i"), mods(true, false, false), k), Some(b"\x1b[105;5u".to_vec()));
        assert_eq!(key_bytes(&ch("a"), mods(false, false, true), k), Some(b"\x1b[97;3u".to_vec()));
        assert_eq!(key_bytes(&ch("C"), mods(true, true, false), k), Some(b"\x1b[99;6u".to_vec()));
        assert_eq!(key_bytes(&named(NamedKey::Enter), mods(false, false, false), k), Some(b"\r".to_vec()));
        assert_eq!(
            key_bytes(&named(NamedKey::Enter), mods(false, true, false), k),
            Some(b"\x1b[13;2u".to_vec())
        );
        assert_eq!(key_bytes(&ch("A"), mods(false, true, false), k), Some(b"A".to_vec()));
        assert_eq!(
            key_bytes(&named(NamedKey::ArrowUp), mods(false, false, false), k),
            Some(b"\x1b[A".to_vec())
        );
    }

    #[test]
    fn mouse_reports() {
        let none = mods(false, false, false);
        assert_eq!(mouse_report(true, 0, 4, 9, false, false, none), Some(b"\x1b[<0;10;5M".to_vec()));
        assert_eq!(mouse_report(true, 0, 4, 9, true, false, none), Some(b"\x1b[<0;10;5m".to_vec()));
        assert_eq!(mouse_report(false, 0, 0, 0, true, false, none), Some(vec![0x1b, b'[', b'M', 35, 33, 33]));
        assert_eq!(mouse_report(false, 0, 0, 300, false, false, none), None);
        assert_eq!(
            mouse_report(true, 64, 0, 0, false, false, mods(true, false, false)),
            Some(b"\x1b[<80;1;1M".to_vec())
        );
    }

    #[test]
    fn combos() {
        let c = KeyCombo::parse("ctrl+shift+t").unwrap();
        assert!(combo_matches(&c, &ch("T"), mods(true, true, false)));
        assert!(!combo_matches(&c, &ch("T"), mods(true, false, false)));
        let z = KeyCombo::parse("ctrl+shift+plus").unwrap();
        assert!(combo_matches(&z, &ch("+"), mods(true, true, false)));
        let f = KeyCombo::parse("f11").unwrap();
        assert!(combo_matches(&f, &named(NamedKey::F11), mods(false, false, false)));
        let tab = KeyCombo::parse("ctrl+shift+tab").unwrap();
        assert!(combo_matches(&tab, &named(NamedKey::Tab), mods(true, true, false)));
    }
}
