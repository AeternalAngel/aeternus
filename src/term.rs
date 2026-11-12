use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::cell::{flags, Cell, Color};
use crate::colors::{DEFAULT_BG, DEFAULT_FG};
use crate::config::Config;
use crate::grid::Grid;
use crate::parser::Perform;

const SCROLLBACK_LINES: usize = 10_000;

pub const SYNC_TIMEOUT: Duration = Duration::from_millis(150);

const OSC52_MAX_BYTES: usize = 1 << 20;

/// Flag na última célula de uma linha que continua na seguinte por quebra
/// automática. Serve para copiar texto sem inserir '\n' falsos.
pub const WRAPLINE: u16 = 1 << 8;

pub const WIDE_SPACER: u16 = 1 << 9;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorShape {
    Block,
    Underline,
    Bar,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseMode {
    Off,
    Click,
    Drag,
    Motion,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelMode {
    Simple,
    Word,
    Line,
}

/// Posição absoluta: `line` conta desde o início do histórico (estável ao rolar).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelPoint {
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Copy, Debug)]
struct Selection {
    anchor: SelPoint,
    head: SelPoint,
    mode: SelMode,
}

#[derive(Clone, Copy)]
struct Pen {
    fg: Color,
    bg: Color,
    flags: u16,
}

impl Pen {
    const DEFAULT: Pen = Pen { fg: Color::Default, bg: Color::Default, flags: 0 };
}

#[derive(Clone, Copy, Default)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
}

pub struct Term {
    pub grid: Grid,
    pub cursor: Cursor,
    pen: Pen,
    scroll_top: usize,
    scroll_bottom: usize,
    wrap_pending: bool,
    saved: Option<(Cursor, Pen)>,
    main_grid: Option<Grid>,
    scrollback: VecDeque<Vec<Cell>>,
    max_scrollback: usize,
    popped: usize,
    selection: Option<Selection>,
    default_fg: [u8; 3],
    default_bg: [u8; 3],
    default_cursor_shape: CursorShape,
    default_cursor_blink: bool,
    pub display_offset: usize,
    pub replies: Vec<u8>,
    pub autowrap: bool,
    pub cursor_visible: bool,
    pub cursor_shape: CursorShape,
    pub cursor_blink: bool,
    pub app_cursor_keys: bool,
    pub app_keypad: bool,
    pub bracketed_paste: bool,
    pub mouse_mode: MouseMode,
    pub mouse_sgr: bool,
    pub focus_reporting: bool,
    pub alt_scroll: bool,
    pub title: String,
    pub sync_output: bool,
    pub sync_since: Option<Instant>,
    pub fg_override: Option<[u8; 3]>,
    pub bg_override: Option<[u8; 3]>,
    pub cursor_override: Option<[u8; 3]>,
    pub ansi_override: std::collections::HashMap<u8, [u8; 3]>,
    pub clipboard_write: Option<(u8, String)>,
    pub osc52: bool,
}

