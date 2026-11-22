use super::{base64_decode, Term};
use crate::graphics::{decode_png, fit_rect, inflate, raw_to_rgba, Origin, Placement, QUOTA};

#[derive(Clone, Default, Debug)]
pub(super) struct Command {
    action: u8,
    quiet: u8,
    format: u32,
    medium: u8,
    width: u32,
    height: u32,
    id: u32,
    number: u32,
    placement: u32,
    compression: u8,
    more: bool,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    xoff: u32,
    yoff: u32,
    cols: u32,
    rows: u32,
    no_move: bool,
    unicode: bool,
    relative: bool,
    z: i32,
    delete: u8,
    continuation: bool,
    invalid: bool,
}

pub(super) struct Pending {
    cmd: Command,
    data: Vec<u8>,
    failed: bool,
}

type Loaded = (u32, u32, Vec<u8>);

impl Command {
    fn parse(ctrl: &[u8]) -> Self {
        let mut c = Command { action: b't', format: 32, medium: b'd', delete: b'a', ..Default::default() };
        let mut only_mq = true;
        for kv in ctrl.split(|&b| b == b',').filter(|kv| !kv.is_empty()) {
            if kv.len() < 3 || kv[1] != b'=' {
                c.invalid = true;
                continue;
            }
            let (k, v) = (kv[0], &kv[2..]);
            if k != b'm' && k != b'q' {
                only_mq = false;
            }
            let Ok(v) = std::str::from_utf8(v) else {
                c.invalid = true;
                continue;
            };
            let byte = v.as_bytes()[0];
            let ok = match k {
                b'a' | b'd' | b't' | b'o' if v.len() == 1 => {
                    match k {
                        b'a' => c.action = byte,
                        b'd' => c.delete = byte,
                        b't' => c.medium = byte,
                        _ => c.compression = byte,
                    }
                    true
                }
                b'a' | b'd' | b't' | b'o' => false,
                b'z' | b'H' | b'V' => match v.parse::<i32>() {
                    Ok(z) => {
                        if k == b'z' {
                            c.z = z;
                        }
                        true
                    }
                    Err(_) => false,
                },
                _ => match v.parse::<u32>() {
                    Ok(n) => {
                        c.set(k, n);
                        true
                    }
                    Err(_) => false,
                },
            };
            if !ok {
                c.invalid = true;
            }
        }
        c.continuation = only_mq;
        c
    }
    fn set(&mut self, k: u8, n: u32) {
        match k {
            b'q' => self.quiet = n.min(2) as u8,
            b'f' => self.format = n,
            b's' => self.width = n,
            b'v' => self.height = n,
            b'i' => self.id = n,
            b'I' => self.number = n,
            b'p' => self.placement = n,
            b'm' => self.more = n == 1,
            b'x' => self.x = n,
            b'y' => self.y = n,
            b'w' => self.w = n,
            b'h' => self.h = n,
            b'X' => self.xoff = n,
            b'Y' => self.yoff = n,
            b'c' => self.cols = n,
            b'r' => self.rows = n,
            b'C' => self.no_move = n == 1,
            b'U' => self.unicode = n == 1,
            b'P' | b'Q' => self.relative |= n != 0,
            _ => {}
        }
    }
    fn check_transmit(&self) -> Result<(), &'static str> {
        if self.invalid {
            return Err("EINVAL:malformed control data");
        }
        if self.id != 0 && self.number != 0 {
            return Err("EINVAL:i and I cannot be used together");
        }
        if self.medium != b'd' {
            return Err("EINVAL:unsupported transmission medium");
        }
        if !matches!(self.format, 24 | 32 | 100) {
            return Err("EINVAL:unsupported format");
        }
        if self.compression != 0 && self.compression != b'z' {
            return Err("EINVAL:unsupported compression");
        }
        if self.action == b'T' {
            self.check_place()?;
        }
        Ok(())
    }
    fn check_place(&self) -> Result<(), &'static str> {
        if self.unicode {
            return Err("EINVAL:Unicode placeholders are not supported");
        }
        if self.relative {
            return Err("EINVAL:relative placements are not supported");
        }
        Ok(())
    }
    fn load(&self, data: Vec<u8>) -> Result<Loaded, &'static str> {
        let data = if self.compression == b'z' {
            let limit = match self.format {
                24 => self.width as usize * self.height as usize * 3,
                32 => self.width as usize * self.height as usize * 4,
                _ => QUOTA,
            };
            inflate(&data, limit.min(QUOTA)).ok_or("EINVAL:could not decompress the data")?
        } else {
            data
        };
        match self.format {
            24 => raw_to_rgba(&data, self.width, self.height, 3).map(|v| (self.width, self.height, v)),
            32 => raw_to_rgba(&data, self.width, self.height, 4).map(|v| (self.width, self.height, v)),
            _ => decode_png(&data),
        }
    }
}

