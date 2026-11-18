use std::path::{Path, PathBuf};

use crate::cell::Color;

pub const DEFAULT_FG: [u8; 3] = [239, 220, 249];
pub const DEFAULT_BG: [u8; 3] = [18, 10, 24];
pub const DEFAULT_SELECTION: [u8; 3] = [88, 58, 126];
const DEFAULT_ANSI: [[u8; 3]; 16] = [
    [0, 0, 0],
    [204, 4, 3],
    [25, 203, 0],
    [206, 203, 0],
    [13, 115, 204],
    [203, 30, 209],
    [13, 205, 205],
    [221, 221, 221],
    [118, 118, 118],
    [242, 32, 31],
    [35, 253, 0],
    [255, 253, 0],
    [26, 143, 255],
    [253, 40, 255],
    [20, 255, 255],
    [255, 255, 255],
];

pub const KEYS: [&str; 6] = [
    "foreground",
    "background",
    "selection_background",
    "selection_foreground",
    "cursor",
    "cursor_text_color",
];

fn xterm_256(i: u8) -> [u8; 3] {
    match i {
        0..=15 => DEFAULT_ANSI[i as usize],
        16..=231 => {
            let n = i - 16;
            let lv = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            [lv(n / 36), lv(n / 6 % 6), lv(n % 6)]
        }
        232..=255 => {
            let v = 8 + 10 * (i - 232);
            [v, v, v]
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub selection: [u8; 3],
    pub selection_fg: Option<[u8; 3]>,
    pub cursor: [u8; 3],
    pub cursor_text: Option<[u8; 3]>,
    pub ansi: [[u8; 3]; 256],
}
impl Default for Palette {
    fn default() -> Self {
        let mut ansi = [[0u8; 3]; 256];
        for (i, c) in ansi.iter_mut().enumerate() {
            *c = xterm_256(i as u8);
        }
        Self {
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            selection: DEFAULT_SELECTION,
            selection_fg: None,
            cursor: DEFAULT_FG,
            cursor_text: None,
            ansi,
        }
    }
}
impl Palette {
    pub fn rgb(&self, c: Color, default: [u8; 3]) -> [u8; 3] {
        match c {
            Color::Default => default,
            Color::Indexed(i) => self.ansi[i as usize],
            Color::Rgb(r, g, b) => [r, g, b],
        }
    }
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if !s.is_ascii() {
        return None;
    }
    match s.len() {
        6 => {
            let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
            Some([p(0)?, p(2)?, p(4)?])
        }
        3 => {
            let p = |i: usize| u8::from_str_radix(&s[i..i + 1], 16).ok().map(|v| v * 17);
            Some([p(0)?, p(1)?, p(2)?])
        }
        _ => None,
    }
}

pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("rgb:") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() != 3 {
            return None;
        }
        let mut out = [0u8; 3];
        for (o, p) in out.iter_mut().zip(parts) {
            if p.is_empty() || p.len() > 4 {
                return None;
            }
            let v = u32::from_str_radix(p, 16).ok()?;
            let max = (1u32 << (4 * p.len())) - 1;
            *o = (v * 255 / max) as u8;
        }
        return Some(out);
    }
    if s.starts_with('#') {
        return parse_hex(s);
    }
    None
}

pub fn color_index(key: &str) -> Option<u8> {
    let n = key.strip_prefix("color")?;
    if n.is_empty() || (n.len() > 1 && n.starts_with('0')) {
        return None;
    }
    n.parse::<u8>().ok()
}

pub fn is_color_key(key: &str) -> bool {
    KEYS.contains(&key) || color_index(key).is_some()
}

pub enum Applied {
    Ok,
    Ignored,
    Invalid,
}