impl Term {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            cursor: Cursor::default(),
            pen: Pen::DEFAULT,
            scroll_top: 0,
            scroll_bottom: rows - 1,
            wrap_pending: false,
            saved: None,
            main_grid: None,
            scrollback: VecDeque::new(),
            max_scrollback: SCROLLBACK_LINES,
            popped: 0,
            selection: None,
            default_fg: DEFAULT_FG,
            default_bg: DEFAULT_BG,
            default_cursor_shape: CursorShape::Block,
            default_cursor_blink: true,
            display_offset: 0,
            replies: Vec::new(),
            autowrap: true,
            cursor_visible: true,
            cursor_shape: CursorShape::Block,
            cursor_blink: true,
            app_cursor_keys: false,
            app_keypad: false,
            bracketed_paste: false,
            mouse_mode: MouseMode::Off,
            mouse_sgr: false,
            focus_reporting: false,
            alt_scroll: true,
            title: String::new(),
            sync_output: false,
            sync_since: None,
            fg_override: None,
            bg_override: None,
            cursor_override: None,
            ansi_override: std::collections::HashMap::new(),
            clipboard_write: None,
            osc52: true,
        }
    }

    /// Aplica a parte da configuração que vive no terminal. Chamado ao abrir
    /// e a cada recarga (Ctrl+Shift+R).
    pub fn apply_config(&mut self, cfg: &Config) {
        self.default_fg = cfg.palette.fg;
        self.default_bg = cfg.palette.bg;
        self.max_scrollback = cfg.terminal.scrollback;
        self.trim_scrollback();
        self.osc52 = cfg.terminal.osc52;
        self.default_cursor_shape = cfg.cursor.shape;
        self.default_cursor_blink = cfg.cursor.blink;
        self.cursor_shape = cfg.cursor.shape;
        self.cursor_blink = cfg.cursor.blink;
    }

    /// Descarta as linhas mais antigas se o histórico passou do limite.
    fn trim_scrollback(&mut self) {
        while self.scrollback.len() > self.max_scrollback {
            self.scrollback.pop_front();
            self.popped += 1;
        }
        self.display_offset = self.display_offset.min(self.scrollback.len());
    }

    pub fn cols(&self) -> usize { self.grid.cols() }
    pub fn rows(&self) -> usize { self.grid.rows() }

    pub fn is_alt_screen(&self) -> bool {
        self.main_grid.is_some()
    }

    /// Linha visível `r`, levando em conta a rolagem. Linhas do scrollback
    /// podem ser mais curtas que `cols` (espaços finais são cortados).
    pub fn visible_row(&self, r: usize) -> &[Cell] {
        let sb = self.scrollback.len();
        let idx = sb.saturating_sub(self.display_offset) + r;
        if idx < sb {
            self.scrollback[idx].as_slice()
        } else {
            self.grid.row(idx - sb)
        }
    }

    /// Rola a visão: delta > 0 vai para o passado. Retorna true se mudou.
    pub fn scroll_display(&mut self, delta: isize) -> bool {
        if self.main_grid.is_some() {
            return false;
        }
        let max = self.scrollback.len() as isize;
        let new = (self.display_offset as isize).saturating_add(delta).clamp(0, max) as usize;
        let changed = new != self.display_offset;
        self.display_offset = new;
        changed
    }

    pub fn sync_deadline(&self) -> Option<Instant> {
        if self.sync_output {
            self.sync_since.map(|t| t + SYNC_TIMEOUT)
        } else {
            None
        }
    }

    pub fn expire_sync(&mut self, now: Instant) -> bool {
        match self.sync_deadline() {
            Some(d) if now >= d => {
                self.sync_output = false;
                self.sync_since = None;
                true
            }
            _ => false,
        }
    }

    pub fn scroll_to_bottom(&mut self) -> bool {
        let changed = self.display_offset != 0;
        self.display_offset = 0;
        changed
    }


    /// Índice absoluto da linha visível `row` (não muda quando o conteúdo rola).
    pub fn abs_line(&self, row: usize) -> usize {
        self.popped + self.scrollback.len().saturating_sub(self.display_offset) + row
    }

    fn line_cells(&self, abs: usize) -> Option<&[Cell]> {
        let idx = abs.checked_sub(self.popped)?;
        let sb = self.scrollback.len();
        if idx < sb {
            Some(self.scrollback[idx].as_slice())
        } else if idx - sb < self.rows() {
            Some(self.grid.row(idx - sb))
        } else {
            None
        }
    }

    fn snap_col(&self, row: usize, col: usize) -> usize {
        if col > 0 && self.visible_row(row).get(col).map_or(false, |c| c.flags & WIDE_SPACER != 0) {
            col - 1
        } else {
            col
        }
    }

    pub fn selection_start(&mut self, row: usize, col: usize, mode: SelMode) {
        let col = self.snap_col(row, col);
        let p = SelPoint { line: self.abs_line(row), col };
        self.selection = Some(Selection { anchor: p, head: p, mode });
    }

    pub fn selection_update(&mut self, row: usize, col: usize) {
        let col = self.snap_col(row, col);
        let p = SelPoint { line: self.abs_line(row), col };
        if let Some(sel) = &mut self.selection {
            sel.head = p;
        }
    }

    pub fn selection_clear(&mut self) {
        self.selection = None;
    }

    /// Início e fim (inclusivos), já normalizados e com a expansão de palavra/linha.
    /// None se não há seleção ou se é um simples clique (sem arrastar).
    pub fn selection_bounds(&self) -> Option<(SelPoint, SelPoint)> {
        let sel = self.selection?;
        if sel.mode == SelMode::Simple && sel.anchor == sel.head {
            return None;
        }
        let key = |p: SelPoint| (p.line, p.col);
        let (mut a, mut b) = if key(sel.anchor) <= key(sel.head) {
            (sel.anchor, sel.head)
        } else {
            (sel.head, sel.anchor)
        };
        match sel.mode {
            SelMode::Simple => {}
            SelMode::Word => {
                a.col = self.word_bounds(a).0;
                b.col = self.word_bounds(b).1;
            }
            SelMode::Line => {
                a.col = 0;
                b.col = usize::MAX;
            }
        }
        Some((a, b))
    }

    fn word_bounds(&self, p: SelPoint) -> (usize, usize) {
        let Some(cells) = self.line_cells(p.line) else {
            return (p.col, p.col);
        };
        let at = |c: usize| cells.get(c).map_or(0, |x| char_kind(x.ch));
        let k = at(p.col);
        let mut s = p.col;
        while s > 0 && at(s - 1) == k {
            s -= 1;
        }
        let limit = self.cols().max(cells.len());
        let mut e = p.col;
        while e + 1 < limit && at(e + 1) == k {
            e += 1;
        }
        (s, e)
    }

    pub fn selection_text(&self) -> Option<String> {
        let (a, b) = self.selection_bounds()?;
        let mut out = String::new();
        for line in a.line..=b.line {
            let cells = self.line_cells(line).unwrap_or(&[]);
            let start = if line == a.line { a.col } else { 0 };
            let end = if line == b.line { b.col.saturating_add(1) } else { usize::MAX };
            let end = end.min(cells.len());
            let start = start.min(end);
            let wrapped =
                line != b.line && cells.last().map_or(false, |c| c.flags & WRAPLINE != 0);
            let mut text: String = cells[start..end]
                .iter()
                .filter(|c| c.flags & WIDE_SPACER == 0)
                .map(|c| c.ch)
                .collect();
            if !wrapped {
                let n = text.trim_end_matches(' ').len();
                text.truncate(n);
            }
            out.push_str(&text);
            if line != b.line && !wrapped {
                out.push('\n');
            }
        }
        Some(out)
    }


    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.selection = None;
        if self.main_grid.is_some() {
            self.grid = self.grid.resized(cols, rows);
            if let Some(g) = &mut self.main_grid {
                *g = g.resized(cols, rows);
            }
        } else if cols != self.cols() {
            self.reflow(cols, rows);
        } else {
            let old_rows = self.rows();
            let mut shift_up = 0;
            let mut pull = 0;
            if rows < old_rows {
                if self.cursor.row >= rows {
                    shift_up = self.cursor.row + 1 - rows;
                }
            } else if rows > old_rows {
                pull = (rows - old_rows).min(self.scrollback.len());
            }
            if shift_up > 0 {
                self.push_scrollback(shift_up);
            }

            let mut g = Grid::new(cols, rows);
            for i in 0..pull {
                if let Some(line) = self.scrollback.pop_back() {
                    let n = line.len().min(cols);
                    g.row_mut(pull - 1 - i)[..n].copy_from_slice(&line[..n]);
                }
            }
            for r in shift_up..old_rows {
                let nr = r - shift_up + pull;
                if nr >= rows {
                    break;
                }
                g.row_mut(nr).copy_from_slice(self.grid.row(r));
            }
            self.grid = g;
            self.cursor.row = (self.cursor.row + pull).saturating_sub(shift_up);
            self.display_offset = self.display_offset.min(self.scrollback.len());
        }
        fix_wide_pairs(&mut self.grid);
        if let Some(g) = &mut self.main_grid {
            fix_wide_pairs(g);
        }
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        self.move_to(self.cursor.row, self.cursor.col);
    }

    fn reflow(&mut self, cols: usize, rows: usize) {
        let old_sb_len = self.scrollback.len();
        let old_rows = self.rows();
        let cursor_phys_index = old_sb_len + self.cursor.row;
        let cursor_col = self.cursor.col;

        let mut physical: Vec<(Vec<Cell>, bool)> = Vec::with_capacity(old_sb_len + old_rows);
        for line in self.scrollback.drain(..) {
            let wrapped = line.last().map_or(false, |c| c.flags & WRAPLINE != 0);
            physical.push((line, wrapped));
        }
        for r in 0..old_rows {
            let row = self.grid.row(r);
            let wrapped = row[row.len() - 1].flags & WRAPLINE != 0;
            let len = if wrapped {
                row.len()
            } else {
                row.iter().rposition(|c| *c != Cell::BLANK).map_or(0, |p| p + 1)
            };
            physical.push((row[..len].to_vec(), wrapped));
        }

        let mut new_rows: Vec<Vec<Cell>> = Vec::new();
        let mut new_cursor = (0usize, 0usize);
        let mut logical: Vec<Cell> = Vec::new();
        let mut cursor_offset: Option<usize> = None;

        for (i, (line, wrapped)) in physical.into_iter().enumerate() {
            if i == cursor_phys_index {
                cursor_offset = Some(logical.len() + cursor_col);
            }
            logical.extend(line);
            if !wrapped {
                let base = new_rows.len();
                let chunks = wrap_logical_line(std::mem::take(&mut logical), cols);
                if let Some(off) = cursor_offset.take() {
                    new_cursor = (base + off / cols, off % cols);
                }
                new_rows.extend(chunks);
            }
        }
        if !logical.is_empty() || new_rows.is_empty() {
            let base = new_rows.len();
            let chunks = wrap_logical_line(logical, cols);
            if let Some(off) = cursor_offset {
                new_cursor = (base + off / cols, off % cols);
            }
            new_rows.extend(chunks);
        }

        let total = new_rows.len();
        let grid_start = total.saturating_sub(rows);
        let pad_top = rows - (total - grid_start);
        let mut g = Grid::new(cols, rows);
        for (i, row) in new_rows[grid_start..].iter().enumerate() {
            g.row_mut(pad_top + i).copy_from_slice(row);
        }
        self.grid = g;

        for mut line in new_rows.drain(..grid_start) {
            let n = line.iter().rposition(|c| *c != Cell::BLANK).map_or(0, |p| p + 1);
            line.truncate(n);
            self.scrollback.push_back(line);
        }
        self.trim_scrollback();

        let cur_row = new_cursor.0.max(grid_start) - grid_start;
        self.cursor.row = pad_top + cur_row;
        self.cursor.col = new_cursor.1;
        self.display_offset = 0;
    }

    /// Representação em texto da grade, para testar sem janela.
    pub fn dump(&self) -> String {
        let mut s = String::new();
        s.push('┌');
        s.push_str(&"─".repeat(self.cols()));
        s.push_str("┐\n");
        for r in 0..self.rows() {
            s.push('│');
            for (c, cell) in self.grid.row(r).iter().enumerate() {
                if r == self.cursor.row && c == self.cursor.col {
                    s.push('█');
                } else {
                    s.push(cell.ch);
                }
            }
            s.push_str("│\n");
        }
        s.push('└');
        s.push_str(&"─".repeat(self.cols()));
        s.push('┘');
        s
    }


    fn blank(&self) -> Cell {
        Cell { bg: self.pen.bg, ..Cell::BLANK }
    }

    fn clear_wide_edges(&mut self, r: usize, from: usize, to: usize) {
        let b = self.blank();
        let cols = self.cols();
        let row = self.grid.row_mut(r);
        if from > 0 && from < cols && row[from].flags & WIDE_SPACER != 0 {
            row[from - 1] = b;
        }
        if to > 0 && to < cols && row[to].flags & WIDE_SPACER != 0 {
            row[to] = b;
        }
    }

    fn reply(&mut self, s: &str) {
        self.replies.extend_from_slice(s.as_bytes());
    }

    fn move_to(&mut self, row: usize, col: usize) {
        self.cursor.row = row.min(self.rows() - 1);
        self.cursor.col = col.min(self.cols() - 1);
        self.wrap_pending = false;
    }

    /// Move as `n` linhas do topo da grade para o scrollback (antes de sumirem).
    fn push_scrollback(&mut self, n: usize) {
        if self.max_scrollback == 0 {
            return;
        }
        for i in 0..n {
            let row = self.grid.row(i);
            let len = row.iter().rposition(|c| *c != Cell::BLANK).map_or(0, |p| p + 1);
            let mut line = if self.scrollback.len() >= self.max_scrollback {
                self.popped += 1;
                let mut l = self.scrollback.pop_front().unwrap_or_default();
                l.clear();
                l
            } else {
                Vec::with_capacity(len)
            };
            line.extend_from_slice(&row[..len]);
            self.scrollback.push_back(line);
        }
        if self.display_offset > 0 {
            self.display_offset = (self.display_offset + n).min(self.scrollback.len());
        }
    }

    /// Scroll da região ativa; só vira histórico se for a tela inteira.
    fn scroll_up_region(&mut self, n: usize) {
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        if top == 0 && bottom == self.rows() - 1 && self.main_grid.is_none() {
            self.push_scrollback(n.min(self.rows()));
        }
        let b = self.blank();
        self.grid.scroll_up(top, bottom, n, b);
    }

    fn linefeed(&mut self) {
        self.wrap_pending = false;
        if self.cursor.row == self.scroll_bottom {
            self.scroll_up_region(1);
        } else if self.cursor.row + 1 < self.rows() {
            self.cursor.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.wrap_pending = false;
        if self.cursor.row == self.scroll_top {
            let b = self.blank();
            self.grid.scroll_down(self.scroll_top, self.scroll_bottom, 1, b);
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
        }
    }

    fn save_cursor(&mut self) {
        self.saved = Some((self.cursor, self.pen));
    }

    fn restore_cursor(&mut self) {
        if let Some((c, pen)) = self.saved {
            self.pen = pen;
            self.move_to(c.row, c.col);
        }
    }

    fn reset(&mut self) {
        let (c, r) = (self.cols(), self.rows());
        let title = std::mem::take(&mut self.title);
        let replies = std::mem::take(&mut self.replies);
        let (fg, bg) = (self.default_fg, self.default_bg);
        let (shape, blink) = (self.default_cursor_shape, self.default_cursor_blink);
        let max_sb = self.max_scrollback;
        let osc52 = self.osc52;
        *self = Term::new(c, r);
        self.osc52 = osc52;
        self.title = title;
        self.replies = replies;
        self.default_fg = fg;
        self.default_bg = bg;
        self.default_cursor_shape = shape;
        self.default_cursor_blink = blink;
        self.cursor_shape = shape;
        self.cursor_blink = blink;
        self.max_scrollback = max_sb;
    }

    fn erase_display(&mut self, mode: u16) {
        let b = self.blank();
        let (r, c) = (self.cursor.row, self.cursor.col);
        let (cols, rows) = (self.cols(), self.rows());
        match mode {
            0 => {
                self.grid.fill(r, c, cols, b);
                for i in (r + 1)..rows {
                    self.grid.fill(i, 0, cols, b);
                }
            }
            1 => {
                for i in 0..r {
                    self.grid.fill(i, 0, cols, b);
                }
                self.grid.fill(r, 0, c + 1, b);
            }
            2 => {
                for i in 0..rows {
                    self.grid.fill(i, 0, cols, b);
                }
            }
            3 => {
                self.popped += self.scrollback.len();
                self.scrollback.clear();
                self.display_offset = 0;
                self.selection = None;
            }
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let b = self.blank();
        let (r, c, cols) = (self.cursor.row, self.cursor.col, self.cols());
        match mode {
            0 => self.grid.fill(r, c, cols, b),
            1 => self.grid.fill(r, 0, c + 1, b),
            2 => self.grid.fill(r, 0, cols, b),
            _ => {}
        }
    }

    fn insert_lines(&mut self, n: usize) {
        let r = self.cursor.row;
        if r >= self.scroll_top && r <= self.scroll_bottom {
            let b = self.blank();
            self.grid.scroll_down(r, self.scroll_bottom, n, b);
            self.cursor.col = 0;
            self.wrap_pending = false;
        }
    }

    fn delete_lines(&mut self, n: usize) {
        let r = self.cursor.row;
        if r >= self.scroll_top && r <= self.scroll_bottom {
            let b = self.blank();
            self.grid.scroll_up(r, self.scroll_bottom, n, b);
            self.cursor.col = 0;
            self.wrap_pending = false;
        }
    }

    fn insert_chars(&mut self, n: usize) {
        let b = self.blank();
        let (r, c, cols) = (self.cursor.row, self.cursor.col, self.cols());
        let n = n.min(cols - c);
        let row = self.grid.row_mut(r);
        if c > 0 && row[c].flags & WIDE_SPACER != 0 {
            row[c - 1] = b;
            row[c] = b;
        }
        let k = cols - n;
        if k > c && k < cols && row[k].flags & WIDE_SPACER != 0 {
            row[k - 1] = b;
        }
        row.copy_within(c..cols - n, c + n);
        row[c..c + n].fill(b);
    }

    fn delete_chars(&mut self, n: usize) {
        let b = self.blank();
        let (r, c, cols) = (self.cursor.row, self.cursor.col, self.cols());
        let n = n.min(cols - c);
        self.clear_wide_edges(r, c, c + n);
        let row = self.grid.row_mut(r);
        row.copy_within(c + n..cols, c);
        row[cols - n..].fill(b);
    }

    fn erase_chars(&mut self, n: usize) {
        let b = self.blank();
        let (r, c, cols) = (self.cursor.row, self.cursor.col, self.cols());
        let end = (c + n).min(cols);
        self.clear_wide_edges(r, c, end);
        self.grid.fill(r, c, end, b);
    }

    fn set_alt_screen(&mut self, on: bool, save: bool) {
        if on == self.main_grid.is_some() {
            return;
        }
        self.selection = None;
        if on {
            self.display_offset = 0;
            if save { self.save_cursor(); }
            let blank = Grid::new(self.cols(), self.rows());
            self.main_grid = Some(std::mem::replace(&mut self.grid, blank));
        } else {
            if let Some(g) = self.main_grid.take() {
                self.grid = g;
            }
            if save { self.restore_cursor(); }
        }
    }

    fn set_mouse(&mut self, mode: MouseMode, on: bool) {
        if on {
            self.mouse_mode = mode;
        } else if self.mouse_mode == mode {
            self.mouse_mode = MouseMode::Off;
        }
    }

    fn set_modes(&mut self, params: &[u16], on: bool) {
        for &m in params {
            match m {
                1 => self.app_cursor_keys = on,
                7 => self.autowrap = on,
                12 => self.cursor_blink = on,
                25 => self.cursor_visible = on,
                47 | 1047 => self.set_alt_screen(on, false),
                1000 => self.set_mouse(MouseMode::Click, on),
                1002 => self.set_mouse(MouseMode::Drag, on),
                1003 => self.set_mouse(MouseMode::Motion, on),
                1004 => self.focus_reporting = on,
                1006 => self.mouse_sgr = on,
                1007 => self.alt_scroll = on,
                1049 => self.set_alt_screen(on, true),
                2004 => self.bracketed_paste = on,
                2026 => {
                    if on && !self.sync_output {
                        self.sync_since = Some(Instant::now());
                    } else if !on {
                        self.sync_since = None;
                    }
                    self.sync_output = on;
                }
                _ => {}
            }
        }
    }

    fn decrqm(&mut self, mode: u16) {
        let state = match mode {
            1 => Some(self.app_cursor_keys),
            7 => Some(self.autowrap),
            12 => Some(self.cursor_blink),
            25 => Some(self.cursor_visible),
            47 | 1047 => Some(self.main_grid.is_some()),
            1000 => Some(self.mouse_mode == MouseMode::Click),
            1002 => Some(self.mouse_mode == MouseMode::Drag),
            1003 => Some(self.mouse_mode == MouseMode::Motion),
            1004 => Some(self.focus_reporting),
            1006 => Some(self.mouse_sgr),
            1007 => Some(self.alt_scroll),
            1049 => Some(self.main_grid.is_some()),
            2004 => Some(self.bracketed_paste),
            2026 => Some(self.sync_output),
            _ => None,
        };
        let ps = match state {
            Some(true) => 1,
            Some(false) => 2,
            None => 0,
        };
        self.reply(&format!("\x1b[?{};{}$y", mode, ps));
    }

    /// DECSCUSR: `CSI Ps SP q`
    fn set_cursor_style(&mut self, n: u16) {
        let (shape, blink) = match n {
            0 => (self.default_cursor_shape, self.default_cursor_blink),
            1 => (CursorShape::Block, true),
            2 => (CursorShape::Block, false),
            3 => (CursorShape::Underline, true),
            4 => (CursorShape::Underline, false),
            5 => (CursorShape::Bar, true),
            6 => (CursorShape::Bar, false),
            _ => return,
        };
        self.cursor_shape = shape;
        self.cursor_blink = blink;
    }

    fn sgr(&mut self, params: &[u16], subs: &[bool]) {
        if params.is_empty() {
            self.pen = Pen::DEFAULT;
            return;
        }
        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => self.pen = Pen::DEFAULT,
                1 => self.pen.flags |= flags::BOLD,
                2 => self.pen.flags |= flags::DIM,
                3 => self.pen.flags |= flags::ITALIC,
                4 | 21 => self.pen.flags |= flags::UNDERLINE,
                5 | 6 => self.pen.flags |= flags::BLINK,
                7 => self.pen.flags |= flags::INVERSE,
                8 => self.pen.flags |= flags::HIDDEN,
                9 => self.pen.flags |= flags::STRIKE,
                22 => self.pen.flags &= !(flags::BOLD | flags::DIM),
                23 => self.pen.flags &= !flags::ITALIC,
                24 => self.pen.flags &= !flags::UNDERLINE,
                25 => self.pen.flags &= !flags::BLINK,
                27 => self.pen.flags &= !flags::INVERSE,
                28 => self.pen.flags &= !flags::HIDDEN,
                29 => self.pen.flags &= !flags::STRIKE,
                30..=37 => self.pen.fg = Color::Indexed((p - 30) as u8),
                38 => {
                    if let Some(c) = extended_color(params, subs, &mut i) {
                        self.pen.fg = c;
                    }
                }
                39 => self.pen.fg = Color::Default,
                40..=47 => self.pen.bg = Color::Indexed((p - 40) as u8),
                48 => {
                    if let Some(c) = extended_color(params, subs, &mut i) {
                        self.pen.bg = c;
                    }
                }
                49 => self.pen.bg = Color::Default,
                58 => {
                    let _ = extended_color(params, subs, &mut i);
                }
                59 => {}
                90..=97 => self.pen.fg = Color::Indexed((p - 90 + 8) as u8),
                100..=107 => self.pen.bg = Color::Indexed((p - 100 + 8) as u8),
                _ => {}
            }
            i += 1;
        }
    }
}