impl Term {
    pub(super) fn kitty_apc(&mut self, data: &[u8]) {
        let Some(rest) = data.strip_prefix(b"G") else { return };
        let (ctrl, payload) = match rest.iter().position(|&b| b == b';') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, &[][..]),
        };
        let cmd = Command::parse(ctrl);
        if let Some(mut p) = self.kitty_pending.take() {
            if cmd.continuation {
                if ctrl.contains(&b'q') {
                    p.cmd.quiet = cmd.quiet;
                }
                if !p.failed {
                    match base64_decode(payload) {
                        Some(d) if p.data.len() + d.len() <= self.image_quota => p.data.extend_from_slice(&d),
                        Some(_) => {
                            self.kitty_fail(&p.cmd, "EFBIG:image exceeds the size limit");
                            p.failed = true;
                        }
                        None => {
                            self.kitty_fail(&p.cmd, "EINVAL:invalid base64 payload");
                            p.failed = true;
                        }
                    }
                }
                if cmd.more {
                    self.kitty_pending = Some(p);
                } else if !p.failed {
                    self.kitty_finish(p.cmd, p.data);
                }
                return;
            }
        }
        match cmd.action {
            b't' | b'T' | b'q' => self.kitty_transmit(cmd, payload),
            b'p' => self.kitty_put(&cmd),
            b'd' => self.kitty_delete(&cmd),
            b'f' | b'a' | b'c' => self.kitty_fail(&cmd, "EINVAL:animation is not supported"),
            _ => self.kitty_fail(&cmd, "EINVAL:unsupported action"),
        }
    }
    fn kitty_transmit(&mut self, cmd: Command, payload: &[u8]) {
        let checked =
            cmd.check_transmit().and_then(|_| base64_decode(payload).ok_or("EINVAL:invalid base64 payload"));
        match checked {
            Err(e) => {
                self.kitty_fail(&cmd, e);
                if cmd.more {
                    self.kitty_pending = Some(Pending { cmd, data: Vec::new(), failed: true });
                }
            }
            Ok(data) if cmd.more => self.kitty_pending = Some(Pending { cmd, data, failed: false }),
            Ok(data) => self.kitty_finish(cmd, data),
        }
    }
    fn kitty_finish(&mut self, cmd: Command, data: Vec<u8>) {
        let (w, h, rgba) = match cmd.load(data) {
            Ok(v) => v,
            Err(e) => return self.kitty_fail(&cmd, e),
        };
        if cmd.action == b'q' {
            return self.kitty_reply(&cmd, cmd.id, None);
        }
        if !self.make_room(rgba.len()) {
            return self.kitty_fail(&cmd, "ENOSPC:image storage is full");
        }
        let id = if cmd.number != 0 { self.graphics.free_id() } else { cmd.id };
        let uid = self.graphics.add_image(id, cmd.number, w, h, rgba);
        if cmd.action == b'T' {
            self.kitty_place(&cmd, uid, id, w, h);
        }
        self.kitty_reply(&cmd, id, None);
    }
    fn kitty_put(&mut self, cmd: &Command) {
        if cmd.invalid {
            return self.kitty_fail(cmd, "EINVAL:malformed control data");
        }
        if cmd.id != 0 && cmd.number != 0 {
            return self.kitty_fail(cmd, "EINVAL:i and I cannot be used together");
        }
        if let Err(e) = cmd.check_place() {
            return self.kitty_fail(cmd, e);
        }
        let img =
            if cmd.number != 0 { self.graphics.by_number(cmd.number) } else { self.graphics.by_id(cmd.id) };
        let Some(img) = img else {
            return self.kitty_fail(cmd, "ENOENT:no image with this id or number");
        };
        let (uid, id, w, h) = (img.uid, img.id, img.width, img.height);
        self.kitty_place(cmd, uid, id, w, h);
        self.kitty_reply(cmd, id, None);
    }
    fn kitty_place(&mut self, cmd: &Command, uid: u64, id: u32, w: u32, h: u32) {
        let sx = cmd.x.min(w - 1);
        let sy = cmd.y.min(h - 1);
        let sw = if cmd.w == 0 { w - sx } else { cmd.w.min(w - sx) };
        let sh = if cmd.h == 0 { h - sy } else { cmd.h.min(h - sy) };
        let (cw, ch) = (self.cell_px.0.max(1), self.cell_px.1.max(1));
        let off = (cmd.xoff.min(cw - 1), cmd.yoff.min(ch - 1));
        let (cols, rows, rect) = fit_rect(sw, sh, off, (cmd.cols, cmd.rows), (cw, ch));
        self.graphics.place(Placement {
            image: uid,
            image_id: id,
            id: cmd.placement,
            origin: Origin::Kitty,
            line: self.screen_base() + self.cursor.row as isize,
            col: self.cursor.col,
            cols,
            rows,
            rect,
            src: [sx, sy, sw, sh],
            z: cmd.z,
        });
        if !cmd.no_move {
            for _ in 1..rows {
                self.linefeed();
            }
            let col = self.cursor.col + cols;
            if col >= self.cols() {
                self.linefeed();
                self.cursor.col = 0;
            } else {
                self.cursor.col = col;
            }
            self.wrap_pending = false;
        }
    }
    fn kitty_delete(&mut self, cmd: &Command) {
        let free = cmd.delete.is_ascii_uppercase();
        let base = self.screen_base();
        let rows = self.rows() as isize;
        let on_screen = move |p: &Placement| p.bottom() > base && p.line < base + rows;
        let cell = (base + cmd.y.max(1) as isize - 1, cmd.x.max(1) as usize - 1);
        let cursor = (base + self.cursor.row as isize, self.cursor.col);
        let g = &mut self.graphics;
        match cmd.delete.to_ascii_lowercase() {
            b'a' => g.remove_placements(on_screen, free),
            b'i' | b'n' => {
                let img = if cmd.delete.eq_ignore_ascii_case(&b'n') {
                    g.by_number(cmd.number)
                } else {
                    g.by_id(cmd.id)
                };
                let Some(uid) = img.map(|i| i.uid) else { return };
                if cmd.placement != 0 {
                    g.remove_placements(|p| p.image == uid && p.id == cmd.placement, free);
                } else if free {
                    g.remove_images(|i| i.uid == uid);
                } else {
                    g.remove_placements(|p| p.image == uid, false);
                }
            }
            b'c' => g.remove_placements(|p| p.covers(cursor.0, cursor.1), free),
            b'p' => g.remove_placements(|p| p.covers(cell.0, cell.1), free),
            b'q' => g.remove_placements(|p| p.covers(cell.0, cell.1) && p.z == cmd.z, free),
            b'x' => g.remove_placements(|p| p.covers_col(cell.1), free),
            b'y' => g.remove_placements(|p| p.covers_line(cell.0), free),
            b'z' => g.remove_placements(|p| p.z == cmd.z, free),
            b'r' => {
                let (lo, hi) = (cmd.x, cmd.y);
                if free {
                    g.remove_images(|i| i.id != 0 && i.id >= lo && i.id <= hi);
                } else {
                    g.remove_placements(|p| p.image_id != 0 && p.image_id >= lo && p.image_id <= hi, false);
                }
            }
            _ => {}
        }
    }
    fn kitty_fail(&mut self, cmd: &Command, err: &str) {
        self.kitty_reply(cmd, cmd.id, Some(err));
    }
    fn kitty_reply(&mut self, cmd: &Command, id: u32, err: Option<&str>) {
        if (id == 0 && cmd.number == 0) || cmd.quiet >= 2 || (err.is_none() && cmd.quiet == 1) {
            return;
        }
        let mut keys = Vec::new();
        if id != 0 {
            keys.push(format!("i={id}"));
        }
        if cmd.number != 0 {
            keys.push(format!("I={}", cmd.number));
        }
        if cmd.placement != 0 && matches!(cmd.action, b'T' | b'p') {
            keys.push(format!("p={}", cmd.placement));
        }
        let s = format!("\x1b_G{};{}\x1b\\", keys.join(","), err.unwrap_or("OK"));
        self.reply(&s);
    }
}

