use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub const QUOTA: usize = 256 << 20;
pub const MAX_SIZE: u32 = 10_000;
pub const BELOW_BG: i32 = i32::MIN / 2;

static NEXT_UID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    Kitty,
    Sixel,
}

#[derive(Debug)]
pub struct Image {
    pub uid: u64,
    pub id: u32,
    pub number: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
    used: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub image: u64,
    pub image_id: u32,
    pub id: u32,
    pub origin: Origin,
    pub line: isize,
    pub col: usize,
    pub cols: usize,
    pub rows: usize,
    pub rect: [f32; 4],
    pub src: [u32; 4],
    pub z: i32,
}

impl Placement {
    pub fn bottom(&self) -> isize {
        self.line + self.rows as isize
    }
    pub fn covers_line(&self, line: isize) -> bool {
        line >= self.line && line < self.bottom()
    }
    pub fn covers_col(&self, col: usize) -> bool {
        col >= self.col && col < self.col + self.cols
    }
    pub fn covers(&self, line: isize, col: usize) -> bool {
        self.covers_line(line) && self.covers_col(col)
    }
}

#[derive(Default)]
pub struct Graphics {
    pub images: Vec<Image>,
    pub placements: Vec<Placement>,
    next_id: u32,
    tick: u64,
}

impl Graphics {
    pub fn is_empty(&self) -> bool {
        self.images.is_empty() && self.placements.is_empty()
    }
    pub fn bytes(&self) -> usize {
        self.images.iter().map(|i| i.rgba.len()).sum()
    }
    fn touch(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }
    pub fn image(&self, uid: u64) -> Option<&Image> {
        self.images.iter().find(|i| i.uid == uid)
    }
    pub fn by_id(&self, id: u32) -> Option<&Image> {
        if id == 0 {
            return None;
        }
        self.images.iter().find(|i| i.id == id)
    }
    pub fn by_number(&self, number: u32) -> Option<&Image> {
        if number == 0 {
            return None;
        }
        self.images.iter().filter(|i| i.number == number).max_by_key(|i| i.uid)
    }
    pub fn free_id(&mut self) -> u32 {
        loop {
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if self.by_id(self.next_id).is_none() {
                return self.next_id;
            }
        }
    }
    pub fn add_image(&mut self, id: u32, number: u32, width: u32, height: u32, rgba: Vec<u8>) -> u64 {
        if id != 0 {
            self.remove_images(|i| i.id == id);
        }
        let uid = NEXT_UID.fetch_add(1, Ordering::Relaxed);
        let used = self.touch();
        self.images.push(Image { uid, id, number, width, height, rgba: rgba.into(), used });
        uid
    }
    pub fn place(&mut self, p: Placement) {
        if p.id != 0 && p.image_id != 0 {
            self.placements.retain(|q| !(q.image_id == p.image_id && q.id == p.id));
        }
        let used = self.touch();
        if let Some(img) = self.images.iter_mut().find(|i| i.uid == p.image) {
            img.used = used;
        }
        self.placements.push(p);
    }
    pub fn remove_images(&mut self, f: impl Fn(&Image) -> bool) {
        let gone: Vec<u64> = self.images.iter().filter(|i| f(i)).map(|i| i.uid).collect();
        if gone.is_empty() {
            return;
        }
        self.images.retain(|i| !gone.contains(&i.uid));
        self.placements.retain(|p| !gone.contains(&p.image));
    }
    pub fn remove_placements(&mut self, f: impl Fn(&Placement) -> bool, free: bool) {
        let mut touched = Vec::new();
        self.placements.retain(|p| {
            let hit = f(p);
            if hit && !touched.contains(&p.image) {
                touched.push(p.image);
            }
            !hit
        });
        if free {
            let placed: Vec<u64> = self.placements.iter().map(|p| p.image).collect();
            self.images.retain(|i| !touched.contains(&i.uid) || placed.contains(&i.uid));
        }
        self.gc();
    }
    pub fn gc(&mut self) {
        if self.images.iter().all(|i| i.id != 0 || i.number != 0) {
            return;
        }
        let placed: Vec<u64> = self.placements.iter().map(|p| p.image).collect();
        self.images.retain(|i| i.id != 0 || i.number != 0 || placed.contains(&i.uid));
    }
    pub fn evict_one(&mut self, unplaced_only: bool) -> Option<usize> {
        let victim = self
            .images
            .iter()
            .filter(|i| !unplaced_only || !self.placements.iter().any(|p| p.image == i.uid))
            .min_by_key(|i| i.used)?
            .uid;
        let bytes = self.image(victim).map_or(0, |i| i.rgba.len());
        self.remove_images(|i| i.uid == victim);
        Some(bytes)
    }
    pub fn prune_above(&mut self, line: isize) {
        if self.placements.iter().any(|p| p.bottom() <= line) {
            self.placements.retain(|p| p.bottom() > line);
            self.gc();
        }
    }
    pub fn prune_below(&mut self, line: isize) {
        if self.placements.iter().any(|p| p.line >= line) {
            self.placements.retain(|p| p.line < line);
            self.gc();
        }
    }
    pub fn shift(&mut self, base: isize, top: usize, bottom: usize, up: isize) {
        if self.placements.is_empty() || up == 0 {
            return;
        }
        let (top, bottom) = (base + top as isize, base + bottom as isize);
        self.placements.retain_mut(|p| {
            let straddles = top == base && p.line < top && p.bottom() > top;
            if p.line > bottom || (p.line < top && !straddles) {
                return true;
            }
            p.line -= up;
            if up > 0 {
                p.line >= top || (top == base && p.bottom() > top)
            } else {
                p.line <= bottom
            }
        });
        self.gc();
    }
    pub fn clear(&mut self) {
        self.images.clear();
        self.placements.clear();
    }
}