fn is_wide(c: char) -> bool {
    unicode_width::UnicodeWidthChar::width(c) == Some(2)
}

fn wrap_logical_line(line: Vec<Cell>, cols: usize) -> Vec<Vec<Cell>> {
    let len = line.len();
    if len == 0 {
        return vec![vec![Cell::BLANK; cols]];
    }
    let mut out = Vec::with_capacity(len.div_ceil(cols).max(1));
    let mut i = 0;
    while i < len {
        let end = (i + cols).min(len);
        let mut row = line[i..end].to_vec();
        if row.len() < cols {
            row.resize(cols, Cell::BLANK);
        } else if end < len {
            row[cols - 1].flags |= WRAPLINE;
        }
        out.push(row);
        i = end;
    }
    out
}

fn fix_wide_pairs(g: &mut Grid) {
    let cols = g.cols();
    for r in 0..g.rows() {
        let row = g.row_mut(r);
        for c in 0..cols {
            if row[c].flags & WIDE_SPACER != 0 {
                if c == 0 || !is_wide(row[c - 1].ch) || row[c - 1].flags & WIDE_SPACER != 0 {
                    row[c] = Cell { flags: row[c].flags & !WIDE_SPACER, ..row[c] };
                }
            } else if is_wide(row[c].ch) && (c + 1 >= cols || row[c + 1].flags & WIDE_SPACER == 0) {
                row[c] = Cell { ch: ' ', ..row[c] };
            }
        }
    }
}

