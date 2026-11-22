use crate::graphics::MAX_SIZE;

pub const REGISTERS: usize = 1024;

const VT340: [[u8; 3]; 16] = [
    [0, 0, 0],
    [20, 20, 80],
    [80, 13, 13],
    [20, 80, 20],
    [80, 20, 80],
    [20, 80, 80],
    [80, 80, 20],
    [53, 53, 53],
    [26, 26, 26],
    [33, 33, 60],
    [60, 26, 26],
    [33, 60, 33],
    [60, 33, 60],
    [33, 60, 60],
    [60, 60, 33],
    [80, 80, 80],
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Data,
    Repeat,
    Color,
    Raster,
}

pub struct Decoder {
    palette: Vec<[u8; 4]>,
    color: usize,
    mode: Mode,
    params: [u32; 5],
    nparams: usize,
    x: usize,
    band: usize,
    cap_w: usize,
    cap_h: usize,
    data: Vec<u8>,
    used_w: usize,
    used_h: usize,
    raster: (usize, usize),
    aspect: usize,
    transparent: bool,
    max_bytes: usize,
    failed: bool,
}

fn pct(v: u32) -> u8 {
    ((v.min(100) * 255 + 50) / 100) as u8
}

fn hls_to_rgb(h: u32, l: u32, s: u32) -> [u8; 4] {
    let h = ((h % 360) as f32 + 240.0) % 360.0 / 60.0;
    let l = l.min(100) as f32 / 100.0;
    let s = s.min(100) as f32 / 100.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [to(r), to(g), to(b), 255]
}

fn aspect_for(p1: u16) -> usize {
    match p1 {
        2 => 5,
        3 | 4 => 3,
        7..=9 => 1,
        _ => 2,
    }
}