pub fn fit_rect(
    src_w: u32,
    src_h: u32,
    off: (u32, u32),
    cells: (u32, u32),
    cell: (u32, u32),
) -> (usize, usize, [f32; 4]) {
    let (cw, ch) = (cell.0.max(1) as f32, cell.1.max(1) as f32);
    let (ox, oy) = (off.0 as f32 / cw, off.1 as f32 / ch);
    let (sw, sh) = (src_w.max(1) as f32, src_h.max(1) as f32);
    let (mut x, mut y) = (ox, oy);
    let (w, h, cols, rows) = match cells {
        (0, 0) => {
            let (w, h) = (sw / cw, sh / ch);
            (w, h, (ox + w).ceil(), (oy + h).ceil())
        }
        (c, 0) => {
            let w = c as f32;
            let h = w * cw * sh / sw / ch;
            (w, h, c as f32, (oy + h).ceil())
        }
        (0, r) => {
            let h = r as f32;
            let w = h * ch * sw / sh / cw;
            (w, h, (ox + w).ceil(), r as f32)
        }
        (c, r) => {
            let scale = (c as f32 * cw / sw).min(r as f32 * ch / sh);
            let (w, h) = (sw * scale / cw, sh * scale / ch);
            x += (c as f32 - w) / 2.0;
            y += (r as f32 - h) / 2.0;
            (w, h, c as f32, r as f32)
        }
    };
    (cols.max(1.0) as usize, rows.max(1.0) as usize, [x, y, w, h])
}

pub fn inflate(data: &[u8], limit: usize) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, limit).ok()
}

pub fn check_size(w: u32, h: u32) -> Result<(), &'static str> {
    if w == 0 || h == 0 {
        return Err("EINVAL:Zero width/height not allowed");
    }
    if w > MAX_SIZE || h > MAX_SIZE || w as usize * h as usize * 4 > QUOTA {
        return Err("EFBIG:Image too large");
    }
    Ok(())
}

pub fn raw_to_rgba(data: &[u8], w: u32, h: u32, bpp: usize) -> Result<Vec<u8>, &'static str> {
    check_size(w, h)?;
    let n = w as usize * h as usize;
    if data.len() < n * bpp {
        return Err("ENODATA:Insufficient image data");
    }
    let data = &data[..n * bpp];
    if bpp == 4 {
        return Ok(data.to_vec());
    }
    let mut out = Vec::with_capacity(n * 4);
    for px in data.as_chunks::<3>().0 {
        out.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    Ok(out)
}

