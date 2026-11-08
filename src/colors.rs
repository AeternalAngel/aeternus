use crate::cell::Color;

pub const DEFAULT_FG: [u8; 3] = [239, 220, 249];
pub const DEFAULT_BG: [u8; 3] = [18, 10, 24];
pub const DEFAULT_SELECTION: [u8; 3] = [88, 58, 126];

const DEFAULT_ANSI: [[u8; 3]; 16] = [
    [69, 71, 90],
    [243, 139, 168],
    [166, 227, 161],
    [249, 226, 175],
    [137, 180, 250],
    [245, 194, 231],
    [148, 226, 213],
    [186, 194, 222],
    [88, 91, 112],
    [243, 139, 168],
    [166, 227, 161],
    [249, 226, 175],
    [137, 180, 250],
    [245, 194, 231],
    [148, 226, 213],
    [205, 214, 244],
];

/// Cores resolvidas do tema: padrões, seleção, cursor e a paleta ANSI de 16.
#[derive(Clone, Debug)]
pub struct Palette {
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub selection: [u8; 3],
    pub cursor: [u8; 3],
    pub ansi: [[u8; 3]; 16],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            selection: DEFAULT_SELECTION,
            cursor: DEFAULT_FG,
            ansi: DEFAULT_ANSI,
        }
    }
}

impl Palette {
    pub fn rgb(&self, c: Color, default: [u8; 3]) -> [u8; 3] {
        match c {
            Color::Default => default,
            Color::Indexed(i) => self.indexed(i),
            Color::Rgb(r, g, b) => [r, g, b],
        }
    }

    fn indexed(&self, i: u8) -> [u8; 3] {
        match i {
            0..=15 => self.ansi[i as usize],
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
}

/// Aceita "#rrggbb" ou "rrggbb".
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}
