use super::Term;
use crate::graphics::{fit_rect, Origin, Placement, MAX_SIZE};
use crate::sixel::{Decoder, REGISTERS};

impl Term {
    pub(super) fn sixel_start(&mut self, params: &[u16]) {
        self.sixel = Some(Box::new(Decoder::new(params, self.image_quota)));
    }
    pub(super) fn sixel_put(&mut self, data: &[u8]) {
        if let Some(d) = &mut self.sixel {
            d.put(data);
        }
    }
    pub(super) fn sixel_end(&mut self, complete: bool) {
        let Some(d) = self.sixel.take() else { return };
        if !complete {
            return;
        }
        let cursor_y = d.cursor_y();
        let Some((w, h, rgba)) = d.finish() else { return };
        let (cw, ch) = (self.cell_px.0.max(1), self.cell_px.1.max(1));
        let scrolling = !self.sixel_display_mode;
        let (row, col) = if scrolling { (self.cursor.row, self.cursor.col) } else { (0, 0) };
        let shown_h = if scrolling { h } else { h.min(self.rows() as u32 * ch) };
        if !self.make_room(rgba.len()) {
            return;
        }
        let uid = self.graphics.add_image(0, 0, w, h, rgba);
        let (cols, rows, rect) = fit_rect(w, shown_h, (0, 0), (0, 0), (cw, ch));
        self.graphics.place(Placement {
            image: uid,
            image_id: 0,
            id: 0,
            origin: Origin::Sixel,
            line: self.screen_base() + row as isize,
            col,
            cols,
            rows,
            rect,
            src: [0, 0, w, shown_h],
            z: 0,
        });
        if scrolling {
            let down = if cursor_y >= h as usize { rows } else { rows - 1 };
            for _ in 0..down {
                self.linefeed();
            }
            self.cursor.col = col;
            self.wrap_pending = false;
        }
    }
    #[inline(always)]
    pub(super) fn erase_sixels(&mut self, r0: usize, r1: usize, c0: usize, c1: usize) {
        if !self.graphics.placements.is_empty() {
            self.erase_sixels_slow(r0, r1, c0, c1);
        }
    }
    #[cold]
    fn erase_sixels_slow(&mut self, r0: usize, r1: usize, c0: usize, c1: usize) {
        let base = self.screen_base();
        self.graphics.erase_sixels(base + r0 as isize, base + r1 as isize, c0, c1);
    }
    pub(super) fn xtsmgraphics(&mut self, params: &[u16]) {
        let pi = params.first().copied().unwrap_or(0);
        let pa = params.get(1).copied().unwrap_or(0);
        let max = MAX_SIZE as usize;
        let (ps, pv) = match (pi, pa) {
            (1 | 2, _) if !self.images => (3, "0".to_string()),
            (1, 1..=4) => (0, REGISTERS.to_string()),
            (2, 1 | 2) => {
                let w = (self.cols() * self.cell_px.0 as usize).min(max);
                let h = (self.rows() * self.cell_px.1 as usize).min(max);
                (0, format!("{w};{h}"))
            }
            (2, 4) => (0, format!("{max};{max}")),
            (2, 3) => (3, "0".to_string()),
            (1 | 2, _) => (2, "0".to_string()),
            _ => (1, "0".to_string()),
        };
        self.reply(&format!("\x1b[?{pi};{ps};{pv}S"));
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
    fn sixel(w: usize, bands: usize) -> Vec<u8> {
        let mut s = b"\x1bP0;1q\"1;1#1;2;100;0;0".to_vec();
        for b in 0..bands {
            if b > 0 {
                s.push(b'-');
            }
            s.extend(format!("#1!{w}~").as_bytes());
        }
        s.extend(b"\x1b\\");
        s
    }

    #[test]
    fn sixel_is_placed_at_the_cursor_and_moves_it_below() {
        let mut t = term();
        feed(&mut t, b"\x1b[2;3H");
        feed(&mut t, &sixel(25, 6));
        let p = &t.graphics.placements[0];
        assert_eq!(p.origin, Origin::Sixel);
        assert_eq!((p.line, p.col, p.cols, p.rows), (1, 2, 3, 2));
        assert_eq!((t.cursor.row, t.cursor.col), (2, 2));
        let img = t.graphics.image(p.image).unwrap();
        assert_eq!((img.width, img.height), (25, 36));
        assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn sixel_scrolls_the_screen_when_it_does_not_fit() {
        let mut t = term();
        feed(&mut t, b"\x1b[9;1H");
        feed(&mut t, &sixel(10, 10));
        let p = &t.graphics.placements[0];
        assert_eq!((p.line, p.rows), (8, 3));
        assert_eq!(t.cursor.row, 9);
        assert_eq!(t.abs_line(0), 1);
    }

    #[test]
    fn trailing_new_line_puts_the_cursor_below_the_image() {
        let mut t = term();
        let mut s = sixel(10, 6);
        s.truncate(s.len() - 2);
        s.extend(b"-\x1b\\");
        feed(&mut t, &s);
        assert_eq!((t.cursor.row, t.cursor.col), (2, 0));
        let mut t = term();
        let mut s = sixel(10, 10);
        s.truncate(s.len() - 2);
        s.extend(b"-\x1b\\");
        feed(&mut t, &s);
        assert_eq!(t.cursor.row, 3);
    }

    #[test]
    fn image_bottom_stays_on_screen_when_the_cursor_is_on_its_last_band() {
        let mut t = term();
        feed(&mut t, b"\x1b[10;1H");
        feed(&mut t, &sixel(10, 4));
        let p = &t.graphics.placements[0];
        assert_eq!(p.rows, 2);
        assert_eq!(p.bottom(), t.abs_line(9) as isize + 1);
        assert_eq!(t.cursor.row, 9);
    }

    #[test]
    fn sixel_display_mode_draws_at_the_top_without_moving_the_cursor() {
        let mut t = term();
        feed(&mut t, b"\x1b[?80h\x1b[5;5H");
        feed(&mut t, &sixel(10, 50));
        let p = &t.graphics.placements[0];
        assert_eq!((p.line, p.col, p.rows), (0, 0, 10));
        assert_eq!(p.src[3], 200);
        assert_eq!((t.cursor.row, t.cursor.col), (4, 4));
        assert_eq!(feed(&mut t, b"\x1b[?80$p"), "\x1b[?80;1$y");
        feed(&mut t, b"\x1b[?80l");
        assert_eq!(feed(&mut t, b"\x1b[?80$p"), "\x1b[?80;2$y");
    }

    #[test]
    fn sixel_larger_than_a_dcs_string_is_decoded() {
        let mut t = term();
        feed(&mut t, &sixel(5000, 2));
        assert_eq!(t.graphics.images[0].width, 5000);
    }

    #[test]
    fn cancelled_or_oversized_sixel_is_dropped() {
        let mut t = term();
        feed(&mut t, b"\x1bPq#1~~\x18");
        assert!(t.graphics.is_empty());
        feed(&mut t, b"\x1bPq\"1;1!10001~\x1b\\");
        assert!(t.graphics.is_empty());
        t.image_quota = 1000;
        feed(&mut t, &sixel(100, 1));
        assert!(t.graphics.is_empty());
    }

    #[test]
    fn text_over_a_sixel_removes_that_part() {
        let mut t = term();
        feed(&mut t, &sixel(40, 10));
        assert_eq!(t.graphics.placements.len(), 1);
        feed(&mut t, b"\x1b[2;2Hx");
        assert_eq!(t.graphics.placements.len(), 4);
        feed(&mut t, b"\x1b[1;1H\x1b[K");
        assert!(t.graphics.placements.iter().all(|p| p.line > 0));
        feed(&mut t, b"\x1b[3;1H\x1b[J");
        assert!(t.graphics.placements.iter().all(|p| p.line < 2));
        feed(&mut t, b"\x1b[2;1H\x1b[10X");
        assert!(t.graphics.is_empty());
    }

    #[test]
    fn line_and_char_editing_over_a_sixel() {
        let mut t = term();
        feed(&mut t, b"\x1b[3;1H");
        feed(&mut t, &sixel(40, 10));
        feed(&mut t, b"\x1b[4;3H\x1b[P");
        let cols: Vec<_> =
            t.graphics.placements.iter().filter(|p| p.line == 3).map(|p| (p.col, p.cols)).collect();
        assert_eq!(cols, vec![(0, 2)]);
        feed(&mut t, b"\x1b[5;1H\x1b[L");
        assert!(t.graphics.placements.iter().all(|p| p.line < 4));
    }

    #[test]
    fn kitty_images_survive_text_over_them() {
        let mut t = term();
        feed(&mut t, b"\x1b_Ga=T,f=24,s=1,v=1,c=4,r=2,i=1,C=1;AAAA\x1b\\");
        feed(&mut t, b"hello\r\n\x1b[K\x1b[2J");
        feed(&mut t, b"\x1b_Ga=T,f=24,s=1,v=1,c=4,r=2,i=1,C=1;AAAA\x1b\\xyz\x1b[K");
        assert_eq!(t.graphics.placements.len(), 1);
    }

    #[test]
    fn device_attributes_and_xtsmgraphics() {
        let mut t = term();
        assert_eq!(feed(&mut t, b"\x1b[c"), "\x1b[?62;4;22c");
        assert_eq!(feed(&mut t, b"\x1b[?1;1;0S"), "\x1b[?1;0;1024S");
        assert_eq!(feed(&mut t, b"\x1b[?1;4;0S"), "\x1b[?1;0;1024S");
        assert_eq!(feed(&mut t, b"\x1b[?2;1;0S"), "\x1b[?2;0;200;200S");
        assert_eq!(feed(&mut t, b"\x1b[?2;4;0S"), "\x1b[?2;0;10000;10000S");
        assert_eq!(feed(&mut t, b"\x1b[?2;3;100;100S"), "\x1b[?2;3;0S");
        assert_eq!(feed(&mut t, b"\x1b[?3;1;0S"), "\x1b[?3;1;0S");
        assert_eq!(feed(&mut t, b"\x1b[?1;9;0S"), "\x1b[?1;2;0S");
    }

    #[test]
    fn images_can_be_turned_off() {
        let mut t = term();
        feed(&mut t, &sixel(10, 2));
        feed(&mut t, b"\x1b[?1049h");
        feed(&mut t, b"\x1b_Ga=T,f=24,s=1,v=1,i=1;AAAA\x1b\\");
        let mut cfg = crate::config::Config::default();
        cfg.terminal.images = false;
        t.apply_config(&cfg);
        assert!(t.graphics.is_empty());
        feed(&mut t, b"\x1b[?1049l");
        assert!(t.graphics.is_empty());
        assert_eq!(feed(&mut t, b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c"), "\x1b[?62;22c");
        assert_eq!(feed(&mut t, b"\x1bZ"), "\x1b[?62;22c");
        assert_eq!(feed(&mut t, b"\x1b[?1;1;0S"), "\x1b[?1;3;0S");
        feed(&mut t, &sixel(10, 2));
        assert!(t.graphics.is_empty());
        feed(&mut t, b"\x1bc");
        assert_eq!(feed(&mut t, b"\x1b[c"), "\x1b[?62;22c");
        cfg.terminal.images = true;
        t.apply_config(&cfg);
        assert_eq!(
            feed(&mut t, b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c"),
            "\x1b_Gi=31;OK\x1b\\\x1b[?62;4;22c"
        );
    }

    #[test]
    fn sixel_on_the_alt_screen_scrolls_with_it() {
        let mut t = term();
        feed(&mut t, b"\x1b[?1049h");
        feed(&mut t, &sixel(10, 4));
        feed(&mut t, b"\x1b[10;1H\n");
        assert_eq!(t.graphics.placements[0].line - t.abs_line(0) as isize, -1);
        feed(&mut t, b"\x1b[?1049l");
        assert!(t.graphics.is_empty());
    }
}