pub fn decode_png(data: &[u8]) -> Result<(u32, u32, Vec<u8>), &'static str> {
    const BAD: &str = "EBADPNG:Failed to decode PNG";
    let mut dec = png::Decoder::new_with_limits(Cursor::new(data), png::Limits { bytes: QUOTA * 2 });
    let (w, h) = dec.read_header_info().map_err(|_| BAD)?.size();
    check_size(w, h)?;
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|_| BAD)?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or(BAD)?];
    let info = reader.next_frame(&mut buf).map_err(|_| BAD)?;
    buf.truncate(info.buffer_size());
    let n = w as usize * h as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => {
            buf.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
        }
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(BAD),
    };
    if rgba.len() != n * 4 {
        return Err(BAD);
    }
    Ok((w, h, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(image: u64, image_id: u32, id: u32, line: isize, rows: usize) -> Placement {
        Placement {
            image,
            image_id,
            id,
            origin: Origin::Kitty,
            line,
            col: 0,
            cols: 2,
            rows,
            rect: [0.0, 0.0, 2.0, rows as f32],
            src: [0, 0, 1, 1],
            z: 0,
        }
    }

    #[test]
    fn retransmit_replaces_image_and_placements() {
        let mut g = Graphics::default();
        let a = g.add_image(7, 0, 1, 1, vec![0; 4]);
        g.place(placement(a, 7, 0, 0, 1));
        let b = g.add_image(7, 0, 1, 1, vec![0; 4]);
        assert_ne!(a, b);
        assert_eq!(g.images.len(), 1);
        assert!(g.placements.is_empty());
    }

    #[test]
    fn same_placement_id_replaces() {
        let mut g = Graphics::default();
        let a = g.add_image(1, 0, 1, 1, vec![0; 4]);
        g.place(placement(a, 1, 5, 0, 1));
        g.place(placement(a, 1, 5, 3, 1));
        g.place(placement(a, 1, 0, 4, 1));
        g.place(placement(a, 1, 0, 6, 1));
        assert_eq!(g.placements.iter().map(|p| p.line).collect::<Vec<_>>(), vec![3, 4, 6]);
    }

    #[test]
    fn anonymous_images_go_with_their_last_placement() {
        let mut g = Graphics::default();
        let a = g.add_image(0, 0, 1, 1, vec![0; 4]);
        g.place(placement(a, 0, 0, 0, 1));
        let b = g.add_image(3, 0, 1, 1, vec![0; 4]);
        g.place(placement(b, 3, 0, 0, 1));
        g.remove_placements(|_| true, false);
        assert_eq!(g.images.len(), 1);
        assert_eq!(g.images[0].id, 3);
    }

    #[test]
    fn eviction_prefers_unplaced_then_oldest() {
        let mut g = Graphics::default();
        let a = g.add_image(1, 0, 1, 1, vec![0; 4]);
        g.add_image(2, 0, 1, 1, vec![0; 4]);
        g.add_image(3, 0, 1, 1, vec![0; 4]);
        g.place(placement(a, 1, 0, 0, 1));
        assert_eq!(g.evict_one(true), Some(4));
        assert!(g.by_id(2).is_none());
        assert_eq!(g.evict_one(true), Some(4));
        assert!(g.by_id(3).is_none());
        assert_eq!(g.evict_one(true), None);
        assert_eq!(g.evict_one(false), Some(4));
        assert!(g.is_empty());
    }

    #[test]
    fn shift_moves_and_drops_placements_in_region() {
        let mut g = Graphics::default();
        let a = g.add_image(1, 0, 1, 1, vec![0; 4]);
        g.place(placement(a, 1, 1, 100, 2));
        g.place(placement(a, 1, 2, 103, 2));
        g.place(placement(a, 1, 3, 108, 1));
        g.shift(100, 0, 5, 1);
        let lines: Vec<_> = g.placements.iter().map(|p| (p.id, p.line)).collect();
        assert_eq!(lines, vec![(1, 99), (2, 102), (3, 108)]);
        g.shift(100, 0, 5, 1);
        let lines: Vec<_> = g.placements.iter().map(|p| (p.id, p.line)).collect();
        assert_eq!(lines, vec![(2, 101), (3, 108)]);
        g.shift(100, 2, 5, 1);
        assert_eq!(g.placements.iter().map(|p| p.id).collect::<Vec<_>>(), vec![2, 3]);
        g.shift(100, 0, 5, -5);
        assert_eq!(g.placements.iter().map(|p| p.id).collect::<Vec<_>>(), vec![3]);
    }

    #[test]
    fn fit_rect_sizes() {
        assert_eq!(fit_rect(25, 30, (0, 0), (0, 0), (10, 20)), (3, 2, [0.0, 0.0, 2.5, 1.5]));
        assert_eq!(fit_rect(20, 20, (5, 0), (0, 0), (10, 20)), (3, 1, [0.5, 0.0, 2.0, 1.0]));
        assert_eq!(fit_rect(100, 50, (0, 0), (4, 0), (10, 20)), (4, 1, [0.0, 0.0, 4.0, 1.0]));
        assert_eq!(fit_rect(100, 100, (0, 0), (0, 2), (10, 20)), (4, 2, [0.0, 0.0, 4.0, 2.0]));
        let (c, r, rect) = fit_rect(100, 100, (0, 0), (10, 2), (10, 20));
        assert_eq!((c, r), (10, 2));
        assert_eq!(rect, [3.0, 0.0, 4.0, 2.0]);
    }

    #[test]
    fn raw_formats() {
        assert_eq!(raw_to_rgba(&[1, 2, 3, 4, 5, 6], 2, 1, 3).unwrap(), vec![1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(raw_to_rgba(&[1, 2, 3], 2, 1, 3), Err("ENODATA:Insufficient image data"));
        assert_eq!(raw_to_rgba(&[], 0, 1, 4), Err("EINVAL:Zero width/height not allowed"));
        assert_eq!(raw_to_rgba(&[], MAX_SIZE + 1, 1, 4), Err("EFBIG:Image too large"));
    }

    #[test]
    fn png_decodes_and_rejects_garbage() {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 1);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 0, 255, 0]).unwrap();
        }
        assert_eq!(decode_png(&out).unwrap(), (2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]));
        assert!(decode_png(b"not a png").is_err());
    }
}