impl Decoder {
    pub fn new(params: &[u16], max_bytes: usize) -> Self {
        let mut palette = vec![[0, 0, 0, 255]; REGISTERS];
        for (slot, c) in palette.iter_mut().zip(VT340) {
            *slot = [pct(c[0] as u32), pct(c[1] as u32), pct(c[2] as u32), 255];
        }
        Self {
            palette,
            color: 0,
            mode: Mode::Data,
            params: [0; 5],
            nparams: 0,
            x: 0,
            band: 0,
            cap_w: 0,
            cap_h: 0,
            data: Vec::new(),
            used_w: 0,
            used_h: 0,
            raster: (0, 0),
            aspect: aspect_for(params.first().copied().unwrap_or(0)),
            transparent: params.get(1) == Some(&1),
            max_bytes,
            failed: false,
        }
    }
    pub fn put(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.failed {
                return;
            }
            self.byte(b);
        }
    }
    fn byte(&mut self, b: u8) {
        match b {
            b'0'..=b'9' if self.mode != Mode::Data => {
                let i = self.nparams.saturating_sub(1).min(4);
                if self.nparams == 0 {
                    self.nparams = 1;
                }
                self.params[i] = (self.params[i].saturating_mul(10)).saturating_add((b - b'0') as u32);
            }
            b';' if self.mode != Mode::Data => {
                if self.nparams == 0 {
                    self.nparams = 1;
                }
                self.nparams = (self.nparams + 1).min(5);
            }
            _ => {
                let n = if self.mode == Mode::Repeat { self.take_repeat() } else { 1 };
                self.end_command();
                match b {
                    b'?'..=b'~' => self.sixel(b - b'?', n),
                    b'!' => self.start(Mode::Repeat),
                    b'#' => self.start(Mode::Color),
                    b'"' => self.start(Mode::Raster),
                    b'$' => self.x = 0,
                    b'-' => {
                        self.x = 0;
                        self.band += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    fn start(&mut self, mode: Mode) {
        self.mode = mode;
        self.params = [0; 5];
        self.nparams = 0;
    }
    fn take_repeat(&mut self) -> usize {
        self.params[0].max(1) as usize
    }
    fn end_command(&mut self) {
        match self.mode {
            Mode::Color => {
                let p = self.params;
                let reg = p[0] as usize % REGISTERS;
                if self.nparams >= 5 {
                    match p[1] {
                        1 => self.palette[reg] = hls_to_rgb(p[2], p[3], p[4]),
                        2 => self.palette[reg] = [pct(p[2]), pct(p[3]), pct(p[4]), 255],
                        _ => {}
                    }
                }
                self.color = reg;
            }
            Mode::Raster => {
                let p = self.params;
                if self.nparams >= 2 && p[0] > 0 && p[1] > 0 {
                    self.aspect = ((p[0] + p[1] / 2) / p[1]).max(1) as usize;
                }
                if self.nparams >= 4 {
                    self.raster = (p[2] as usize, p[3] as usize);
                }
            }
            Mode::Repeat | Mode::Data => {}
        }
        self.mode = Mode::Data;
    }
    fn reserve(&mut self, w: usize, h: usize) -> bool {
        if w <= self.cap_w && h <= self.cap_h {
            return true;
        }
        let max = MAX_SIZE as usize;
        let rows = h * self.aspect;
        if w > max || rows > max || w * rows * 4 > self.max_bytes {
            self.failed = true;
            return false;
        }
        let nw = w.max(self.cap_w).max((self.cap_w * 2).min(max)).max(self.raster.0.min(max));
        let nh = h.max(self.cap_h).max((self.cap_h * 2).min(max / self.aspect.max(1)));
        let nw = if nw * nh * self.aspect * 4 > self.max_bytes { w.max(self.cap_w) } else { nw };
        let nh = if nw * nh * self.aspect * 4 > self.max_bytes { h.max(self.cap_h) } else { nh };
        let mut data = vec![0u8; nw * nh * 4];
        for y in 0..self.cap_h {
            let src = &self.data[y * self.cap_w * 4..(y + 1) * self.cap_w * 4];
            data[y * nw * 4..y * nw * 4 + src.len()].copy_from_slice(src);
        }
        self.data = data;
        self.cap_w = nw;
        self.cap_h = nh;
        true
    }
    fn sixel(&mut self, bits: u8, n: usize) {
        let x0 = self.x;
        self.x = x0.saturating_add(n);
        if bits == 0 {
            return;
        }
        let y0 = self.band * 6;
        if !self.reserve(self.x, y0 + 6) {
            return;
        }
        let color = self.palette[self.color];
        let stride = self.cap_w * 4;
        for i in 0..6 {
            if bits & (1 << i) == 0 {
                continue;
            }
            let row = (y0 + i) * stride;
            for px in self.data[row + x0 * 4..row + self.x * 4].as_chunks_mut::<4>().0 {
                *px = color;
            }
            self.used_h = self.used_h.max(y0 + i + 1);
        }
        self.used_w = self.used_w.max(self.x);
    }
    pub fn cursor_y(&self) -> usize {
        self.band * 6 * self.aspect
    }
    pub fn finish(mut self) -> Option<(u32, u32, Vec<u8>)> {
        self.end_command();
        if self.failed {
            return None;
        }
        let max = MAX_SIZE as usize;
        let w = self.used_w.max(self.raster.0.min(max));
        let h = self.used_h.max(self.raster.1.min(max / self.aspect.max(1)));
        if w == 0 || h == 0 || !self.reserve(w, h) {
            return None;
        }
        let rows = h * self.aspect;
        let bg = if self.transparent { [0, 0, 0, 0] } else { self.palette[0] };
        let mut out = Vec::with_capacity(w * rows * 4);
        for y in 0..h {
            let line = &self.data[y * self.cap_w * 4..y * self.cap_w * 4 + w * 4];
            let start = out.len();
            for px in line.as_chunks::<4>().0 {
                out.extend_from_slice(if px[3] == 0 { &bg } else { px });
            }
            for _ in 1..self.aspect {
                out.extend_from_within(start..start + w * 4);
            }
        }
        Some((w as u32, rows as u32, out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(params: &[u16], data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
        let mut d = Decoder::new(params, 1 << 30);
        d.put(data);
        d.finish()
    }
    fn px(img: &(u32, u32, Vec<u8>), x: usize, y: usize) -> [u8; 4] {
        let i = (y * img.0 as usize + x) * 4;
        img.2[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn single_column_with_rgb_register() {
        let img = decode(&[0, 1], b"\"1;1#1;2;100;0;0#1~").unwrap();
        assert_eq!((img.0, img.1), (1, 6));
        for y in 0..6 {
            assert_eq!(px(&img, 0, y), [255, 0, 0, 255]);
        }
    }

    #[test]
    fn partial_bits_and_transparency() {
        let img = decode(&[0, 1], b"\"1;1#2;2;0;100;0#2A").unwrap();
        assert_eq!((img.0, img.1), (1, 2));
        assert_eq!(px(&img, 0, 0), [0, 0, 0, 0]);
        assert_eq!(px(&img, 0, 1), [0, 255, 0, 255]);
    }

    #[test]
    fn opaque_background_uses_register_zero() {
        let img = decode(&[0, 0], b"\"1;1#0;2;0;0;100#1;2;100;100;100#1A").unwrap();
        assert_eq!(px(&img, 0, 0), [0, 0, 255, 255]);
        assert_eq!(px(&img, 0, 1), [255, 255, 255, 255]);
    }

    #[test]
    fn repeat_carriage_return_and_new_line() {
        let img = decode(&[0, 1], b"\"1;1#1;2;100;0;0!3~$#2;2;0;0;100@-#1!2~").unwrap();
        assert_eq!((img.0, img.1), (3, 12));
        assert_eq!(px(&img, 0, 0), [0, 0, 255, 255]);
        assert_eq!(px(&img, 1, 0), [255, 0, 0, 255]);
        assert_eq!(px(&img, 2, 5), [255, 0, 0, 255]);
        assert_eq!(px(&img, 1, 11), [255, 0, 0, 255]);
        assert_eq!(px(&img, 2, 11), [0, 0, 0, 0]);
    }

    #[test]
    fn hls_hue_zero_is_blue() {
        let img = decode(&[0, 1], b"\"1;1#1;1;0;50;100#1~#2;1;120;50;100#2~#3;1;240;50;100#3~").unwrap();
        assert_eq!(px(&img, 0, 0), [0, 0, 255, 255]);
        assert_eq!(px(&img, 1, 0), [255, 0, 0, 255]);
        assert_eq!(px(&img, 2, 0), [0, 255, 0, 255]);
    }

    #[test]
    fn default_palette_and_aspect_ratio() {
        let img = decode(&[0, 1], b"#2~").unwrap();
        assert_eq!((img.0, img.1), (1, 12));
        assert_eq!(px(&img, 0, 11), [204, 33, 33, 255]);
        let img = decode(&[9, 1], b"#2~").unwrap();
        assert_eq!(img.1, 6);
        let img = decode(&[0, 1], b"\"3;1#2~").unwrap();
        assert_eq!(img.1, 18);
    }

    #[test]
    fn raster_size_extends_the_image() {
        let img = decode(&[0, 1], b"\"1;1;4;3#1@").unwrap();
        assert_eq!((img.0, img.1), (4, 3));
    }

    #[test]
    fn oversized_images_are_dropped() {
        assert!(decode(&[0, 1], b"\"1;1!10001~").is_none());
        let mut d = Decoder::new(&[0, 1], 1024);
        d.put(b"\"1;1!100~-!100~");
        assert!(d.finish().is_none());
        let mut long = b"\"1;1".to_vec();
        long.extend(std::iter::repeat_n(b'-', 2000));
        long.push(b'~');
        assert!(decode(&[0, 1], &long).is_none());
    }

    #[test]
    fn empty_image_is_dropped() {
        assert!(decode(&[0, 1], b"#1;2;1;2;3").is_none());
    }
}