impl Palette {
    pub fn apply(&mut self, key: &str, value: &str) -> Applied {
        let v = value.trim();
        if v.is_empty() {
            return Applied::Ok;
        }
        let unset = v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("background");
        match key {
            "selection_foreground" if unset => self.selection_fg = None,
            "cursor_text_color" if unset => self.cursor_text = None,
            "cursor" if v.eq_ignore_ascii_case("none") => self.cursor = self.fg,
            _ => {
                let Some(c) = parse_color(v) else {
                    return if is_color_key(key) { Applied::Invalid } else { Applied::Ignored };
                };
                match key {
                    "foreground" => self.fg = c,
                    "background" => self.bg = c,
                    "selection_background" => self.selection = c,
                    "selection_foreground" => self.selection_fg = Some(c),
                    "cursor" => self.cursor = c,
                    "cursor_text_color" => self.cursor_text = Some(c),
                    other => match color_index(other) {
                        Some(i) => self.ansi[i as usize] = c,
                        None => return Applied::Ignored,
                    },
                }
            }
        }
        Applied::Ok
    }
}

pub fn expand_path(raw: &str, base: Option<&Path>) -> PathBuf {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    let p = PathBuf::from(raw);
    match base {
        Some(b) if p.is_relative() => b.join(p),
        _ => p,
    }
}

pub fn read_kitty_theme(path: &Path, out: &mut Vec<(String, String)>, errors: &mut Vec<String>, depth: u8) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            errors.push(format!("{}: {e}", path.display()));
            return;
        }
    };
    let dir = path.parent().map(Path::to_path_buf);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("").trim();
        if key == "include" {
            if depth >= 8 {
                errors.push(format!("{}: includes nested too deeply", path.display()));
                continue;
            }
            read_kitty_theme(&expand_path(value, dir.as_deref()), out, errors, depth + 1);
            continue;
        }
        if is_color_key(key) {
            out.push((key.to_string(), value.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colors() {
        assert_eq!(parse_hex("#a0b1c2"), Some([0xa0, 0xb1, 0xc2]));
        assert_eq!(parse_hex("#fff"), Some([255, 255, 255]));
        assert_eq!(parse_hex("nope"), None);
        assert_eq!(parse_color("rgb:ff/80/00"), Some([255, 128, 0]));
        assert_eq!(parse_color("red"), None);
    }

    #[test]
    fn color_keys() {
        assert_eq!(color_index("color0"), Some(0));
        assert_eq!(color_index("color255"), Some(255));
        assert_eq!(color_index("color256"), None);
        assert_eq!(color_index("color07"), None);
        assert!(is_color_key("selection_background"));
        assert!(!is_color_key("url_color"));
    }

    #[test]
    fn default_palette_matches_xterm() {
        let p = Palette::default();
        assert_eq!(p.ansi[1], DEFAULT_ANSI[1]);
        assert_eq!(p.ansi[196], [255, 0, 0]);
        assert_eq!(p.ansi[232], [8, 8, 8]);
    }

    #[test]
    fn apply_kitty_values() {
        let mut p = Palette::default();
        assert!(matches!(p.apply("color4", "#123456"), Applied::Ok));
        assert_eq!(p.ansi[4], [0x12, 0x34, 0x56]);
        p.apply("selection_foreground", "#000000");
        assert_eq!(p.selection_fg, Some([0, 0, 0]));
        p.apply("selection_foreground", "none");
        assert_eq!(p.selection_fg, None);
        assert!(matches!(p.apply("color1", "blue"), Applied::Invalid));
        assert!(matches!(p.apply("url_color", "#ffffff"), Applied::Ignored));
    }

    #[test]
    fn reads_kitty_theme_with_includes() {
        let dir = std::env::temp_dir().join(format!("aeternus-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("base.conf"), "color1 #ff0000\n").unwrap();
        std::fs::write(
            dir.join("theme.conf"),
            "# a comment\n## name: Test\nforeground   #eeeeee\nbackground #111111\n\
             url_color #00ff00\nactive_tab_background #222222\ninclude base.conf\ncolor2\t#00ff00\n",
        )
        .unwrap();
        let mut out = Vec::new();
        let mut errors = Vec::new();
        read_kitty_theme(&dir.join("theme.conf"), &mut out, &mut errors, 0);
        assert!(errors.is_empty(), "{errors:?}");
        let keys: Vec<&str> = out.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["foreground", "background", "color1", "color2"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