#[cfg(test)]
mod tests {
    use crate::graphics::Origin;
    use crate::parser::Parser;
    use crate::term::Term;

    fn term() -> Term {
        let mut t = Term::new(20, 10);
        t.cell_px = (10, 20);
        t
    }
    fn feed(t: &mut Term, s: &[u8]) -> String {
        Parser::new().advance(t, s);
        String::from_utf8(std::mem::take(&mut t.replies)).unwrap()
    }
    fn b64(data: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in data.chunks(3) {
            let n =
                (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for i in 0..4 {
                if i <= c.len() {
                    s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    s.push('=');
                }
            }
        }
        s
    }
    fn apc(ctrl: &str, data: &[u8]) -> Vec<u8> {
        format!("\x1b_G{ctrl};{}\x1b\\", b64(data)).into_bytes()
    }
    fn rgba(w: usize, h: usize) -> Vec<u8> {
        (0..w * h * 4).map(|i| i as u8).collect()
    }
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        let mut wr = enc.write_header().unwrap();
        wr.write_image_data(&rgba(w as usize, h as usize)).unwrap();
        drop(wr);
        out
    }
    fn zlib(data: &[u8]) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
    }

    #[test]
    fn transmit_and_display_rgba() {
        let mut t = term();
        let r = feed(&mut t, &apc("a=T,f=32,s=25,v=30,i=1", &rgba(25, 30)));
        assert_eq!(r, "\x1b_Gi=1;OK\x1b\\");
        assert_eq!(t.graphics.images.len(), 1);
        let p = &t.graphics.placements[0];
        assert_eq!((p.line, p.col, p.cols, p.rows), (0, 0, 3, 2));
        assert_eq!(p.origin, Origin::Kitty);
        assert_eq!((t.cursor.row, t.cursor.col), (1, 3));
    }

    #[test]
    fn unchunked_image_larger_than_a_chunk() {
        let mut t = term();
        let r = feed(&mut t, &apc("a=T,f=32,s=64,v=64,i=2", &rgba(64, 64)));
        assert_eq!(r, "\x1b_Gi=2;OK\x1b\\");
        assert_eq!(t.graphics.by_id(2).unwrap().rgba.len(), 64 * 64 * 4);
    }

    #[test]
    fn transmit_only_does_not_place() {
        let mut t = term();
        assert_eq!(feed(&mut t, &apc("a=t,f=24,s=2,v=2,i=4", &[0; 12])), "\x1b_Gi=4;OK\x1b\\");
        assert!(t.graphics.placements.is_empty());
        assert_eq!((t.cursor.row, t.cursor.col), (0, 0));
        let r = feed(&mut t, b"\x1b_Ga=p,i=4,p=9,C=1\x1b\\");
        assert_eq!(r, "\x1b_Gi=4,p=9;OK\x1b\\");
        assert_eq!(t.graphics.placements.len(), 1);
        assert_eq!((t.cursor.row, t.cursor.col), (0, 0));
    }

    #[test]
    fn put_unknown_image_is_enoent() {
        let mut t = term();
        assert_eq!(
            feed(&mut t, b"\x1b_Ga=p,i=77\x1b\\"),
            "\x1b_Gi=77;ENOENT:no image with this id or number\x1b\\"
        );
    }

    #[test]
    fn quiet_levels() {
        let mut t = term();
        assert_eq!(feed(&mut t, &apc("a=t,f=24,s=1,v=1,i=1,q=1", &[0; 3])), "");
        assert_eq!(
            feed(&mut t, b"\x1b_Ga=p,i=9,q=1\x1b\\"),
            "\x1b_Gi=9;ENOENT:no image with this id or number\x1b\\"
        );
        assert_eq!(feed(&mut t, b"\x1b_Ga=p,i=9,q=2\x1b\\"), "");
    }

    #[test]
    fn no_reply_without_id_or_number() {
        let mut t = term();
        assert_eq!(feed(&mut t, &apc("a=T,f=24,s=1,v=1", &[0; 3])), "");
        assert_eq!(t.graphics.placements.len(), 1);
        assert_eq!(feed(&mut t, b"\x1b_Ga=p\x1b\\"), "");
    }

    #[test]
    fn image_number_gets_an_id() {
        let mut t = term();
        let r = feed(&mut t, &apc("a=t,f=24,s=1,v=1,I=13", &[0; 3]));
        let id = t.graphics.by_number(13).unwrap().id;
        assert_ne!(id, 0);
        assert_eq!(r, format!("\x1b_Gi={id},I=13;OK\x1b\\"));
        assert_eq!(
            feed(&mut t, &apc("a=t,f=24,s=1,v=1,i=1,I=2", &[0; 3])),
            "\x1b_Gi=1,I=2;EINVAL:i and I cannot be used together\x1b\\"
        );
    }

    #[test]
    fn chunked_transfer() {
        let mut t = term();
        let data = b64(&rgba(4, 4));
        let (a, b) = data.split_at(32);
        let mut s = format!("\x1b_Ga=T,f=32,s=4,v=4,i=3,m=1;{a}\x1b\\").into_bytes();
        assert_eq!(feed(&mut t, &s), "");
        assert!(t.graphics.images.is_empty());
        s = format!("\x1b_Gm=0;{b}\x1b\\").into_bytes();
        assert_eq!(feed(&mut t, &s), "\x1b_Gi=3;OK\x1b\\");
        assert_eq!(&*t.graphics.by_id(3).unwrap().rgba, &rgba(4, 4)[..]);
        assert_eq!(t.graphics.placements.len(), 1);
    }

    #[test]
    fn chunked_compressed_png() {
        let mut t = term();
        let data = b64(&zlib(&png(3, 2)));
        let mut out = Vec::new();
        let chunks: Vec<&str> = data.as_bytes().chunks(8).map(|c| std::str::from_utf8(c).unwrap()).collect();
        for (i, c) in chunks.iter().enumerate() {
            let m = if i + 1 < chunks.len() { 1 } else { 0 };
            let ctrl = if i == 0 { format!("a=T,f=100,o=z,i=8,m={m}") } else { format!("m={m}") };
            out.extend_from_slice(format!("\x1b_G{ctrl};{c}\x1b\\").as_bytes());
        }
        assert_eq!(feed(&mut t, &out), "\x1b_Gi=8;OK\x1b\\");
        let img = t.graphics.by_id(8).unwrap();
        assert_eq!((img.width, img.height), (3, 2));
        assert_eq!(&*img.rgba, &rgba(3, 2)[..]);
    }

    #[test]
    fn zlib_raw_and_bad_data() {
        let mut t = term();
        assert_eq!(
            feed(&mut t, &apc("a=t,f=24,s=2,v=1,o=z,i=1", &zlib(&[1, 2, 3, 4, 5, 6]))),
            "\x1b_Gi=1;OK\x1b\\"
        );
        assert_eq!(&*t.graphics.by_id(1).unwrap().rgba, &[1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(
            feed(&mut t, &apc("a=t,f=24,s=2,v=1,o=z,i=2", b"garbage")),
            "\x1b_Gi=2;EINVAL:could not decompress the data\x1b\\"
        );
        assert_eq!(
            feed(&mut t, &apc("a=t,f=32,s=2,v=2,i=3", &[0; 4])),
            "\x1b_Gi=3;ENODATA:not enough pixel data for the given size\x1b\\"
        );
        assert_eq!(
            feed(&mut t, &apc("a=t,f=100,i=4", b"nope")),
            "\x1b_Gi=4;EBADPNG:could not decode the PNG data\x1b\\"
        );
    }

    #[test]
    fn query_does_not_store() {
        let mut t = term();
        assert_eq!(feed(&mut t, &apc("a=q,i=31,s=1,v=1,t=d,f=24", &[0; 3])), "\x1b_Gi=31;OK\x1b\\");
        assert!(t.graphics.is_empty());
    }

    #[test]
    fn unsupported_features_are_rejected() {
        let mut t = term();
        for (ctrl, err) in [
            ("a=t,t=f,i=1", "EINVAL:unsupported transmission medium"),
            ("a=t,t=s,i=1", "EINVAL:unsupported transmission medium"),
            ("a=t,f=8,i=1", "EINVAL:unsupported format"),
            ("a=T,U=1,f=24,s=1,v=1,i=1", "EINVAL:Unicode placeholders are not supported"),
            ("a=p,P=1,i=1", "EINVAL:relative placements are not supported"),
            ("a=f,i=1", "EINVAL:animation is not supported"),
            ("a=a,i=1", "EINVAL:animation is not supported"),
            ("a=c,i=1", "EINVAL:animation is not supported"),
        ] {
            let r = feed(&mut t, &apc(ctrl, &[0; 3]));
            assert_eq!(r, format!("\x1b_Gi=1;{err}\x1b\\"), "{ctrl}");
        }
        assert!(t.graphics.is_empty());
    }

    #[test]
    fn failed_chunked_upload_swallows_remaining_chunks() {
        let mut t = term();
        let r = feed(&mut t, b"\x1b_Ga=T,t=f,i=5,m=1;AAAA\x1b\\\x1b_Gm=1;AAAA\x1b\\\x1b_Gm=0;AAAA\x1b\\");
        assert_eq!(r, "\x1b_Gi=5;EINVAL:unsupported transmission medium\x1b\\");
        assert!(t.graphics.is_empty());
        assert_eq!(feed(&mut t, &apc("a=t,f=24,s=1,v=1,i=6", &[0; 3])), "\x1b_Gi=6;OK\x1b\\");
    }

    #[test]
    fn source_rect_and_cell_size() {
        let mut t = term();
        feed(&mut t, &apc("a=t,f=24,s=40,v=40,i=1", &[0; 40 * 40 * 3]));
        feed(&mut t, b"\x1b_Ga=p,i=1,x=10,y=5,w=20,h=100,c=4,C=1\x1b\\");
        let p = &t.graphics.placements[0];
        assert_eq!(p.src, [10, 5, 20, 35]);
        assert_eq!((p.cols, p.rows), (4, 4));
    }

    #[test]
    fn cursor_wraps_and_scrolls_after_image() {
        let mut t = term();
        feed(&mut t, b"\x1b[10;15H");
        feed(&mut t, &apc("a=T,f=24,s=1,v=1,c=6,r=3,i=1", &[0; 3]));
        assert_eq!((t.cursor.row, t.cursor.col), (9, 0));
        let p = &t.graphics.placements[0];
        assert_eq!((p.line, p.col, p.rows), (9, 14, 3));
        assert_eq!(t.abs_line(0), 3);
    }

    #[test]
    fn delete_modes() {
        let mut t = term();
        feed(&mut t, &apc("a=t,f=24,s=10,v=20,i=1", &[0; 600]));
        feed(&mut t, &apc("a=t,f=24,s=10,v=20,i=2", &[0; 600]));
        let put = |t: &mut Term, row: usize, col: usize, id: u32, p: u32, z: i32| {
            feed(
                t,
                format!("\x1b[{};{}H\x1b_Ga=p,i={id},p={p},z={z},C=1\x1b\\", row + 1, col + 1).as_bytes(),
            );
        };
        put(&mut t, 0, 0, 1, 1, 0);
        put(&mut t, 1, 1, 1, 2, -5);
        put(&mut t, 2, 2, 2, 1, 0);
        put(&mut t, 3, 3, 2, 2, 7);
        feed(&mut t, b"\x1b_Ga=d,d=z,z=-5\x1b\\");
        assert_eq!(t.graphics.placements.len(), 3);
        feed(&mut t, b"\x1b_Ga=d,d=p,x=3,y=3\x1b\\");
        assert_eq!(t.graphics.placements.len(), 2);
        feed(&mut t, b"\x1b_Ga=d,d=x,x=4\x1b\\");
        assert_eq!(t.graphics.placements.len(), 1);
        feed(&mut t, b"\x1b[1;1H\x1b_Ga=d,d=c\x1b\\");
        assert!(t.graphics.placements.is_empty());
        assert_eq!(t.graphics.images.len(), 2);
        put(&mut t, 4, 0, 1, 1, 0);
        put(&mut t, 5, 0, 2, 1, 0);
        feed(&mut t, b"\x1b_Ga=d,d=Y,y=6\x1b\\");
        assert!(t.graphics.by_id(2).is_none());
        assert!(t.graphics.by_id(1).is_some());
        feed(&mut t, b"\x1b_Ga=d,d=I,i=1\x1b\\");
        assert!(t.graphics.is_empty());
    }

    #[test]
    fn delete_by_id_and_placement_and_all() {
        let mut t = term();
        feed(&mut t, &apc("a=t,f=24,s=1,v=1,i=1", &[0; 3]));
        feed(&mut t, b"\x1b_Ga=p,i=1,p=1,C=1\x1b\\\x1b_Ga=p,i=1,p=2,C=1\x1b\\");
        feed(&mut t, b"\x1b_Ga=d,d=I,i=1,p=1\x1b\\");
        assert_eq!(t.graphics.placements.len(), 1);
        assert!(t.graphics.by_id(1).is_some());
        feed(&mut t, b"\x1b_Ga=d,d=i,i=1\x1b\\");
        assert!(t.graphics.placements.is_empty());
        assert!(t.graphics.by_id(1).is_some());
        feed(&mut t, b"\x1b_Ga=p,i=1,C=1\x1b\\\x1b_Ga=d\x1b\\");
        assert!(t.graphics.placements.is_empty());
        assert!(t.graphics.by_id(1).is_some());
        feed(&mut t, b"\x1b_Ga=p,i=1,C=1\x1b\\\x1b_Ga=d,d=A\x1b\\");
        assert!(t.graphics.is_empty());
        feed(&mut t, &apc("a=t,f=24,s=1,v=1,i=5", &[0; 3]));
        feed(&mut t, &apc("a=t,f=24,s=1,v=1,i=9", &[0; 3]));
        feed(&mut t, b"\x1b_Ga=d,d=R,x=4,y=6\x1b\\");
        assert!(t.graphics.by_id(5).is_none() && t.graphics.by_id(9).is_some());
        feed(&mut t, &apc("a=t,f=24,s=1,v=1,I=4", &[0; 3]));
        feed(&mut t, b"\x1b_Ga=d,d=N,I=4\x1b\\");
        assert!(t.graphics.by_number(4).is_none());
    }

    #[test]
    fn delete_aborts_chunked_upload() {
        let mut t = term();
        feed(&mut t, b"\x1b_Ga=T,f=24,s=2,v=1,i=1,m=1;AAAA\x1b\\\x1b_Ga=d\x1b\\\x1b_Gm=0;AAAA\x1b\\");
        assert!(t.graphics.by_id(1).is_none());
    }

    #[test]
    fn images_follow_scrolling_and_leave_with_history() {
        let mut t = term();
        t.max_scrollback = 5;
        feed(&mut t, &apc("a=T,f=24,s=10,v=20,i=1,C=1", &[0; 600]));
        let line = t.graphics.placements[0].line;
        feed(&mut t, &b"\n".repeat(12));
        assert_eq!(t.graphics.placements[0].line, line);
        assert_eq!(t.abs_line(0), 3);
        feed(&mut t, b"\x1b[10;1H\n\n\n");
        assert!(t.graphics.placements.is_empty());
        assert!(t.graphics.by_id(1).is_some());
    }

    #[test]
    fn visible_images_follow_the_viewport() {
        let mut t = term();
        feed(&mut t, &apc("a=T,f=24,s=10,v=40,i=1,C=1", &[0; 1200]));
        feed(&mut t, &b"\n".repeat(30));
        assert_eq!(t.visible_images().count(), 0);
        t.scroll_display(20);
        let rows: Vec<_> = t.visible_images().map(|(r, p, _)| (r, p.id)).collect();
        assert_eq!(rows, vec![(-1, 0)]);
        t.scroll_display(1);
        let rows: Vec<_> = t.visible_images().map(|(r, ..)| r).collect();
        assert_eq!(rows, vec![0]);
    }

    #[test]
    fn images_scroll_with_regions_and_line_ops() {
        let mut t = term();
        feed(&mut t, &apc("a=t,f=24,s=10,v=20,i=1", &[0; 600]));
        feed(&mut t, b"\x1b[4;1H\x1b_Ga=p,i=1,p=1,C=1\x1b\\\x1b[8;1H\x1b_Ga=p,i=1,p=2,C=1\x1b\\");
        feed(&mut t, b"\x1b[3;6r\x1b[2S");
        let lines: Vec<_> = t.graphics.placements.iter().map(|p| (p.id, p.line)).collect();
        assert_eq!(lines, vec![(2, 7)]);
        feed(&mut t, b"\x1b[r\x1b[5;1H\x1b_Ga=p,i=1,p=1,C=1\x1b\\\x1b[1;1H\x1b[2L");
        let lines: Vec<_> = t.graphics.placements.iter().map(|p| (p.id, p.line)).collect();
        assert_eq!(lines, vec![(2, 9), (1, 6)]);
        feed(&mut t, b"\x1b[1;1H\x1b[5M");
        let lines: Vec<_> = t.graphics.placements.iter().map(|p| (p.id, p.line)).collect();
        assert_eq!(lines, vec![(2, 4), (1, 1)]);
    }

    #[test]
    fn alt_screen_has_its_own_images() {
        let mut t = term();
        feed(&mut t, &apc("a=T,f=24,s=1,v=1,i=1,C=1", &[0; 3]));
        feed(&mut t, b"\x1b[?1049h");
        assert!(t.graphics.is_empty());
        feed(&mut t, &apc("a=T,f=24,s=1,v=1,i=1,C=1", &[9; 3]));
        assert_eq!(t.graphics.by_id(1).unwrap().rgba[0], 9);
        feed(&mut t, &b"\n".repeat(20));
        assert!(t.graphics.placements.is_empty());
        feed(&mut t, b"\x1b[?1049l");
        assert_eq!(t.graphics.by_id(1).unwrap().rgba[0], 0);
        assert_eq!(t.graphics.placements.len(), 1);
    }

    #[test]
    fn alt_screen_scroll_moves_images_up() {
        let mut t = term();
        feed(&mut t, b"\x1b[?1049h\x1b[5;1H");
        feed(&mut t, &apc("a=T,f=24,s=10,v=40,i=1,C=1", &[0; 1200]));
        let base = t.abs_line(0) as isize;
        feed(&mut t, b"\x1b[10;1H\n\n\n\n\n");
        assert_eq!(t.graphics.placements[0].line, base - 1);
        feed(&mut t, b"\n");
        assert!(t.graphics.placements.is_empty());
    }

    #[test]
    fn erase_display_and_reset() {
        let mut t = term();
        feed(&mut t, &apc("a=T,f=24,s=1,v=1,i=1", &[0; 3]));
        feed(&mut t, &b"\n".repeat(15));
        feed(&mut t, &apc("a=p,i=1,C=1", &[]));
        assert_eq!(t.graphics.placements.len(), 2);
        feed(&mut t, b"\x1b[2J");
        assert_eq!(t.graphics.placements.len(), 1);
        feed(&mut t, b"\x1b[3J");
        assert!(t.graphics.is_empty());
        feed(&mut t, &apc("a=T,f=24,s=1,v=1,i=1", &[0; 3]));
        feed(&mut t, b"\x1bc");
        assert!(t.graphics.is_empty());
    }

    #[test]
    fn reflow_moves_anchors() {
        let mut t = term();
        feed(&mut t, &"x".repeat(30).into_bytes());
        feed(&mut t, &apc("a=T,f=24,s=1,v=1,i=1,C=1", &[0; 3]));
        let p = &t.graphics.placements[0];
        assert_eq!((p.line, p.col), (1, 10));
        t.resize(40, 10);
        let p = &t.graphics.placements[0];
        assert_eq!((p.line, p.col), (0, 30));
        t.resize(10, 10);
        let p = &t.graphics.placements[0];
        assert_eq!((p.line, p.col), (2, 9));
    }

    #[test]
    fn shrinking_rows_drops_images_below_screen() {
        let mut t = term();
        feed(&mut t, b"\x1b[9;1H");
        feed(&mut t, &apc("a=T,f=24,s=1,v=1,i=1,C=1", &[0; 3]));
        feed(&mut t, b"\x1b[1;1H");
        t.resize(20, 5);
        assert!(t.graphics.placements.is_empty());
    }

    #[test]
    fn quota_evicts_unplaced_then_oldest() {
        let mut t = term();
        t.image_quota = 4 * 16 * 16 * 4;
        let img = |i: u32| apc(&format!("a=t,f=24,s=16,v=16,i={i}"), &[0; 16 * 16 * 3]);
        for i in 1..=4 {
            assert_eq!(feed(&mut t, &img(i)), format!("\x1b_Gi={i};OK\x1b\\"));
        }
        feed(&mut t, b"\x1b_Ga=p,i=1,C=1\x1b\\");
        feed(&mut t, &img(5));
        assert!(t.graphics.by_id(1).is_some());
        assert!(t.graphics.by_id(2).is_none());
        feed(&mut t, &img(6));
        assert!(t.graphics.by_id(3).is_none());
        assert_eq!(t.graphics.bytes(), t.image_quota);
        let big = apc("a=t,f=24,s=32,v=33,i=9", &[0; 32 * 33 * 3]);
        assert!(t.graphics.by_id(9).is_none());
        assert_eq!(feed(&mut t, &big), "\x1b_Gi=9;ENOSPC:image storage is full\x1b\\");
    }

    #[test]
    fn quota_is_shared_with_the_other_screen() {
        let mut t = term();
        t.image_quota = 2 * 16 * 16 * 4;
        let img = |i: u32| apc(&format!("a=T,f=24,s=16,v=16,i={i},C=1"), &[0; 16 * 16 * 3]);
        feed(&mut t, &img(1));
        feed(&mut t, &img(2));
        feed(&mut t, b"\x1b[?1049h");
        feed(&mut t, &img(3));
        assert_eq!(t.graphics.bytes() + t.saved_graphics.as_ref().unwrap().bytes(), t.image_quota);
        feed(&mut t, b"\x1b[?1049l");
        assert!(t.graphics.by_id(1).is_none());
        assert!(t.graphics.by_id(2).is_some());
    }

    #[test]
    fn chunked_upload_respects_the_quota() {
        let mut t = term();
        t.image_quota = 16;
        let r = feed(&mut t, b"\x1b_Ga=t,f=32,s=4,v=4,i=1,m=1;AAAAAAAAAAAAAAAA\x1b\\\x1b_Gm=1;AAAAAAAA\x1b\\\x1b_Gm=0;AAAA\x1b\\");
        assert_eq!(r, "\x1b_Gi=1;EFBIG:image exceeds the size limit\x1b\\");
        assert!(t.graphics.is_empty());
    }
}