/// 0 = espaço, 1 = separador, 2 = parte de palavra (para o clique duplo).
fn char_kind(c: char) -> u8 {
    if c == ' ' || c == '\0' {
        0
    } else if ",│`|:\"'()[]{}<>".contains(c) {
        1
    } else {
        2
    }
}

fn extended_color(params: &[u16], subs: &[bool], i: &mut usize) -> Option<Color> {
    match *params.get(*i + 1)? {
        5 => {
            let n = *params.get(*i + 2)?;
            *i += 2;
            Some(Color::Indexed(n as u8))
        }
        2 => {
            let base = *i + 2;
            let mut run = 0usize;
            while matches!(subs.get(base + run), Some(true)) {
                run += 1;
            }
            let skip = if run >= 4 { 1 } else { 0 };
            let r = *params.get(base + skip)?;
            let g = *params.get(base + skip + 1)?;
            let b = *params.get(base + skip + 2)?;
            *i = base + skip + 2;
            Some(Color::Rgb(r as u8, g as u8, b as u8))
        }
        _ => None,
    }
}

fn parse_color_spec(s: &[u8]) -> Option<[u8; 3]> {
    let s = std::str::from_utf8(s).ok()?.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return crate::colors::parse_hex(hex);
    }
    let rest = s.strip_prefix("rgb:")?;
    let mut it = rest.split('/');
    let comp = |s: &str| -> Option<u8> {
        if s.is_empty() || s.len() > 4 {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        let max = (1u32 << (4 * s.len())) - 1;
        Some(((v * 255) / max.max(1)) as u8)
    };
    let r = comp(it.next()?)?;
    let g = comp(it.next()?)?;
    let b = comp(it.next()?)?;
    if it.next().is_some() {
        return None;
    }
    Some([r, g, b])
}

