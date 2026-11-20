const SS: u32 = 4;

#[derive(Clone, Copy)]
enum Shape {
    Tri([(f32, f32); 3]),
    Line((f32, f32), (f32, f32)),
    Chevron(bool),
    Half(bool, bool),
}

fn shape(ch: char) -> Option<Shape> {
    use Shape::*;
    Some(match ch as u32 {
        0xE0B0 => Tri([(0.0, 0.0), (1.0, 0.5), (0.0, 1.0)]),
        0xE0B1 => Chevron(true),
        0xE0B2 => Tri([(1.0, 0.0), (0.0, 0.5), (1.0, 1.0)]),
        0xE0B3 => Chevron(false),
        0xE0B4 => Half(true, true),
        0xE0B5 => Half(true, false),
        0xE0B6 => Half(false, true),
        0xE0B7 => Half(false, false),
        0xE0B8 => Tri([(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)]),
        0xE0B9 | 0xE0BF => Line((0.0, 0.0), (1.0, 1.0)),
        0xE0BA => Tri([(1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        0xE0BB | 0xE0BD => Line((1.0, 0.0), (0.0, 1.0)),
        0xE0BC => Tri([(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]),
        0xE0BE => Tri([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]),
        _ => return None,
    })
}

pub fn is_sprite(ch: char) -> bool {
    shape(ch).is_some()
}

fn edge(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

fn in_tri(t: &[(f32, f32); 3], p: (f32, f32)) -> bool {
    let d0 = edge(t[0], t[1], p);
    let d1 = edge(t[1], t[2], p);
    let d2 = edge(t[2], t[0], p);
    let neg = d0 < 0.0 || d1 < 0.0 || d2 < 0.0;
    let pos = d0 > 0.0 || d1 > 0.0 || d2 > 0.0;
    !(neg && pos)
}

fn seg_dist(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    (qx * qx + qy * qy).sqrt()
}

fn inside(s: Shape, w: f32, h: f32, thick: f32, p: (f32, f32)) -> bool {
    let sc = |q: (f32, f32)| (q.0 * w, q.1 * h);
    match s {
        Shape::Tri(t) => in_tri(&[sc(t[0]), sc(t[1]), sc(t[2])], p),
        Shape::Line(a, b) => seg_dist(sc(a), sc(b), p) <= thick / 2.0,
        Shape::Chevron(right) => {
            let (x0, x1) = if right { (0.0, w) } else { (w, 0.0) };
            let tip = (x1, h / 2.0);
            seg_dist((x0, 0.0), tip, p) <= thick / 2.0 || seg_dist((x0, h), tip, p) <= thick / 2.0
        }
        Shape::Half(right, filled) => {
            let cx = if right { 0.0 } else { w };
            let (rx, ry) = (w, h / 2.0);
            let (dx, dy) = (p.0 - cx, p.1 - ry);
            let outer = (dx / rx).powi(2) + (dy / ry).powi(2) <= 1.0;
            if filled {
                return outer;
            }
            let (ix, iy) = ((rx - thick).max(0.5), (ry - thick).max(0.5));
            outer && (dx / ix).powi(2) + (dy / iy).powi(2) > 1.0
        }
    }
}

pub fn render(ch: char, w: u32, h: u32, thick: f32) -> Option<Vec<u8>> {
    let s = shape(ch)?;
    let (wf, hf) = (w as f32, h as f32);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut hits = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = x as f32 + (sx as f32 + 0.5) / SS as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SS as f32;
                    if inside(s, wf, hf, thick, (px, py)) {
                        hits += 1;
                    }
                }
            }
            let a = (hits * 255 / (SS * SS)) as u8;
            out.extend_from_slice(&[a, 0, 0, 0]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(m: &[u8], w: u32, x: u32, y: u32) -> u8 {
        m[((y * w + x) * 4) as usize]
    }

    #[test]
    fn right_triangle_fills_cell_height() {
        let (w, h) = (10, 22);
        let m = render('\u{e0b0}', w, h, 1.0).unwrap();
        assert!(alpha(&m, w, 0, 0) > 0);
        assert!(alpha(&m, w, 0, h - 1) > 0);
        assert_eq!(alpha(&m, w, w - 1, 0), 0);
        assert_eq!(alpha(&m, w, w / 2, h / 2), 255);
    }

    #[test]
    fn only_powerline_range() {
        assert!(is_sprite('\u{e0b4}'));
        assert!(!is_sprite('\u{e0a0}'));
        assert!(!is_sprite('a'));
    }
}