fn base64_decode(data: &[u8]) -> Option<Vec<u8>> {
    fn val(b: u8) -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some((b - b'A') as u32),
            b'a'..=b'z' => Some((b - b'a') as u32 + 26),
            b'0'..=b'9' => Some((b - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &b in data {
        if b == b'=' {
            break;
        }
        let v = val(b)?;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

impl Perform for Term {
    fn print(&mut self, c: char) {
        if self.wrap_pending {
            if self.autowrap {
                let last = self.cols() - 1;
                self.grid.row_mut(self.cursor.row)[last].flags |= WRAPLINE;
                self.cursor.col = 0;
                self.linefeed();
            }
            self.wrap_pending = false;
        }
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if w == 0 {
            return;
        }
        let cols = self.cols();
        if w == 2 && self.cursor.col + 2 > cols {
            if self.autowrap {
                let last = cols - 1;
                self.grid.row_mut(self.cursor.row)[last].flags |= WRAPLINE;
                self.cursor.col = 0;
                self.linefeed();
            } else {
                self.cursor.col = cols - 1;
            }
        }
        let col = self.cursor.col;
        self.clear_wide_edges(self.cursor.row, col, (col + w).min(cols));
        let cell = Cell { ch: c, fg: self.pen.fg, bg: self.pen.bg, flags: self.pen.flags };
        self.grid.row_mut(self.cursor.row)[self.cursor.col] = cell;
        if w == 2 && self.cursor.col + 1 < cols {
            let spacer =
                Cell { ch: ' ', fg: self.pen.fg, bg: self.pen.bg, flags: self.pen.flags | WIDE_SPACER };
            self.grid.row_mut(self.cursor.row)[self.cursor.col + 1] = spacer;
        }
        let advance = if w == 2 { 2 } else { 1 };
        if self.cursor.col + advance < cols {
            self.cursor.col += advance;
        } else {
            self.cursor.col = cols - 1;
            self.wrap_pending = self.autowrap;
        }
    }

    /// Caminho rápido: trecho contínuo de ASCII imprimível escrito em lote.
    fn print_ascii(&mut self, mut s: &[u8]) {
        let cols = self.cols();
        while !s.is_empty() {
            if self.wrap_pending {
                if self.autowrap {
                    let last = cols - 1;
                    self.grid.row_mut(self.cursor.row)[last].flags |= WRAPLINE;
                    self.cursor.col = 0;
                    self.linefeed();
                }
                self.wrap_pending = false;
            }
            let col = self.cursor.col;
            let n = s.len().min(cols - col);
            self.clear_wide_edges(self.cursor.row, col, col + n);
            let (fg, bg, fl) = (self.pen.fg, self.pen.bg, self.pen.flags);
            let row = self.grid.row_mut(self.cursor.row);
            for (cell, &b) in row[col..col + n].iter_mut().zip(&s[..n]) {
                *cell = Cell { ch: b as char, fg, bg, flags: fl };
            }
            s = &s[n..];
            if col + n < cols {
                self.cursor.col = col + n;
            } else {
                self.cursor.col = cols - 1;
                self.wrap_pending = self.autowrap;
            }
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => {
                self.cursor.col = self.cursor.col.saturating_sub(1);
                self.wrap_pending = false;
            }
            0x09 => {
                let next = ((self.cursor.col / 8) + 1) * 8;
                self.cursor.col = next.min(self.cols() - 1);
            }
            0x0A | 0x0B | 0x0C => self.linefeed(),
            0x0D => {
                self.cursor.col = 0;
                self.wrap_pending = false;
            }
            _ => {}
        }
    }

    fn csi(&mut self, params: &[u16], subs: &[bool], inter: &[u8], private: Option<u8>, action: u8) {
        if !inter.is_empty() {
            match (inter, private, action) {
                ([b' '], None, b'q') => {
                    self.set_cursor_style(params.first().copied().unwrap_or(0));
                }
                ([b'$'], Some(b'?'), b'p') => {
                    self.decrqm(params.first().copied().unwrap_or(0));
                }
                _ => {}
            }
            return;
        }
        let p = |i: usize, d: usize| -> usize {
            match params.get(i) {
                Some(&v) if v != 0 => v as usize,
                _ => d,
            }
        };
        let first = params.first().copied().unwrap_or(0);
        let (row, col) = (self.cursor.row, self.cursor.col);

        match (private, action) {
            (Some(b'?'), b'h') => self.set_modes(params, true),
            (Some(b'?'), b'l') => self.set_modes(params, false),
            (Some(b'>'), b'c') if first == 0 => self.reply("\x1b[>0;0;0c"), // Secondary DA
            (Some(b'>'), b'q') => {
                self.reply(concat!("\x1bP>|aeternus ", env!("CARGO_PKG_VERSION"), "\x1b\\"))
            }
            (Some(_), _) => {}
            (None, b'A') => self.move_to(row.saturating_sub(p(0, 1)), col),
            (None, b'B') => self.move_to(row + p(0, 1), col),
            (None, b'C') => self.move_to(row, col + p(0, 1)),
            (None, b'D') => self.move_to(row, col.saturating_sub(p(0, 1))),
            (None, b'E') => self.move_to(row + p(0, 1), 0),
            (None, b'F') => self.move_to(row.saturating_sub(p(0, 1)), 0),
            (None, b'G') => self.move_to(row, p(0, 1) - 1),
            (None, b'H') | (None, b'f') => self.move_to(p(0, 1) - 1, p(1, 1) - 1),
            (None, b'd') => self.move_to(p(0, 1) - 1, col),
            (None, b'J') => self.erase_display(first),
            (None, b'K') => self.erase_line(first),
            (None, b'L') => self.insert_lines(p(0, 1)),
            (None, b'M') => self.delete_lines(p(0, 1)),
            (None, b'@') => self.insert_chars(p(0, 1)),
            (None, b'P') => self.delete_chars(p(0, 1)),
            (None, b'X') => self.erase_chars(p(0, 1)),
            (None, b'S') => self.scroll_up_region(p(0, 1)),
            (None, b'T') => {
                let b = self.blank();
                self.grid.scroll_down(self.scroll_top, self.scroll_bottom, p(0, 1), b);
            }
            (None, b'm') => self.sgr(params, subs),
            (None, b'r') => {
                let top = p(0, 1) - 1;
                let bottom = p(1, self.rows()).min(self.rows()) - 1;
                if top < bottom {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                    self.move_to(0, 0);
                }
            }
            (None, b's') => self.save_cursor(),
            (None, b'u') => self.restore_cursor(),
            (None, b'c') if first == 0 => self.reply("\x1b[?62;c"), // Primary DA
            (None, b'n') => match first {
                5 => self.reply("\x1b[0n"), // "estou ok"
                6 => {
                    let s = format!("\x1b[{};{}R", row + 1, col + 1); // posição do cursor
                    self.reply(&s);
                }
                _ => {}
            },
            (None, b't') => {
                if first == 18 {
                    let s = format!("\x1b[8;{};{}t", self.rows(), self.cols());
                    self.reply(&s);
                }
            }
            _ => {}
        }
    }

    fn esc(&mut self, inter: &[u8], byte: u8) {
        if !inter.is_empty() {
            return;
        }
        match byte {
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            b'D' => self.linefeed(),
            b'E' => {
                self.cursor.col = 0;
                self.linefeed();
            }
            b'M' => self.reverse_index(),
            b'=' => self.app_keypad = true,
            b'>' => self.app_keypad = false,
            b'Z' => self.reply("\x1b[?62;c"), // DECID
            b'c' => self.reset(),
            _ => {}
        }
    }

    fn osc(&mut self, data: &[u8]) {
        let mut top = data.splitn(2, |&b| b == b';');
        let code = top.next().unwrap_or(&[]);
        let rest = top.next().unwrap_or(&[]);
        match code {
            b"0" | b"2" => self.title = String::from_utf8_lossy(rest).into_owned(),
            b"4" => {
                let mut it = rest.split(|&b| b == b';');
                loop {
                    let (Some(idx), Some(spec)) = (it.next(), it.next()) else { break };
                    if spec == b"?" {
                        continue;
                    }
                    if let (Ok(i), Some(c)) = (
                        std::str::from_utf8(idx).unwrap_or("").parse::<u8>(),
                        parse_color_spec(spec),
                    ) {
                        if (i as usize) < 16 {
                            self.ansi_override.insert(i, c);
                        }
                    }
                }
            }
            b"10" | b"11" | b"12" => {
                if rest == b"?" {
                    let c = match code {
                        b"10" => self.fg_override.unwrap_or(self.default_fg),
                        b"11" => self.bg_override.unwrap_or(self.default_bg),
                        _ => self.cursor_override.unwrap_or(self.default_fg),
                    };
                    let s = format!(
                        "\x1b]{};rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}\x1b\\",
                        String::from_utf8_lossy(code),
                        c[0], c[0], c[1], c[1], c[2], c[2]
                    );
                    self.reply(&s);
                } else if let Some(c) = parse_color_spec(rest) {
                    match code {
                        b"10" => self.fg_override = Some(c),
                        b"11" => self.bg_override = Some(c),
                        _ => self.cursor_override = Some(c),
                    }
                }
            }
            b"104" => {
                if rest.is_empty() {
                    self.ansi_override.clear();
                } else {
                    for idx in rest.split(|&b| b == b';') {
                        if let Ok(i) = std::str::from_utf8(idx).unwrap_or("").parse::<u8>() {
                            self.ansi_override.remove(&i);
                        }
                    }
                }
            }
            b"110" => self.fg_override = None,
            b"111" => self.bg_override = None,
            b"112" => self.cursor_override = None,
            b"52" if self.osc52 => {
                let mut it = rest.splitn(2, |&b| b == b';');
                let sel = it.next().unwrap_or(&[]);
                let payload = it.next().unwrap_or(&[]);
                if payload != b"?" && payload != b"!" {
                    if let Some(text) = base64_decode(payload)
                        .filter(|b| b.len() <= OSC52_MAX_BYTES)
                        .and_then(|b| String::from_utf8(b).ok())
                    {
                        let kind = sel.first().copied().unwrap_or(b'c');
                        self.clipboard_write = Some((kind, text));
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn feed(t: &mut Term, s: &str) {
        let mut p = Parser::new();
        p.advance(t, s.as_bytes());
    }

    #[test]
    fn scroll_region() {
        let mut t = Term::new(10, 5);
        feed(&mut t, "\x1b[2;4r");
        feed(&mut t, "a\r\nb\r\nc\r\n");
        let dump = t.dump();
        assert!(dump.contains('a'));
        assert!(dump.contains('b'));
        assert!(dump.contains('c'));
    }

    #[test]
    fn scroll_region_limits_scrollback() {
        let mut t = Term::new(10, 5);
        feed(&mut t, "top\r\n");
        feed(&mut t, "\x1b[2;4r");
        for i in 0..10 {
            feed(&mut t, &format!("{i}\r\n"));
        }
        assert!(!t.scroll_display(1));
    }

    #[test]
    fn erase_display_modes() {
        let mut t = Term::new(5, 3);
        feed(&mut t, "aaaaa\r\nbbbbb\r\nccccc");
        feed(&mut t, "\x1b[2;3H");
        feed(&mut t, "\x1b[0J");
        let dump = t.dump();
        assert!(dump.contains("aaaaa"));
        assert!(!dump.contains("bbbbb"));
        assert!(!dump.contains("ccccc"));

        let mut t2 = Term::new(5, 3);
        feed(&mut t2, "aaaaa\r\nbbbbb\r\nccccc");
        feed(&mut t2, "\x1b[2;3H");
        feed(&mut t2, "\x1b[1J");
        let dump2 = t2.dump();
        assert!(!dump2.contains("aaaaa"));
        assert!(dump2.contains("ccccc"));

        let mut t3 = Term::new(5, 3);
        feed(&mut t3, "aaaaa\r\nbbbbb\r\nccccc");
        feed(&mut t3, "\x1b[2J");
        let dump3 = t3.dump();
        assert!(!dump3.contains('a'));
        assert!(!dump3.contains('b'));
        assert!(!dump3.contains('c'));
    }

    #[test]
    fn erase_line_modes() {
        let mut t = Term::new(5, 1);
        feed(&mut t, "abcde");
        feed(&mut t, "\x1b[3G");
        feed(&mut t, "\x1b[0K");
        assert_eq!(t.grid.row(0).iter().map(|c| c.ch).collect::<String>(), "ab   ");

        let mut t2 = Term::new(5, 1);
        feed(&mut t2, "abcde");
        feed(&mut t2, "\x1b[3G");
        feed(&mut t2, "\x1b[1K");
        assert_eq!(t2.grid.row(0).iter().map(|c| c.ch).collect::<String>(), "   de");
    }

    #[test]
    fn wrap_sets_wrapline_and_moves_cursor() {
        let mut t = Term::new(5, 2);
        feed(&mut t, "abcdef");
        assert_eq!(t.grid.row(0)[4].flags & WRAPLINE, WRAPLINE);
        assert_eq!(t.cursor.row, 1);
        assert_eq!(t.cursor.col, 1);
        assert_eq!(t.grid.row(1)[0].ch, 'f');
    }

    #[test]
    fn sgr_basic_colors() {
        let mut t = Term::new(5, 1);
        feed(&mut t, "\x1b[31;44mx");
        let cell = t.grid.row(0)[0];
        assert_eq!(cell.fg, Color::Indexed(1));
        assert_eq!(cell.bg, Color::Indexed(4));
    }

    #[test]
    fn sgr_extended_38_48_semicolon() {
        let mut t = Term::new(5, 1);
        feed(&mut t, "\x1b[38;2;10;20;30;48;5;9mx");
        let cell = t.grid.row(0)[0];
        assert_eq!(cell.fg, Color::Rgb(10, 20, 30));
        assert_eq!(cell.bg, Color::Indexed(9));
    }

    #[test]
    fn sgr_extended_colon_truecolor_no_colorspace() {
        let mut t = Term::new(5, 1);
        feed(&mut t, "\x1b[38:2:10:20:30mx");
        let cell = t.grid.row(0)[0];
        assert_eq!(cell.fg, Color::Rgb(10, 20, 30));
    }

    #[test]
    fn sgr_extended_colon_truecolor_with_colorspace() {
        let mut t = Term::new(5, 1);
        feed(&mut t, "\x1b[38:2::10:20:30mx");
        let cell = t.grid.row(0)[0];
        assert_eq!(cell.fg, Color::Rgb(10, 20, 30));
    }

    #[test]
    fn sgr_colon_does_not_break_alignment() {
        let mut t = Term::new(5, 1);
        feed(&mut t, "\x1b[38:2::10:20:30;1mx");
        let cell = t.grid.row(0)[0];
        assert_eq!(cell.fg, Color::Rgb(10, 20, 30));
        assert_eq!(cell.flags & flags::BOLD, flags::BOLD);
    }

    #[test]
    fn sgr_58_59_ignored_without_breaking_params() {
        let mut t = Term::new(5, 1);
        feed(&mut t, "\x1b[58:2::1:2:3;59;1mx");
        let cell = t.grid.row(0)[0];
        assert_eq!(cell.fg, Color::Default);
        assert_eq!(cell.bg, Color::Default);
        assert_eq!(cell.flags & flags::BOLD, flags::BOLD);
    }

    #[test]
    fn selection_basic_text() {
        let mut t = Term::new(10, 2);
        feed(&mut t, "hello\r\nworld");
        t.selection_start(0, 0, SelMode::Simple);
        t.selection_update(0, 4);
        assert_eq!(t.selection_text(), Some("hello".to_string()));
    }

    #[test]
    fn wide_char_takes_two_cells() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "你好");
        assert_eq!(t.grid.row(0)[0].ch, '你');
        assert_eq!(t.grid.row(0)[1].flags & WIDE_SPACER, WIDE_SPACER);
        assert_eq!(t.grid.row(0)[2].ch, '好');
        assert_eq!(t.grid.row(0)[3].flags & WIDE_SPACER, WIDE_SPACER);
        assert_eq!(t.cursor.col, 4);
    }

    #[test]
    fn wide_char_selection_skips_continuation() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "你好!");
        t.selection_start(0, 0, SelMode::Simple);
        t.selection_update(0, 4);
        assert_eq!(t.selection_text(), Some("你好!".to_string()));
    }

    #[test]
    fn keypad_application_mode() {
        let mut t = Term::new(5, 1);
        feed(&mut t, "\x1b=");
        assert!(t.app_keypad);
        feed(&mut t, "\x1b>");
        assert!(!t.app_keypad);
    }

    #[test]
    fn combining_char_does_not_advance_cursor() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "e");
        let col_after_e = t.cursor.col;
        feed(&mut t, "\u{0301}");
        assert_eq!(t.cursor.col, col_after_e);
    }

    fn assert_no_half_pairs(t: &Term) {
        for r in 0..t.rows() {
            let row = t.grid.row(r);
            for c in 0..row.len() {
                if row[c].flags & WIDE_SPACER != 0 {
                    assert!(c > 0 && is_wide(row[c - 1].ch), "spacer órfão em {r},{c}: {}", t.dump());
                }
                if is_wide(row[c].ch) {
                    assert!(
                        c + 1 < row.len() && row[c + 1].flags & WIDE_SPACER != 0,
                        "metade larga sem par em {r},{c}: {}",
                        t.dump()
                    );
                }
            }
        }
    }

    fn row_text(t: &Term, r: usize) -> String {
        t.grid.row(r).iter().map(|c| c.ch).collect()
    }

    #[test]
    fn wide_overwrite_lead_clears_spacer() {
        let mut t = Term::new(6, 1);
        feed(&mut t, "你好\x1b[1Gx");
        assert_eq!(row_text(&t, 0), "x 好   ");
        assert_eq!(t.grid.row(0)[1].flags & WIDE_SPACER, 0);
        assert_no_half_pairs(&t);
    }

    #[test]
    fn wide_overwrite_spacer_with_ascii_clears_lead() {
        let mut t = Term::new(6, 1);
        feed(&mut t, "你好\x1b[2Gx");
        assert_eq!(row_text(&t, 0), " x好   ");
        assert_no_half_pairs(&t);
    }

    #[test]
    fn wide_overwrite_spacer_with_print_clears_lead() {
        let mut t = Term::new(6, 1);
        feed(&mut t, "你好\x1b[2Gé");
        assert_eq!(row_text(&t, 0), " é好   ");
        assert_no_half_pairs(&t);
    }

    #[test]
    fn wide_over_offset_wide() {
        let mut t = Term::new(6, 1);
        feed(&mut t, "你好\x1b[2G世");
        assert_eq!(t.grid.row(0)[0].ch, ' ');
        assert_eq!(t.grid.row(0)[1].ch, '世');
        assert_eq!(t.grid.row(0)[3].flags & WIDE_SPACER, 0);
        assert_no_half_pairs(&t);
    }

    #[test]
    fn wide_erase_chars_on_spacer() {
        let mut t = Term::new(6, 1);
        feed(&mut t, "你好\x1b[2G\x1b[1X");
        assert_eq!(row_text(&t, 0), "  好   ");
        assert_no_half_pairs(&t);

        let mut t = Term::new(6, 1);
        feed(&mut t, "你好\x1b[1G\x1b[3X");
        assert_eq!(row_text(&t, 0), "      ");
        assert_no_half_pairs(&t);
    }

    #[test]
    fn wide_insert_chars_on_spacer() {
        let mut t = Term::new(6, 1);
        feed(&mut t, "你ab\x1b[2G\x1b[1@");
        assert_eq!(row_text(&t, 0), "   ab ");
        assert_no_half_pairs(&t);
    }

    #[test]
    fn wide_insert_chars_pushes_pair_off_edge() {
        let mut t = Term::new(6, 1);
        feed(&mut t, "abc你\x1b[1G\x1b[2@");
        assert_eq!(row_text(&t, 0), "  abc ");
        assert_no_half_pairs(&t);
    }

    #[test]
    fn wide_delete_chars() {
        let mut t = Term::new(6, 1);
        feed(&mut t, "你好\x1b[2G\x1b[1P");
        assert_eq!(row_text(&t, 0), " 好    ");
        assert_no_half_pairs(&t);

        let mut t = Term::new(6, 1);
        feed(&mut t, "你好\x1b[1G\x1b[3P");
        assert_eq!(row_text(&t, 0), "      ");
        assert_no_half_pairs(&t);
    }

    #[test]
    fn wide_resize_cuts_pair() {
        let mut t = Term::new(6, 1);
        feed(&mut t, "ab你");
        t.resize(3, 2);
        assert_eq!(row_text(&t, 0), "ab ");
        assert_eq!(row_text(&t, 1), "   ");
        assert_no_half_pairs(&t);

        let mut t = Term::new(6, 2);
        feed(&mut t, "\x1b[?1049hab你");
        t.resize(3, 2);
        assert_eq!(row_text(&t, 0), "ab ");
        assert_no_half_pairs(&t);
    }

    #[test]
    fn reflow_wraps_long_line_on_narrower_cols() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "abcdefghij");
        t.resize(5, 2);
        assert_eq!(row_text(&t, 0), "abcde");
        assert_eq!(row_text(&t, 1), "fghij");
        assert_eq!(t.grid.row(0)[4].flags & WRAPLINE, WRAPLINE);
    }

    #[test]
    fn reflow_rejoins_wrapped_rows_on_wider_cols() {
        let mut t = Term::new(5, 2);
        feed(&mut t, "abcdefghij");
        t.resize(10, 1);
        assert_eq!(row_text(&t, 0), "abcdefghij");
    }

    #[test]
    fn reflow_preserves_cursor_mid_line() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "abcdefghij\x1b[1;3H");
        t.resize(5, 2);
        assert_eq!(t.cursor.row, 0);
        assert_eq!(t.cursor.col, 2);
        assert_eq!(row_text(&t, 0), "abcde");
    }

    #[test]
    fn reflow_rejoins_line_split_across_scrollback_and_grid() {
        let mut t = Term::new(5, 1);
        feed(&mut t, "abcde");
        feed(&mut t, "f\r\n");
        feed(&mut t, "g");
        t.resize(10, 2);
        assert_eq!(row_text(&t, 0), "abcdef    ");
        assert_eq!(row_text(&t, 1), "g         ");
    }
}
