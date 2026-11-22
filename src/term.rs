use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::cell::{flags, Cell, Color};
use crate::colors::{DEFAULT_BG, DEFAULT_FG};
use crate::config::Config;
use crate::grid::Grid;
use crate::parser::Perform;
use winit::window::CursorIcon;

const SCROLLBACK_LINES: usize = 10_000;
pub const SYNC_TIMEOUT: Duration = Duration::from_millis(150);
const OSC52_MAX_BYTES: usize = 1 << 20;
const MAX_INTERNED: usize = 1 << 16;
const MAX_LINK_LEN: usize = 2048;
const KB_STACK_MAX: usize = 16;
pub const KB_SUPPORTED: u8 = 0b1;
pub const WRAPLINE: u16 = 1 << 8;
pub const WIDE_SPACER: u16 = 1 << 9;
pub const PROMPT_MARK: u16 = 1 << 13;
pub const INPUT_MARK: u16 = 1 << 14;
pub const OUTPUT_MARK: u16 = 1 << 15;
const MARKS: u16 = PROMPT_MARK | INPUT_MARK | OUTPUT_MARK;
const NOTIFY_QUEUE_MAX: usize = 16;
const NOTIFY_TITLE_MAX: usize = 256;
const NOTIFY_BODY_MAX: usize = 4096;
const POINTER_STACK_MAX: usize = 16;
const ZWJ: char = '\u{200D}';

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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelPoint {
    pub line: usize,
    pub col: usize,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SearchMatch {
    pub start: SelPoint,
    pub end: SelPoint,
}
#[derive(Clone, Copy, Debug)]
struct Selection {
    anchor: SelPoint,
    head: SelPoint,
    mode: SelMode,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Charset {
    Ascii,
    DecGraphics,
    Uk,
}
#[derive(Clone, Copy)]
struct Pen {
    fg: Color,
    bg: Color,
    flags: u16,
    extra: u16,
}
impl Pen {
    const DEFAULT: Pen = Pen { fg: Color::Default, bg: Color::Default, flags: 0, extra: 0 };
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub urgency: Urgency,
    pub only_unfocused: bool,
}
impl Notification {
    fn new() -> Self {
        Self { title: String::new(), body: String::new(), urgency: Urgency::Normal, only_unfocused: false }
    }
}
#[derive(Clone, Copy)]
struct SavedCursor {
    cursor: Cursor,
    pen: Pen,
    charsets: [Charset; 2],
    gl: usize,
    origin: bool,
    autowrap: bool,
}
#[derive(Default)]
struct Extras {
    items: Vec<(u32, u32, u32)>,
    ids: HashMap<(u32, u32, u32), u16>,
}
impl Extras {
    fn id(&mut self, cluster: u32, link: u32, underline: u32) -> u16 {
        let key = (cluster, link, underline);
        if key == (0, 0, 0) {
            return 0;
        }
        if let Some(&id) = self.ids.get(&key) {
            return id;
        }
        if self.items.len() >= u16::MAX as usize {
            return 0;
        }
        self.items.push(key);
        let id = self.items.len() as u16;
        self.ids.insert(key, id);
        id
    }
    fn get(&self, id: u16) -> (u32, u32, u32) {
        if id == 0 {
            return (0, 0, 0);
        }
        self.items.get(id as usize - 1).copied().unwrap_or((0, 0, 0))
    }
}
fn encode_color(c: Color) -> u32 {
    match c {
        Color::Default => 0,
        Color::Indexed(i) => 1 << 24 | i as u32,
        Color::Rgb(r, g, b) => 2 << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32,
    }
}
fn decode_color(v: u32) -> Color {
    match v >> 24 {
        1 => Color::Indexed(v as u8),
        2 => Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
        _ => Color::Default,
    }
}
#[derive(Default)]
struct Interner {
    items: Vec<String>,
    ids: HashMap<String, u32>,
}
impl Interner {
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.ids.get(s) {
            return id;
        }
        if self.items.len() >= MAX_INTERNED {
            return 0;
        }
        self.items.push(s.to_string());
        let id = self.items.len() as u32;
        self.ids.insert(s.to_string(), id);
        id
    }
    fn get(&self, id: u32) -> Option<&str> {
        if id == 0 {
            return None;
        }
        self.items.get(id as usize - 1).map(String::as_str)
    }
}
pub struct Term {
    pub grid: Grid,
    pub cursor: Cursor,
    pen: Pen,
    scroll_top: usize,
    scroll_bottom: usize,
    wrap_pending: bool,
    saved: Option<SavedCursor>,
    main_grid: Option<Grid>,
    scrollback: VecDeque<Vec<Cell>>,
    max_scrollback: usize,
    popped: usize,
    selection: Option<Selection>,
    default_fg: [u8; 3],
    default_bg: [u8; 3],
    default_cursor_shape: CursorShape,
    default_cursor_blink: bool,
    clusters: Interner,
    links: Interner,
    extras: Extras,
    charsets: [Charset; 2],
    gl: usize,
    tabs: Vec<bool>,
    last_char: Option<char>,
    join_next: bool,
    insert_mode: bool,
    newline_mode: bool,
    origin_mode: bool,
    kb_stack: Vec<u8>,
    alt_kb: (u8, Vec<u8>),
    pub kb_flags: u8,
    pub cell_px: (u32, u32),
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
    pub cwd: Option<String>,
    pub bell: bool,
    pub sync_output: bool,
    pub sync_since: Option<Instant>,
    pub fg_override: Option<[u8; 3]>,
    pub bg_override: Option<[u8; 3]>,
    pub cursor_override: Option<[u8; 3]>,
    pub ansi_override: HashMap<u8, [u8; 3]>,
    palette_ansi: Box<[[u8; 3]; 256]>,
    pub clipboard_write: Option<(u8, String)>,
    pub osc52: bool,
    pub notifications: Vec<Notification>,
    notify_partial: Option<(String, Notification)>,
    pointer_stack: Vec<CursorIcon>,
}
fn default_tabs(cols: usize) -> Vec<bool> {
    (0..cols).map(|c| c > 0 && c % 8 == 0).collect()
}
impl Term {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
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
            clusters: Interner::default(),
            links: Interner::default(),
            extras: Extras::default(),
            charsets: [Charset::Ascii; 2],
            gl: 0,
            tabs: default_tabs(cols),
            last_char: None,
            join_next: false,
            insert_mode: false,
            newline_mode: false,
            origin_mode: false,
            kb_stack: Vec::new(),
            alt_kb: (0, Vec::new()),
            kb_flags: 0,
            cell_px: (0, 0),
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
            cwd: None,
            bell: false,
            sync_output: false,
            sync_since: None,
            fg_override: None,
            bg_override: None,
            cursor_override: None,
            ansi_override: HashMap::new(),
            palette_ansi: Box::new(crate::colors::Palette::default().ansi),
            clipboard_write: None,
            osc52: true,
            notifications: Vec::new(),
            notify_partial: None,
            pointer_stack: Vec::new(),
        }
    }
    pub fn apply_config(&mut self, cfg: &Config) {
        self.default_fg = cfg.palette.fg;
        self.default_bg = cfg.palette.bg;
        *self.palette_ansi = cfg.palette.ansi;
        self.max_scrollback = cfg.terminal.scrollback;
        self.trim_scrollback();
        self.osc52 = cfg.terminal.osc52;
        self.default_cursor_shape = cfg.cursor.shape;
        self.default_cursor_blink = cfg.cursor.blink;
        self.cursor_shape = cfg.cursor.shape;
        self.cursor_blink = cfg.cursor.blink;
    }
    fn trim_scrollback(&mut self) {
        while self.scrollback.len() > self.max_scrollback {
            self.scrollback.pop_front();
            self.popped += 1;
        }
        self.display_offset = self.display_offset.min(self.scrollback.len());
    }
    pub fn cols(&self) -> usize {
        self.grid.cols()
    }
    pub fn rows(&self) -> usize {
        self.grid.rows()
    }
    pub fn is_alt_screen(&self) -> bool {
        self.main_grid.is_some()
    }
    pub fn cluster(&self, cell: &Cell) -> Option<&str> {
        self.clusters.get(self.extras.get(cell.extra).0)
    }
    pub fn link(&self, cell: &Cell) -> Option<&str> {
        self.links.get(self.extras.get(cell.extra).1)
    }
    pub fn underline_color(&self, cell: &Cell) -> Option<Color> {
        match decode_color(self.extras.get(cell.extra).2) {
            Color::Default => None,
            c => Some(c),
        }
    }
    pub fn visible_row(&self, r: usize) -> &[Cell] {
        let sb = self.scrollback.len();
        let idx = sb.saturating_sub(self.display_offset) + r;
        if idx < sb {
            self.scrollback[idx].as_slice()
        } else {
            self.grid.row(idx - sb)
        }
    }
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
    pub fn abs_line(&self, row: usize) -> usize {
        self.popped + self.scrollback.len().saturating_sub(self.display_offset) + row
    }
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchMatch> {
        let smart = query.chars().any(char::is_uppercase);
        let norm = |c: char| if smart { c } else { fold_case(c) };
        let needle: Vec<char> = query.chars().map(norm).collect();
        let mut out = Vec::new();
        if needle.is_empty() || limit == 0 {
            return out;
        }
        let sb = self.scrollback.len();
        let first = if self.is_alt_screen() { self.popped + sb } else { self.popped };
        let end = self.popped + sb + self.rows();
        let mut chars: Vec<char> = Vec::new();
        let mut pos: Vec<SelPoint> = Vec::new();
        let mut line = first;
        while line < end {
            chars.clear();
            pos.clear();
            loop {
                let cells = self.line_cells(line).unwrap_or(&[]);
                for (col, c) in cells.iter().enumerate() {
                    if c.flags & WIDE_SPACER != 0 {
                        continue;
                    }
                    let p = SelPoint { line, col };
                    chars.push(norm(c.ch));
                    pos.push(p);
                    if let Some(extra) = self.cluster(c) {
                        for e in extra.chars() {
                            chars.push(norm(e));
                            pos.push(p);
                        }
                    }
                }
                line += 1;
                let wrapped = cells.last().is_some_and(|c| c.flags & WRAPLINE != 0);
                if !wrapped || line >= end {
                    break;
                }
            }
            let mut i = 0;
            while i + needle.len() <= chars.len() {
                if chars[i..i + needle.len()] == needle[..] {
                    out.push(SearchMatch { start: pos[i], end: pos[i + needle.len() - 1] });
                    if out.len() >= limit.saturating_mul(2) {
                        out.drain(..limit);
                    }
                    i += needle.len();
                } else {
                    i += 1;
                }
            }
        }
        if out.len() > limit {
            out.drain(..out.len() - limit);
        }
        out
    }
    pub fn scroll_to_line(&mut self, abs: usize) -> bool {
        if self.is_alt_screen() {
            return false;
        }
        let rows = self.rows();
        if abs >= self.abs_line(0) && abs <= self.abs_line(rows.saturating_sub(1)) {
            return false;
        }
        let sb = self.scrollback.len();
        let target = (self.popped + sb + rows / 2).saturating_sub(abs).min(sb);
        let changed = target != self.display_offset;
        self.display_offset = target;
        changed
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
        if col > 0 && self.visible_row(row).get(col).is_some_and(|c| c.flags & WIDE_SPACER != 0) {
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
    pub fn selection_select_all(&mut self) {
        let last = SelPoint {
            line: self.abs_line(self.rows().saturating_sub(1)),
            col: self.cols().saturating_sub(1),
        };
        self.selection = Some(Selection {
            anchor: SelPoint { line: self.popped, col: 0 },
            head: last,
            mode: SelMode::Line,
        });
    }
    pub fn selection_bounds(&self) -> Option<(SelPoint, SelPoint)> {
        let sel = self.selection?;
        if sel.mode == SelMode::Simple && sel.anchor == sel.head {
            return None;
        }
        let key = |p: SelPoint| (p.line, p.col);
        let (mut a, mut b) =
            if key(sel.anchor) <= key(sel.head) { (sel.anchor, sel.head) } else { (sel.head, sel.anchor) };
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
            let wrapped = line != b.line && cells.last().is_some_and(|c| c.flags & WRAPLINE != 0);
            let mut text = String::new();
            for c in cells[start..end].iter().filter(|c| c.flags & WIDE_SPACER == 0) {
                text.push(c.ch);
                if let Some(extra) = self.cluster(c) {
                    text.push_str(extra);
                }
            }
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
    pub fn link_at(&self, row: usize, col: usize) -> Option<String> {
        let cell = self.visible_row(row).get(col)?;
        self.link(cell).map(str::to_string)
    }
    pub fn url_at(&self, row: usize, col: usize) -> Option<String> {
        if let Some(l) = self.link_at(row, col) {
            return Some(l);
        }
        let mut first = row;
        while first > 0 && self.visible_row(first - 1).last().is_some_and(|c| c.flags & WRAPLINE != 0) {
            first -= 1;
        }
        let mut last = row;
        while last + 1 < self.rows() && self.visible_row(last).last().is_some_and(|c| c.flags & WRAPLINE != 0)
        {
            last += 1;
        }
        let mut chars: Vec<char> = Vec::new();
        let mut target = 0usize;
        for r in first..=last {
            for (c, cell) in self.visible_row(r).iter().enumerate() {
                if r == row && c == col {
                    target = chars.len();
                }
                if cell.flags & WIDE_SPACER != 0 {
                    continue;
                }
                chars.push(cell.ch);
            }
        }
        find_url(&chars, target)
    }
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        self.selection = None;
        let old_cols = self.cols();
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
        if cols != old_cols {
            let defaults = default_tabs(cols);
            self.tabs.resize(cols, false);
            let from = old_cols.min(cols);
            self.tabs[from..cols].copy_from_slice(&defaults[from..cols]);
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
            let wrapped = line.last().is_some_and(|c| c.flags & WRAPLINE != 0);
            physical.push((line, wrapped));
        }
        let last_used = (0..old_rows)
            .rev()
            .find(|&r| self.grid.row(r).iter().any(|c| *c != Cell::BLANK))
            .unwrap_or(0)
            .max(self.cursor.row);
        for r in 0..=last_used {
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
            logical.extend(line.into_iter().map(|mut c| {
                c.flags &= !WRAPLINE;
                c
            }));
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
        let mut g = Grid::new(cols, rows);
        for (i, row) in new_rows[grid_start..].iter().enumerate() {
            g.row_mut(i).copy_from_slice(row);
        }
        self.grid = g;

        for mut line in new_rows.drain(..grid_start) {
            let n = line.iter().rposition(|c| *c != Cell::BLANK).map_or(0, |p| p + 1);
            line.truncate(n);
            self.scrollback.push_back(line);
        }
        self.trim_scrollback();

        let cur_row = new_cursor.0.max(grid_start) - grid_start;
        self.cursor.row = cur_row.min(rows - 1);
        self.cursor.col = new_cursor.1.min(cols - 1);
        self.display_offset = 0;
    }
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
    fn move_to_origin(&mut self, row: usize, col: usize) {
        if self.origin_mode {
            let r = (self.scroll_top + row).min(self.scroll_bottom);
            self.move_to(r, col);
        } else {
            self.move_to(row, col);
        }
    }
    fn push_scrollback(&mut self, n: usize) {
        if self.max_scrollback == 0 {
            self.popped += n;
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
        self.saved = Some(SavedCursor {
            cursor: self.cursor,
            pen: self.pen,
            charsets: self.charsets,
            gl: self.gl,
            origin: self.origin_mode,
            autowrap: self.autowrap,
        });
    }
    fn restore_cursor(&mut self) {
        match self.saved {
            Some(s) => {
                self.pen = s.pen;
                self.charsets = s.charsets;
                self.gl = s.gl;
                self.origin_mode = s.origin;
                self.autowrap = s.autowrap;
                self.move_to(s.cursor.row, s.cursor.col);
            }
            None => {
                self.pen = Pen::DEFAULT;
                self.charsets = [Charset::Ascii; 2];
                self.gl = 0;
                self.origin_mode = false;
                self.move_to(0, 0);
            }
        }
    }
    pub fn reset(&mut self) {
        let (c, r) = (self.cols(), self.rows());
        let title = std::mem::take(&mut self.title);
        let cwd = self.cwd.take();
        let replies = std::mem::take(&mut self.replies);
        let notifications = std::mem::take(&mut self.notifications);
        let (fg, bg) = (self.default_fg, self.default_bg);
        let (shape, blink) = (self.default_cursor_shape, self.default_cursor_blink);
        let max_sb = self.max_scrollback;
        let osc52 = self.osc52;
        let cell_px = self.cell_px;
        let palette_ansi = std::mem::replace(&mut self.palette_ansi, Box::new([[0; 3]; 256]));
        *self = Term::new(c, r);
        self.osc52 = osc52;
        self.title = title;
        self.cwd = cwd;
        self.replies = replies;
        self.notifications = notifications;
        self.default_fg = fg;
        self.default_bg = bg;
        self.default_cursor_shape = shape;
        self.default_cursor_blink = blink;
        self.cursor_shape = shape;
        self.cursor_blink = blink;
        self.max_scrollback = max_sb;
        self.cell_px = cell_px;
        self.palette_ansi = palette_ansi;
    }
    fn erase_display(&mut self, mode: u16) {
        let b = self.blank();
        let (r, c) = (self.cursor.row, self.cursor.col);
        let (cols, rows) = (self.cols(), self.rows());
        match mode {
            0 => {
                self.clear_wide_edges(r, c, cols);
                self.fill_keep_marks(r, c, cols, b);
                for i in (r + 1)..rows {
                    self.grid.fill(i, 0, cols, b);
                }
            }
            1 => {
                for i in 0..r {
                    self.grid.fill(i, 0, cols, b);
                }
                self.clear_wide_edges(r, 0, (c + 1).min(cols));
                self.fill_keep_marks(r, 0, (c + 1).min(cols), b);
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
            0 => {
                self.clear_wide_edges(r, c, cols);
                self.fill_keep_marks(r, c, cols, b);
            }
            1 => {
                self.clear_wide_edges(r, 0, (c + 1).min(cols));
                self.fill_keep_marks(r, 0, (c + 1).min(cols), b);
            }
            2 => self.fill_keep_marks(r, 0, cols, b),
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
        self.wrap_pending = false;
    }
    fn delete_chars(&mut self, n: usize) {
        let b = self.blank();
        let (r, c, cols) = (self.cursor.row, self.cursor.col, self.cols());
        let n = n.min(cols - c);
        self.clear_wide_edges(r, c, c + n);
        let row = self.grid.row_mut(r);
        row.copy_within(c + n..cols, c);
        row[cols - n..].fill(b);
        self.wrap_pending = false;
    }
    fn erase_chars(&mut self, n: usize) {
        let b = self.blank();
        let (r, c, cols) = (self.cursor.row, self.cursor.col, self.cols());
        let end = (c + n).min(cols);
        self.clear_wide_edges(r, c, end);
        self.grid.fill(r, c, end, b);
    }
    fn screen_alignment(&mut self) {
        let e = Cell { ch: 'E', ..Cell::BLANK };
        for r in 0..self.rows() {
            self.grid.row_mut(r).fill(e);
        }
        self.scroll_top = 0;
        self.scroll_bottom = self.rows() - 1;
        self.origin_mode = false;
        self.move_to(0, 0);
    }
    fn tab_forward(&mut self, n: usize) {
        let cols = self.cols();
        let mut col = self.cursor.col;
        for _ in 0..n {
            let next = (col + 1..cols).find(|&c| self.tabs.get(c).copied().unwrap_or(false));
            col = next.unwrap_or(cols - 1);
            if col == cols - 1 {
                break;
            }
        }
        self.cursor.col = col;
        self.wrap_pending = false;
    }
    fn tab_backward(&mut self, n: usize) {
        let mut col = self.cursor.col;
        for _ in 0..n {
            if col == 0 {
                break;
            }
            col = (0..col).rev().find(|&c| self.tabs.get(c).copied().unwrap_or(false)).unwrap_or(0);
        }
        self.cursor.col = col;
        self.wrap_pending = false;
    }
    fn set_alt_screen(&mut self, on: bool, save: bool) {
        if on == self.main_grid.is_some() {
            return;
        }
        self.selection = None;
        let kb = (self.kb_flags, std::mem::take(&mut self.kb_stack));
        let (flags, stack) = std::mem::replace(&mut self.alt_kb, kb);
        self.kb_flags = flags;
        self.kb_stack = stack;
        if on {
            self.display_offset = 0;
            if save {
                self.save_cursor();
            }
            let blank = Grid::new(self.cols(), self.rows());
            self.main_grid = Some(std::mem::replace(&mut self.grid, blank));
        } else {
            if let Some(g) = self.main_grid.take() {
                self.grid = g;
            }
            if save {
                self.restore_cursor();
            }
        }
    }
    fn set_mouse(&mut self, mode: MouseMode, on: bool) {
        if on {
            self.mouse_mode = mode;
        } else if self.mouse_mode == mode {
            self.mouse_mode = MouseMode::Off;
        }
    }
    fn set_ansi_modes(&mut self, params: &[u16], on: bool) {
        for &m in params {
            match m {
                4 => self.insert_mode = on,
                20 => self.newline_mode = on,
                _ => {}
            }
        }
    }
    fn set_modes(&mut self, params: &[u16], on: bool) {
        for &m in params {
            match m {
                1 => self.app_cursor_keys = on,
                6 => {
                    self.origin_mode = on;
                    self.move_to_origin(0, 0);
                }
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
                1048 => {
                    if on {
                        self.save_cursor();
                    } else {
                        self.restore_cursor();
                    }
                }
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
            6 => Some(self.origin_mode),
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
    fn decrqm_ansi(&mut self, mode: u16) {
        let ps = match mode {
            4 => {
                if self.insert_mode {
                    1
                } else {
                    2
                }
            }
            20 => {
                if self.newline_mode {
                    1
                } else {
                    2
                }
            }
            _ => 0,
        };
        self.reply(&format!("\x1b[{};{}$y", mode, ps));
    }
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
    fn kitty_keyboard(&mut self, private: u8, params: &[u16]) {
        let first = params.first().copied().unwrap_or(0);
        match private {
            b'?' => {
                let s = format!("\x1b[?{}u", self.kb_flags);
                self.reply(&s);
            }
            b'>' => {
                if self.kb_stack.len() >= KB_STACK_MAX {
                    self.kb_stack.remove(0);
                }
                self.kb_stack.push(self.kb_flags);
                self.kb_flags = (first as u8) & KB_SUPPORTED;
            }
            b'<' => {
                let n = (first as usize).max(1);
                for _ in 0..n {
                    match self.kb_stack.pop() {
                        Some(f) => self.kb_flags = f,
                        None => {
                            self.kb_flags = 0;
                            break;
                        }
                    }
                }
            }
            b'=' => {
                let f = (first as u8) & KB_SUPPORTED;
                match params.get(1).copied().unwrap_or(1) {
                    2 => self.kb_flags |= f,
                    3 => self.kb_flags &= !f,
                    _ => self.kb_flags = f,
                }
            }
            _ => {}
        }
    }
    fn sgr(&mut self, params: &[u16], subs: &[bool]) {
        if params.is_empty() {
            self.reset_pen_attrs();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => self.reset_pen_attrs(),
                1 => self.pen.flags |= flags::BOLD,
                2 => self.pen.flags |= flags::DIM,
                3 => self.pen.flags |= flags::ITALIC,
                4 => {
                    let style = if subs.get(i + 1) == Some(&true) {
                        i += 1;
                        params[i]
                    } else {
                        1
                    };
                    self.set_underline(style);
                }
                21 => self.set_underline(2),
                5 | 6 => self.pen.flags |= flags::BLINK,
                7 => self.pen.flags |= flags::INVERSE,
                8 => self.pen.flags |= flags::HIDDEN,
                9 => self.pen.flags |= flags::STRIKE,
                22 => self.pen.flags &= !(flags::BOLD | flags::DIM),
                23 => self.pen.flags &= !flags::ITALIC,
                24 => self.set_underline(0),
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
                    if let Some(c) = extended_color(params, subs, &mut i) {
                        self.set_underline_color(c);
                    }
                }
                59 => self.set_underline_color(Color::Default),
                90..=97 => self.pen.fg = Color::Indexed((p - 90 + 8) as u8),
                100..=107 => self.pen.bg = Color::Indexed((p - 100 + 8) as u8),
                _ => {}
            }
            i += 1;
        }
    }
    fn reset_pen_attrs(&mut self) {
        let extra = self.pen.extra;
        self.pen = Pen::DEFAULT;
        self.pen.extra = extra;
        self.set_underline_color(Color::Default);
    }
    fn set_underline(&mut self, style: u16) {
        self.pen.flags &= !(flags::UNDERLINE | flags::UNDERLINE_STYLE);
        self.pen.flags |= match style {
            0 => 0,
            2 => flags::UNDERLINE | flags::UNDERLINE_DOUBLE,
            3 => flags::UNDERLINE | flags::UNDERLINE_CURLY,
            4 => flags::UNDERLINE | flags::UNDERLINE_DOTTED,
            5 => flags::UNDERLINE | flags::UNDERLINE_DASHED,
            _ => flags::UNDERLINE,
        };
    }
    fn set_underline_color(&mut self, c: Color) {
        let (_, link, _) = self.extras.get(self.pen.extra);
        self.pen.extra = self.extras.id(0, link, encode_color(c));
    }
    fn prev_cell_pos(&self) -> Option<(usize, usize)> {
        let row = self.cursor.row;
        let mut col = if self.wrap_pending { self.cursor.col } else { self.cursor.col.checked_sub(1)? };
        if col > 0 && self.grid.row(row)[col].flags & WIDE_SPACER != 0 {
            col -= 1;
        }
        Some((row, col))
    }
    fn attach_to_prev(&mut self, c: char) -> bool {
        let Some((r, col)) = self.prev_cell_pos() else {
            return false;
        };
        let cell = self.grid.row(r)[col];
        let (cluster, link, underline) = self.extras.get(cell.extra);
        let mut s = self.clusters.get(cluster).unwrap_or("").to_string();
        if s.chars().count() >= 16 {
            return true;
        }
        s.push(c);
        let id = self.clusters.intern(&s);
        if id != 0 {
            let extra = self.extras.id(id, link, underline);
            if extra != 0 {
                self.grid.row_mut(r)[col].extra = extra;
            }
        }
        true
    }
    fn prev_is_wide(&self) -> bool {
        self.prev_cell_pos().is_some_and(|(r, c)| is_wide(self.grid.row(r)[c].ch))
    }
    fn map_charset(&self, c: char) -> char {
        match self.charsets[self.gl] {
            Charset::Ascii => c,
            Charset::Uk => {
                if c == '#' {
                    '£'
                } else {
                    c
                }
            }
            Charset::DecGraphics => dec_graphics(c),
        }
    }
    fn shell_mark(&mut self, rest: &[u8]) {
        let mark = match rest.first() {
            Some(b'A' | b'P' | b'N') => PROMPT_MARK,
            Some(b'B') => INPUT_MARK,
            Some(b'C') => OUTPUT_MARK,
            _ => return,
        };
        let (r, c) = (self.cursor.row, self.cursor.col);
        self.grid.row_mut(r)[c].flags |= mark;
    }
    fn fill_keep_marks(&mut self, r: usize, from: usize, to: usize, b: Cell) {
        for cell in &mut self.grid.row_mut(r)[from..to] {
            *cell = Cell { flags: b.flags | (cell.flags & MARKS), ..b };
        }
    }
    fn push_notification(&mut self, mut n: Notification) {
        if n.title.trim().is_empty() {
            n.title = std::mem::take(&mut n.body);
        }
        if n.title.trim().is_empty() || self.notifications.len() >= NOTIFY_QUEUE_MAX {
            return;
        }
        truncate_chars(&mut n.title, NOTIFY_TITLE_MAX);
        truncate_chars(&mut n.body, NOTIFY_BODY_MAX);
        self.notifications.push(n);
    }
    fn osc9(&mut self, rest: &[u8]) {
        let head = rest.split(|&b| b == b';').next().unwrap_or(&[]);
        if !head.is_empty() && head.iter().all(u8::is_ascii_digit) {
            return;
        }
        let mut n = Notification::new();
        n.title = String::from_utf8_lossy(rest).into_owned();
        self.push_notification(n);
    }
    fn osc777(&mut self, rest: &[u8]) {
        let mut it = rest.splitn(3, |&b| b == b';');
        if it.next() != Some(b"notify") {
            return;
        }
        let mut n = Notification::new();
        n.title = String::from_utf8_lossy(it.next().unwrap_or(&[])).into_owned();
        n.body = String::from_utf8_lossy(it.next().unwrap_or(&[])).into_owned();
        self.push_notification(n);
    }
    fn osc99(&mut self, rest: &[u8]) {
        let mut it = rest.splitn(2, |&b| b == b';');
        let meta = it.next().unwrap_or(&[]);
        let payload = it.next().unwrap_or(&[]);
        let (mut id, mut done, mut part, mut base64) = (String::new(), true, &b"title"[..], false);
        let (mut urgency, mut when) = (None, None);
        for kv in meta.split(|&b| b == b':') {
            let mut kv = kv.splitn(2, |&b| b == b'=');
            let key = kv.next().unwrap_or(&[]);
            let val = kv.next().unwrap_or(&[]);
            match key {
                b"i" => id = String::from_utf8_lossy(val).into_owned(),
                b"d" => done = val != b"0",
                b"p" => part = val,
                b"e" => base64 = val == b"1",
                b"u" => {
                    urgency = match val {
                        b"0" => Some(Urgency::Low),
                        b"2" => Some(Urgency::Critical),
                        _ => Some(Urgency::Normal),
                    }
                }
                b"o" => when = Some(val != b"always"),
                _ => {}
            }
        }
        let mut n = match self.notify_partial.take() {
            Some((pid, n)) if pid == id => n,
            _ => Notification::new(),
        };
        if let Some(u) = urgency {
            n.urgency = u;
        }
        if let Some(w) = when {
            n.only_unfocused = w;
        }
        let text = if base64 {
            base64_decode(payload).map(|b| String::from_utf8_lossy(&b).into_owned())
        } else {
            Some(String::from_utf8_lossy(payload).into_owned())
        };
        let target = match part {
            b"title" => Some(&mut n.title),
            b"body" => Some(&mut n.body),
            b"?" => return,
            _ => None,
        };
        if let (Some(target), Some(text)) = (target, text) {
            if target.len() + text.len() <= NOTIFY_BODY_MAX * 4 {
                target.push_str(&text);
            }
        }
        if done {
            self.push_notification(n);
        } else {
            self.notify_partial = Some((id, n));
        }
    }
    pub fn pointer_shape(&self) -> Option<CursorIcon> {
        self.pointer_stack.last().copied()
    }
    fn osc22(&mut self, rest: &[u8]) {
        let text = String::from_utf8_lossy(rest);
        let (op, names) = match text.chars().next() {
            Some(c @ ('=' | '>' | '<' | '?')) => (c, &text[1..]),
            _ => ('=', &text[..]),
        };
        match op {
            '?' => {
                let answers: Vec<&str> = names
                    .split(',')
                    .map(|n| match n {
                        "__current__" => self.pointer_shape().map_or("0", |i| i.name()),
                        "__default__" => CursorIcon::Text.name(),
                        "__grabbed__" => CursorIcon::Default.name(),
                        n if pointer_icon(n).is_some() => "1",
                        _ => "0",
                    })
                    .collect();
                let s = format!("\x1b]22;{}\x1b\\", answers.join(","));
                self.reply(&s);
            }
            '<' => {
                self.pointer_stack.pop();
            }
            '=' if names.is_empty() => self.pointer_stack.clear(),
            _ => {
                let Some(icon) = names.split(',').find_map(pointer_icon) else { return };
                if op == '=' && !self.pointer_stack.is_empty() {
                    self.pointer_stack.pop();
                } else if self.pointer_stack.len() >= POINTER_STACK_MAX {
                    self.pointer_stack.remove(0);
                }
                self.pointer_stack.push(icon);
            }
        }
    }
    fn prompt_lines(&self) -> Vec<usize> {
        if self.is_alt_screen() {
            return Vec::new();
        }
        let end = self.popped + self.scrollback.len() + self.rows();
        (self.popped..end).filter(|&l| self.line_has_mark(l, PROMPT_MARK)).collect()
    }
    fn line_has_mark(&self, line: usize, mark: u16) -> bool {
        self.line_cells(line).is_some_and(|cells| cells.iter().any(|c| c.flags & mark != 0))
    }
    fn line_is_blank(&self, line: usize) -> bool {
        self.line_cells(line).is_none_or(|cells| {
            cells.iter().all(|c| (c.ch == ' ' || c.flags & WIDE_SPACER != 0) && self.cluster(c).is_none())
        })
    }
    fn scroll_line_to_top(&mut self, abs: usize) -> bool {
        let sb = self.scrollback.len();
        let target = (self.popped + sb).saturating_sub(abs).min(sb);
        let changed = target != self.display_offset;
        self.display_offset = target;
        changed
    }
    pub fn jump_to_prompt(&mut self, up: bool) -> bool {
        let top = self.abs_line(0);
        let prompts = self.prompt_lines();
        let target =
            if up { prompts.iter().rev().find(|&&l| l < top) } else { prompts.iter().find(|&&l| l > top) };
        match target {
            Some(&l) => self.scroll_line_to_top(l),
            None if !up && !prompts.is_empty() => self.scroll_to_bottom(),
            None => false,
        }
    }
    pub fn command_outputs(&self) -> Vec<(usize, usize)> {
        let prompts = self.prompt_lines();
        let cursor_line = self.popped + self.scrollback.len() + self.cursor.row;
        let mut out = Vec::new();
        for (i, &p) in prompts.iter().enumerate() {
            let next = prompts.get(i + 1).copied();
            let last = next.map_or(cursor_line, |n| n.saturating_sub(1));
            let start = (p + 1..=last)
                .find(|&l| self.line_has_mark(l, OUTPUT_MARK))
                .or_else(|| (p..=last).find(|&l| self.line_has_mark(l, INPUT_MARK)).map(|l| l + 1))
                .unwrap_or(p + 1);
            let mut end = last;
            while end > start && self.line_is_blank(end) {
                end -= 1;
            }
            if start <= end && !self.line_is_blank(end) {
                out.push((start, end));
            }
        }
        out
    }
    pub fn select_command_output(&mut self) -> Option<String> {
        let outputs = self.command_outputs();
        let bottom = self.abs_line(self.rows().saturating_sub(1));
        let current = self.selection_bounds().map(|(a, b)| (a.line, b.line));
        let mut idx = outputs.iter().rposition(|&(s, _)| s <= bottom)?;
        if current == Some(outputs[idx]) {
            idx = idx.checked_sub(1)?;
        }
        let (start, end) = outputs[idx];
        self.selection = Some(Selection {
            anchor: SelPoint { line: start, col: 0 },
            head: SelPoint { line: end, col: usize::MAX },
            mode: SelMode::Line,
        });
        if start < self.abs_line(0) || start > bottom {
            self.scroll_line_to_top(start);
        }
        self.selection_text()
    }
    fn print_slow(&self) -> bool {
        self.charsets[self.gl] != Charset::Ascii || self.insert_mode || self.join_next
    }
}
fn truncate_chars(s: &mut String, max: usize) {
    if let Some((i, _)) = s.char_indices().nth(max) {
        s.truncate(i);
    }
}
fn pointer_icon(name: &str) -> Option<CursorIcon> {
    let name = name.trim();
    let css = match name {
        "left_ptr" | "arrow" | "top_left_arrow" => "default",
        "xterm" | "ibeam" => "text",
        "hand" | "hand1" | "hand2" | "pointing_hand" => "pointer",
        "watch" => "wait",
        "left_ptr_watch" => "progress",
        "question_arrow" | "whats_this" => "help",
        "cross" | "tcross" => "crosshair",
        "fleur" | "size_all" => "move",
        "sb_h_double_arrow" | "h_double_arrow" => "ew-resize",
        "sb_v_double_arrow" | "v_double_arrow" => "ns-resize",
        "top_side" => "n-resize",
        "bottom_side" => "s-resize",
        "left_side" => "w-resize",
        "right_side" => "e-resize",
        "top_left_corner" => "nw-resize",
        "top_right_corner" => "ne-resize",
        "bottom_left_corner" => "sw-resize",
        "bottom_right_corner" => "se-resize",
        "crossed_circle" | "forbidden" => "not-allowed",
        n => n,
    };
    css.parse().ok()
}
fn is_wide(c: char) -> bool {
    unicode_width::UnicodeWidthChar::width(c) == Some(2)
}
fn is_emoji_modifier(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}
fn dec_graphics(c: char) -> char {
    match c {
        '`' => '◆',
        'a' => '▒',
        'b' => '␉',
        'c' => '␌',
        'd' => '␍',
        'e' => '␊',
        'f' => '°',
        'g' => '±',
        'h' => '␤',
        'i' => '␋',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => c,
    }
}
fn charset_for(b: u8) -> Charset {
    match b {
        b'0' => Charset::DecGraphics,
        b'A' => Charset::Uk,
        _ => Charset::Ascii,
    }
}
const URL_SCHEMES: [&str; 6] = ["https://", "http://", "file://", "ftp://", "mailto:", "ssh://"];
fn url_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, '<' | '>' | '"' | '`' | '{' | '}' | '|' | '\\' | '^')
}
fn find_url(chars: &[char], target: usize) -> Option<String> {
    if target >= chars.len() || !url_char(chars[target]) {
        return None;
    }
    let mut s = target;
    while s > 0 && url_char(chars[s - 1]) {
        s -= 1;
    }
    let mut e = target + 1;
    while e < chars.len() && url_char(chars[e]) {
        e += 1;
    }
    let word: String = chars[s..e].iter().collect();
    let target_byte: usize = chars[s..target].iter().map(|c| c.len_utf8()).sum();
    let mut best: Option<(usize, usize)> = None;
    for scheme in URL_SCHEMES {
        let mut from = 0;
        while let Some(pos) = word[from..].find(scheme) {
            let start = from + pos;
            if start <= target_byte && best.is_none_or(|(b, _)| start > b) {
                best = Some((start, scheme.len()));
            }
            from = start + scheme.len();
        }
    }
    let (start, scheme_len) = best?;
    let mut url = &word[start..];
    loop {
        let trimmed = url.trim_end_matches(['.', ',', ';', ':', '!', '?', '\'']);
        let unbalanced_paren =
            trimmed.ends_with(')') && trimmed.matches('(').count() < trimmed.matches(')').count();
        let unbalanced_bracket = trimmed.ends_with(']') && !trimmed.contains('[');
        let trimmed =
            if unbalanced_paren || unbalanced_bracket { &trimmed[..trimmed.len() - 1] } else { trimmed };
        if trimmed.len() == url.len() {
            break;
        }
        url = trimmed;
    }
    if url.len() <= scheme_len || start + url.len() <= target_byte {
        return None;
    }
    Some(url.to_string())
}
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
fn parse_osc7(rest: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(rest).ok()?;
    let after = s.strip_prefix("file://")?;
    let slash = after.find('/')?;
    let path = percent_decode(&after[slash..]);
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
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
                row[c] = Cell { ch: ' ', extra: 0, ..row[c] };
            }
        }
    }
}
fn fold_case(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}
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
        let c = self.map_charset(c);
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        let join = std::mem::take(&mut self.join_next);
        if w == 0 || (join && w == 2) || (is_emoji_modifier(c) && self.prev_is_wide()) {
            if self.attach_to_prev(c) {
                self.join_next = c == ZWJ;
            }
            return;
        }
        if self.wrap_pending {
            if self.autowrap {
                let last = self.cols() - 1;
                self.grid.row_mut(self.cursor.row)[last].flags |= WRAPLINE;
                self.cursor.col = 0;
                self.linefeed();
            }
            self.wrap_pending = false;
        }
        let cols = self.cols();
        if w == 2 && self.cursor.col + 2 > cols {
            if self.autowrap {
                let last = cols - 1;
                self.grid.row_mut(self.cursor.row)[last].flags |= WRAPLINE;
                self.cursor.col = 0;
                self.linefeed();
            } else {
                self.cursor.col = cols.saturating_sub(2);
            }
        }
        if self.insert_mode {
            self.insert_chars(w);
        }
        let col = self.cursor.col;
        self.clear_wide_edges(self.cursor.row, col, (col + w).min(cols));
        let cell =
            Cell { ch: c, fg: self.pen.fg, bg: self.pen.bg, flags: self.pen.flags, extra: self.pen.extra };
        let slot = &mut self.grid.row_mut(self.cursor.row)[self.cursor.col];
        *slot = Cell { flags: cell.flags | (slot.flags & MARKS), ..cell };
        if w == 2 && self.cursor.col + 1 < cols {
            let spacer = Cell { ch: ' ', flags: self.pen.flags | WIDE_SPACER, ..cell };
            self.grid.row_mut(self.cursor.row)[self.cursor.col + 1] = spacer;
        }
        self.last_char = Some(c);
        let advance = if w == 2 { 2 } else { 1 };
        if self.cursor.col + advance < cols {
            self.cursor.col += advance;
        } else {
            self.cursor.col = cols - 1;
            self.wrap_pending = self.autowrap;
        }
    }
    fn print_ascii(&mut self, mut s: &[u8]) {
        if self.print_slow() {
            for &b in s {
                self.print(b as char);
            }
            return;
        }
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
            let (fg, bg, fl, extra) = (self.pen.fg, self.pen.bg, self.pen.flags, self.pen.extra);
            let row = self.grid.row_mut(self.cursor.row);
            for (cell, &b) in row[col..col + n].iter_mut().zip(&s[..n]) {
                *cell = Cell { ch: b as char, fg, bg, flags: fl | (cell.flags & MARKS), extra };
            }
            self.last_char = Some(s[n - 1] as char);
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
            0x07 => self.bell = true,
            0x08 => {
                self.cursor.col = self.cursor.col.saturating_sub(1);
                self.wrap_pending = false;
            }
            0x09 => self.tab_forward(1),
            0x0A..=0x0C => {
                self.linefeed();
                if self.newline_mode {
                    self.cursor.col = 0;
                }
            }
            0x0D => {
                self.cursor.col = 0;
                self.wrap_pending = false;
            }
            0x0E => self.gl = 1,
            0x0F => self.gl = 0,
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
                ([b'$'], None, b'p') => {
                    self.decrqm_ansi(params.first().copied().unwrap_or(0));
                }
                ([b'!'], None, b'p') => {
                    let keep = (self.default_fg, self.default_bg);
                    self.pen = Pen::DEFAULT;
                    self.insert_mode = false;
                    self.origin_mode = false;
                    self.autowrap = true;
                    self.cursor_visible = true;
                    self.app_cursor_keys = false;
                    self.app_keypad = false;
                    self.scroll_top = 0;
                    self.scroll_bottom = self.rows() - 1;
                    self.charsets = [Charset::Ascii; 2];
                    self.gl = 0;
                    self.saved = None;
                    (self.default_fg, self.default_bg) = keep;
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
            (Some(b'>'), b'c') if first == 0 => self.reply("\x1b[>1;10;0c"),
            (Some(b'>'), b'q') => {
                self.reply(concat!("\x1bP>|aeternus ", env!("CARGO_PKG_VERSION"), "\x1b\\"))
            }
            (Some(pv @ (b'?' | b'>' | b'<' | b'=')), b'u') => self.kitty_keyboard(pv, params),
            (Some(_), _) => {}
            (None, b'A') => {
                let top = if row >= self.scroll_top { self.scroll_top } else { 0 };
                self.move_to(row.saturating_sub(p(0, 1)).max(top), col);
            }
            (None, b'B') | (None, b'e') => {
                let bottom = if row <= self.scroll_bottom { self.scroll_bottom } else { self.rows() - 1 };
                self.move_to((row + p(0, 1)).min(bottom), col);
            }
            (None, b'C') | (None, b'a') => self.move_to(row, col + p(0, 1)),
            (None, b'D') => self.move_to(row, col.saturating_sub(p(0, 1))),
            (None, b'E') => {
                let bottom = if row <= self.scroll_bottom { self.scroll_bottom } else { self.rows() - 1 };
                self.move_to((row + p(0, 1)).min(bottom), 0);
            }
            (None, b'F') => {
                let top = if row >= self.scroll_top { self.scroll_top } else { 0 };
                self.move_to(row.saturating_sub(p(0, 1)).max(top), 0);
            }
            (None, b'G') | (None, b'`') => self.move_to(row, p(0, 1) - 1),
            (None, b'H') | (None, b'f') => self.move_to_origin(p(0, 1) - 1, p(1, 1) - 1),
            (None, b'd') => {
                let c = self.cursor.col;
                self.move_to_origin(p(0, 1) - 1, c);
            }
            (None, b'I') => self.tab_forward(p(0, 1)),
            (None, b'Z') => self.tab_backward(p(0, 1)),
            (None, b'g') => match first {
                0 => {
                    if let Some(t) = self.tabs.get_mut(col) {
                        *t = false;
                    }
                }
                3 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            (None, b'b') => {
                if let Some(c) = self.last_char {
                    let n = p(0, 1).min(self.cols() * self.rows());
                    for _ in 0..n {
                        self.print(c);
                    }
                }
            }
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
            (None, b'h') => self.set_ansi_modes(params, true),
            (None, b'l') => self.set_ansi_modes(params, false),
            (None, b'm') => self.sgr(params, subs),
            (None, b'r') => {
                let top = p(0, 1) - 1;
                let bottom = p(1, self.rows()).min(self.rows()) - 1;
                if top < bottom {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                    self.move_to_origin(0, 0);
                }
            }
            (None, b's') => self.save_cursor(),
            (None, b'u') => self.restore_cursor(),
            (None, b'c') if first == 0 => self.reply("\x1b[?62;22c"),
            (None, b'n') => match first {
                5 => self.reply("\x1b[0n"),
                6 => {
                    let r = if self.origin_mode { row.saturating_sub(self.scroll_top) } else { row };
                    let s = format!("\x1b[{};{}R", r + 1, col + 1);
                    self.reply(&s);
                }
                _ => {}
            },
            (None, b't') => match first {
                14 => {
                    let (cw, ch) = self.cell_px;
                    let s = format!("\x1b[4;{};{}t", ch as usize * self.rows(), cw as usize * self.cols());
                    self.reply(&s);
                }
                16 => {
                    let (cw, ch) = self.cell_px;
                    let s = format!("\x1b[6;{};{}t", ch, cw);
                    self.reply(&s);
                }
                18 => {
                    let s = format!("\x1b[8;{};{}t", self.rows(), self.cols());
                    self.reply(&s);
                }
                _ => {}
            },
            _ => {}
        }
    }
    fn esc(&mut self, inter: &[u8], byte: u8) {
        match inter {
            [] => match byte {
                b'7' => self.save_cursor(),
                b'8' => self.restore_cursor(),
                b'D' => self.linefeed(),
                b'E' => {
                    self.linefeed();
                    self.cursor.col = 0;
                }
                b'H' => {
                    let c = self.cursor.col;
                    if let Some(t) = self.tabs.get_mut(c) {
                        *t = true;
                    }
                }
                b'M' => self.reverse_index(),
                b'=' => self.app_keypad = true,
                b'>' => self.app_keypad = false,
                b'Z' => self.reply("\x1b[?62;22c"),
                b'c' => self.reset(),
                b'n' => self.gl = 0,
                _ => {}
            },
            [b'('] => self.charsets[0] = charset_for(byte),
            [b')'] => self.charsets[1] = charset_for(byte),
            [b'#'] if byte == b'8' => self.screen_alignment(),
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
                while let (Some(idx), Some(spec)) = (it.next(), it.next()) {
                    let Ok(i) = std::str::from_utf8(idx).unwrap_or("").parse::<u8>() else {
                        continue;
                    };
                    if spec == b"?" {
                        let c = self.ansi_override.get(&i).copied().unwrap_or(self.palette_ansi[i as usize]);
                        let s = format!(
                            "\x1b]4;{};rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}\x1b\\",
                            i, c[0], c[0], c[1], c[1], c[2], c[2]
                        );
                        self.reply(&s);
                        continue;
                    }
                    if let Some(c) = parse_color_spec(spec) {
                        self.ansi_override.insert(i, c);
                    }
                }
            }
            b"7" => {
                if let Some(path) = parse_osc7(rest) {
                    self.cwd = Some(path);
                }
            }
            b"8" => {
                let mut it = rest.splitn(2, |&b| b == b';');
                let _params = it.next();
                let uri = it.next().unwrap_or(&[]);
                let (_, _, underline) = self.extras.get(self.pen.extra);
                let link = if uri.is_empty() || uri.len() > MAX_LINK_LEN {
                    0
                } else {
                    self.links.intern(&String::from_utf8_lossy(uri))
                };
                self.pen.extra = self.extras.id(0, link, underline);
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
                        c[0],
                        c[0],
                        c[1],
                        c[1],
                        c[2],
                        c[2]
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
            b"133" => self.shell_mark(rest),
            b"9" => self.osc9(rest),
            b"99" => self.osc99(rest),
            b"777" => self.osc777(rest),
            b"22" => self.osc22(rest),
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
    fn spans(t: &Term, q: &str) -> Vec<(usize, usize, usize, usize)> {
        t.search(q, usize::MAX).iter().map(|m| (m.start.line, m.start.col, m.end.line, m.end.col)).collect()
    }
    #[test]
    fn search_finds_matches_in_scrollback_and_screen() {
        let mut t = Term::new(20, 3);
        feed(&mut t, "foo one\r\nbar\r\nfoo two\r\nbaz\r\nfoo three");
        assert_eq!(spans(&t, "foo"), vec![(0, 0, 0, 2), (2, 0, 2, 2), (4, 0, 4, 2)]);
        assert!(spans(&t, "nothing").is_empty());
        assert!(spans(&t, "").is_empty());
    }
    #[test]
    fn search_smart_case() {
        let mut t = Term::new(20, 3);
        feed(&mut t, "Hello hello HELLO");
        assert_eq!(spans(&t, "hello").len(), 3);
        assert_eq!(spans(&t, "Hello"), vec![(0, 0, 0, 4)]);
        assert_eq!(spans(&t, "HELLO"), vec![(0, 12, 0, 16)]);
    }
    #[test]
    fn search_spans_wrapped_rows_but_not_line_breaks() {
        let mut t = Term::new(5, 4);
        feed(&mut t, "abcdefgh\r\nxy\r\nzw");
        assert_eq!(spans(&t, "def"), vec![(0, 3, 1, 0)]);
        assert!(spans(&t, "yz").is_empty());
    }
    #[test]
    fn search_skips_wide_spacers() {
        let mut t = Term::new(10, 2);
        feed(&mut t, "你好吗");
        assert_eq!(spans(&t, "好吗"), vec![(0, 2, 0, 4)]);
    }
    #[test]
    fn search_keeps_most_recent_when_limited() {
        let mut t = Term::new(10, 3);
        feed(&mut t, "a\r\na\r\na\r\na\r\na");
        let m = t.search("a", 2);
        assert_eq!(m.len(), 2);
        assert_eq!((m[0].start.line, m[1].start.line), (3, 4));
    }
    #[test]
    fn search_alt_screen_only_searches_screen() {
        let mut t = Term::new(10, 2);
        feed(&mut t, "x1\r\nx2\r\nx3\r\n\x1b[?1049h\x1b[Hx4");
        let m = t.search("x", usize::MAX);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].start.line, t.abs_line(0));
    }
    #[test]
    fn scroll_to_line_brings_match_into_view() {
        let mut t = Term::new(10, 3);
        for i in 0..20 {
            feed(&mut t, &format!("line{i}\r\n"));
        }
        let m = t.search("line2", usize::MAX)[0];
        assert!(t.scroll_to_line(m.start.line));
        assert!(t.abs_line(0) <= m.start.line && m.start.line <= t.abs_line(t.rows() - 1));
        assert!(!t.scroll_to_line(m.start.line));
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
    fn sgr_58_59_do_not_break_params() {
        let mut t = Term::new(5, 1);
        feed(&mut t, "\x1b[58:2::1:2:3;59;1mx");
        let cell = t.grid.row(0)[0];
        assert_eq!(cell.fg, Color::Default);
        assert_eq!(cell.bg, Color::Default);
        assert_eq!(cell.flags & flags::BOLD, flags::BOLD);
        assert_eq!(t.underline_color(&cell), None);
    }
    #[test]
    fn sgr_58_sets_underline_color() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "\x1b[4;58:2::1:2:3ma\x1b[58;5;9mb\x1b[59mc\x1b[58:5:4md\x1b[0me");
        let row = t.grid.row(0);
        assert_eq!(t.underline_color(&row[0]), Some(Color::Rgb(1, 2, 3)));
        assert_eq!(t.underline_color(&row[1]), Some(Color::Indexed(9)));
        assert_eq!(t.underline_color(&row[2]), None);
        assert_eq!(t.underline_color(&row[3]), Some(Color::Indexed(4)));
        assert_eq!(t.underline_color(&row[4]), None);
    }
    #[test]
    fn underline_color_survives_links_and_combining_marks() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "\x1b[58;2;7;8;9m\x1b]8;;https://a.b\x1b\\e\u{0301}\x1b]8;;\x1b\\x");
        let row = t.grid.row(0);
        assert_eq!(t.underline_color(&row[0]), Some(Color::Rgb(7, 8, 9)));
        assert_eq!(t.link(&row[0]), Some("https://a.b"));
        assert_eq!(t.cluster(&row[0]), Some("\u{0301}"));
        assert_eq!(t.underline_color(&row[1]), Some(Color::Rgb(7, 8, 9)));
        assert_eq!(t.link(&row[1]), None);
    }
    #[test]
    fn sgr_underline_styles() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "\x1b[4ma\x1b[4:2mb\x1b[4:3mc\x1b[4:4md\x1b[4:5me\x1b[21mf\x1b[24mg\x1b[4:3;4mh");
        let style = |c: usize| {
            let f = t.grid.row(0)[c].flags;
            (f & flags::UNDERLINE != 0, f & flags::UNDERLINE_STYLE)
        };
        assert_eq!(style(0), (true, 0));
        assert_eq!(style(1), (true, flags::UNDERLINE_DOUBLE));
        assert_eq!(style(2), (true, flags::UNDERLINE_CURLY));
        assert_eq!(style(3), (true, flags::UNDERLINE_DOTTED));
        assert_eq!(style(4), (true, flags::UNDERLINE_DASHED));
        assert_eq!(style(5), (true, flags::UNDERLINE_DOUBLE));
        assert_eq!(style(6), (false, 0));
        assert_eq!(style(7), (true, 0));
    }
    #[test]
    fn sgr_blink_flag() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "\x1b[5ma\x1b[25mb\x1b[6mc\x1b[0md");
        let blink = |c: usize| t.grid.row(0)[c].flags & flags::BLINK != 0;
        assert!(blink(0) && !blink(1) && blink(2) && !blink(3));
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
                    assert!(c > 0 && is_wide(row[c - 1].ch), "orphan spacer at {r},{c}: {}", t.dump());
                }
                if is_wide(row[c].ch) {
                    assert!(
                        c + 1 < row.len() && row[c + 1].flags & WIDE_SPACER != 0,
                        "unpaired wide half at {r},{c}: {}",
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

    #[test]
    fn reflow_keeps_short_output_on_screen() {
        let mut t = Term::new(40, 20);
        feed(&mut t, "red green blue\r\n");
        t.resize(30, 12);
        assert_eq!(row_text(&t, 0).trim_end(), "red green blue");
        assert_eq!(t.cursor.row, 1);
        assert!(!t.scroll_display(1));
        t.resize(50, 25);
        assert_eq!(row_text(&t, 0).trim_end(), "red green blue");
        assert_eq!(t.cursor.row, 1);
    }

    #[test]
    fn reflow_keeps_full_screen_anchored_to_bottom() {
        let mut t = Term::new(10, 4);
        feed(&mut t, "1\r\n2\r\n3\r\n4\r\n5");
        t.resize(8, 3);
        assert_eq!(row_text(&t, 2).trim_end(), "5");
        assert_eq!(t.cursor.row, 2);
        t.resize(10, 5);
        assert_eq!(row_text(&t, 0).trim_end(), "1");
        assert_eq!(row_text(&t, 4).trim_end(), "5");
    }

    #[test]
    fn dec_special_graphics() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "\x1b(0lqk\x1b(Bx");
        assert_eq!(row_text(&t, 0), "┌─┐x      ");
        let mut t = Term::new(10, 1);
        feed(&mut t, "\x1b)0a\x0eq\x0fq");
        assert_eq!(row_text(&t, 0), "a─q       ");
    }
    #[test]
    fn decsc_restores_charset() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "\x1b(0\x1b7\x1b(B\x1b8q");
        assert_eq!(t.grid.row(0)[0].ch, '─');
    }
    #[test]
    fn rep_repeats_last_char() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "ab\x1b[3b");
        assert_eq!(row_text(&t, 0), "abbbb     ");
    }
    #[test]
    fn tab_stops() {
        let mut t = Term::new(20, 1);
        feed(&mut t, "\tx");
        assert_eq!(t.grid.row(0)[8].ch, 'x');
        let mut t = Term::new(20, 1);
        feed(&mut t, "\x1b[3g\x1b[4G\x1bH\r\tx");
        assert_eq!(t.grid.row(0)[3].ch, 'x');
        let mut t = Term::new(30, 1);
        feed(&mut t, "\x1b[2Ix");
        assert_eq!(t.grid.row(0)[16].ch, 'x');
        assert_eq!(t.cursor.col, 17);
        let mut t = Term::new(30, 1);
        feed(&mut t, "\x1b[20G\x1b[Zy");
        assert_eq!(t.grid.row(0)[16].ch, 'y');
    }
    #[test]
    fn insert_mode_shifts_text() {
        let mut t = Term::new(6, 1);
        feed(&mut t, "abc\r\x1b[4hX\x1b[4lY");
        assert_eq!(row_text(&t, 0), "XYbc  ");
        let mut t = Term::new(6, 1);
        feed(&mut t, "abc\r\x1b[4hXY");
        assert_eq!(row_text(&t, 0), "XYabc ");
    }
    #[test]
    fn origin_mode_addresses_region() {
        let mut t = Term::new(10, 6);
        feed(&mut t, "\x1b[3;5r\x1b[?6h\x1b[1;1Hx");
        assert_eq!(t.grid.row(2)[0].ch, 'x');
        feed(&mut t, "\x1b[10;1H");
        assert_eq!(t.cursor.row, 4);
        feed(&mut t, "\x1b[6n");
        assert_eq!(String::from_utf8_lossy(&t.replies), "\x1b[3;1R");
        feed(&mut t, "\x1b[?6l");
        assert_eq!(t.cursor.row, 0);
    }
    #[test]
    fn cursor_up_stops_at_margin() {
        let mut t = Term::new(10, 6);
        feed(&mut t, "\x1b[2;5r\x1b[4;1H\x1b[10A");
        assert_eq!(t.cursor.row, 1);
    }
    #[test]
    fn newline_mode() {
        let mut t = Term::new(10, 3);
        feed(&mut t, "\x1b[20habc\ndef");
        assert_eq!(row_text(&t, 1), "def       ");
    }
    #[test]
    fn decaln_fills_screen() {
        let mut t = Term::new(3, 2);
        feed(&mut t, "\x1b#8");
        assert_eq!(row_text(&t, 0), "EEE");
        assert_eq!(row_text(&t, 1), "EEE");
    }
    fn shell_session(t: &mut Term) {
        feed(t, "\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07a\r\nb\r\n\r\n\x1b]133;D;0\x07");
        feed(t, "\x1b]133;A\x07$ \x1b]133;B\x07true\r\n\x1b]133;C\x07\x1b]133;D;0\x07");
        feed(t, "\x1b]133;A\x07$ \x1b]133;B\x07echo hi\r\n\x1b]133;C\x07hi\r\n\x1b]133;D;0\x07");
        feed(t, "\x1b]133;A\x07$ ");
    }
    #[test]
    fn osc133_marks_prompts() {
        let mut t = Term::new(20, 4);
        shell_session(&mut t);
        assert_eq!(t.prompt_lines(), vec![0, 4, 5, 7]);
        assert_eq!(t.command_outputs(), vec![(1, 2), (6, 6)]);
        t.resize(5, 3);
        t.resize(20, 4);
        assert_eq!(t.prompt_lines().len(), 4);
        assert_eq!(t.select_command_output().as_deref(), Some("hi"));
    }
    #[test]
    fn osc133_marks_survive_prompt_redraw_but_not_clear() {
        let mut t = Term::new(10, 4);
        feed(&mut t, "out\r\n\x1b]133;A\x07\x1b[7m%\x1b[m         \r\x1b[K$ ls\r\x1b[J$ ls");
        assert_eq!(t.prompt_lines(), vec![1]);
        feed(&mut t, "\x1b[H\x1b[2J");
        assert!(t.prompt_lines().is_empty());
    }
    #[test]
    fn osc133_jumps_between_prompts() {
        let mut t = Term::new(20, 4);
        shell_session(&mut t);
        assert_eq!(t.abs_line(0), 4);
        assert!(t.jump_to_prompt(true));
        assert_eq!(t.abs_line(0), 0);
        assert!(!t.jump_to_prompt(true));
        t.scroll_display(-1);
        assert!(t.jump_to_prompt(true));
        assert_eq!(t.abs_line(0), 0);
        assert!(t.jump_to_prompt(false));
        assert_eq!(t.abs_line(0), 4);
        assert!(!t.jump_to_prompt(false));
    }
    #[test]
    fn osc133_selects_command_output() {
        let mut t = Term::new(20, 4);
        shell_session(&mut t);
        assert_eq!(t.select_command_output().as_deref(), Some("hi"));
        assert_eq!(t.select_command_output().as_deref(), Some("a\nb"));
        assert_eq!(t.abs_line(0), 1);
        assert_eq!(t.select_command_output(), None);
    }
    #[test]
    fn osc133_without_output_marks_uses_prompt_end() {
        let mut t = Term::new(20, 6);
        feed(&mut t, "\x1b]133;A\x07$ \x1b]133;B\x07ls\r\nx\r\ny\r\n\x1b]133;A\x07$ ");
        assert_eq!(t.command_outputs(), vec![(1, 2)]);
        let mut t = Term::new(20, 6);
        feed(&mut t, "\x1b]133;A\x07$ ls\r\nx\r\n\x1b]133;A\x07$ ");
        assert_eq!(t.command_outputs(), vec![(1, 1)]);
    }
    #[test]
    fn osc133_accepts_fish_parameters() {
        let mut t = Term::new(30, 8);
        feed(&mut t, "\x1b]133;A;click_events=1\x07> ls\r\n\x1b]133;C;cmdline_url=ls%20-la\x07a\r\nb\r\n\x1b]133;D;0\x07");
        feed(
            &mut t,
            "\x1b]133;A;click_events=1\x07> true\r\n\x1b]133;C;cmdline_url=true\x07\x1b]133;D;0\x07",
        );
        feed(
            &mut t,
            "\x1b]133;A;click_events=1\x07> false\r\n\x1b]133;C;cmdline_url=false\x07x\r\n\x1b]133;D;1\x07",
        );
        feed(&mut t, "\x1b]133;A;click_events=1\x07> ");
        assert_eq!(t.prompt_lines(), vec![0, 3, 4, 6]);
        assert_eq!(t.command_outputs(), vec![(1, 2), (5, 5)]);
        assert_eq!(t.select_command_output().as_deref(), Some("x"));
        assert_eq!(t.select_command_output().as_deref(), Some("a\nb"));
    }
    #[test]
    fn osc133_without_b_or_c_falls_back_to_line_after_prompt() {
        let mut t = Term::new(30, 8);
        feed(
            &mut t,
            "\x1b]133;A;click_events=1\x07> ls\r\nout\r\n\x1b]133;D;0\x07\x1b]133;A;click_events=1\x07> ",
        );
        assert_eq!(t.command_outputs(), vec![(1, 1)]);
    }
    #[test]
    fn osc133_ignored_on_alt_screen() {
        let mut t = Term::new(20, 4);
        shell_session(&mut t);
        feed(&mut t, "\x1b[?1049h");
        assert!(!t.jump_to_prompt(true));
        assert!(t.command_outputs().is_empty());
    }
    #[test]
    fn osc_notifications() {
        let mut t = Term::new(20, 4);
        feed(&mut t, "\x1b]9;build done\x07\x1b]9;4;1;50\x07\x1b]777;notify;Title;body; with semicolon\x07");
        assert_eq!(t.notifications.len(), 2);
        assert_eq!(t.notifications[0].title, "build done");
        assert_eq!(t.notifications[1].title, "Title");
        assert_eq!(t.notifications[1].body, "body; with semicolon");
        assert!(!t.notifications[1].only_unfocused);
    }
    #[test]
    fn osc99_notifications() {
        let mut t = Term::new(20, 4);
        feed(
            &mut t,
            "\x1b]99;i=1:d=0;Hel\x1b\\\x1b]99;i=1:d=0;lo\x1b\\\x1b]99;i=1:p=body:u=2:o=unfocused;world\x1b\\",
        );
        feed(&mut t, "\x1b]99;e=1;aGk=\x1b\\\x1b]99;p=?;\x1b\\\x1b]99;;\x1b\\");
        assert_eq!(t.notifications.len(), 2);
        let n = &t.notifications[0];
        assert_eq!((n.title.as_str(), n.body.as_str()), ("Hello", "world"));
        assert_eq!(n.urgency, Urgency::Critical);
        assert!(n.only_unfocused);
        assert_eq!(t.notifications[1].title, "hi");
    }
    #[test]
    fn notifications_are_bounded() {
        let mut t = Term::new(20, 4);
        for _ in 0..100 {
            feed(&mut t, "\x1b]9;x\x07");
        }
        assert_eq!(t.notifications.len(), NOTIFY_QUEUE_MAX);
        t.notifications.clear();
        feed(&mut t, &format!("\x1b]777;notify;{};b\x07", "t".repeat(1000)));
        assert_eq!(t.notifications[0].title.chars().count(), NOTIFY_TITLE_MAX);
    }
    #[test]
    fn osc22_pointer_shape() {
        let mut t = Term::new(20, 4);
        feed(&mut t, "\x1b]22;pointer\x07");
        assert_eq!(t.pointer_shape(), Some(CursorIcon::Pointer));
        feed(&mut t, "\x1b]22;bogus\x07");
        assert_eq!(t.pointer_shape(), Some(CursorIcon::Pointer));
        feed(&mut t, "\x1b]22;>bogus,watch\x07");
        assert_eq!(t.pointer_shape(), Some(CursorIcon::Wait));
        feed(&mut t, "\x1b]22;=crosshair\x07");
        assert_eq!(t.pointer_shape(), Some(CursorIcon::Crosshair));
        feed(&mut t, "\x1b]22;<\x07");
        assert_eq!(t.pointer_shape(), Some(CursorIcon::Pointer));
        feed(&mut t, "\x1b]22;<\x07\x1b]22;<\x07");
        assert_eq!(t.pointer_shape(), None);
        feed(&mut t, "\x1b]22;text\x07\x1b]22;\x07");
        assert_eq!(t.pointer_shape(), None);
        feed(&mut t, "\x1b]22;grab\x07\x1bc");
        assert_eq!(t.pointer_shape(), None);
    }
    #[test]
    fn osc22_query_current() {
        let mut t = Term::new(20, 4);
        feed(&mut t, "\x1b]22;?__current__\x1b\\");
        assert_eq!(t.replies, b"\x1b]22;0\x1b\\");
        t.replies.clear();
        feed(&mut t, "\x1b]22;>wait\x07\x1b]22;?__current__\x1b\\");
        assert_eq!(t.replies, b"\x1b]22;wait\x1b\\");
    }
    #[test]
    fn osc22_query_support() {
        let mut t = Term::new(20, 4);
        feed(&mut t, "\x1b]22;?pointer,crosshair,no-such-name,wait\x1b\\");
        assert_eq!(t.replies, b"\x1b]22;1,1,0,1\x1b\\");
    }
    #[test]
    fn bell_sets_flag() {
        let mut t = Term::new(3, 1);
        feed(&mut t, "\x07");
        assert!(t.bell);
        let mut t = Term::new(3, 1);
        feed(&mut t, "\x1b]0;title\x07");
        assert!(!t.bell);
        assert_eq!(t.title, "title");
    }
    #[test]
    fn combining_marks_join_previous_cell() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "e\u{0301}x");
        assert_eq!(t.grid.row(0)[0].ch, 'e');
        assert_eq!(t.cluster(&t.grid.row(0)[0]), Some("\u{0301}"));
        assert_eq!(t.grid.row(0)[1].ch, 'x');
        t.selection_start(0, 0, SelMode::Simple);
        t.selection_update(0, 1);
        assert_eq!(t.selection_text().unwrap(), "e\u{0301}x");
    }
    #[test]
    fn combining_at_wrap_edge() {
        let mut t = Term::new(3, 2);
        feed(&mut t, "abc\u{0301}");
        assert_eq!(t.cluster(&t.grid.row(0)[2]), Some("\u{0301}"));
        assert_eq!(t.cursor.row, 0);
    }
    #[test]
    fn zwj_sequence_is_one_cell_pair() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "👨\u{200D}👩\u{200D}👧!");
        assert_eq!(t.grid.row(0)[0].ch, '👨');
        assert_eq!(t.cluster(&t.grid.row(0)[0]), Some("\u{200D}👩\u{200D}👧"));
        assert_eq!(t.grid.row(0)[2].ch, '!');
    }
    #[test]
    fn emoji_modifier_joins() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "👍🏽x");
        assert_eq!(t.cluster(&t.grid.row(0)[0]), Some("🏽"));
        assert_eq!(t.grid.row(0)[2].ch, 'x');
    }
    #[test]
    fn osc7_sets_cwd() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "\x1b]7;file://host/home/me/my%20dir\x07");
        assert_eq!(t.cwd.as_deref(), Some("/home/me/my dir"));
    }
    #[test]
    fn osc8_hyperlinks() {
        let mut t = Term::new(20, 1);
        feed(&mut t, "\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\ no");
        assert_eq!(t.link_at(0, 0).as_deref(), Some("https://example.com"));
        assert_eq!(t.link_at(0, 3).as_deref(), Some("https://example.com"));
        assert_eq!(t.link_at(0, 5), None);
        feed(&mut t, "\x1b[0m");
        assert_eq!(t.link_at(0, 0).as_deref(), Some("https://example.com"));
    }
    #[test]
    fn url_detection() {
        let mut t = Term::new(60, 2);
        feed(&mut t, "see (https://example.com/a_b?x=1). ok mailto:me@x.org");
        assert_eq!(t.url_at(0, 10).as_deref(), Some("https://example.com/a_b?x=1"));
        assert_eq!(t.url_at(0, 2), None);
        assert_eq!(t.url_at(0, 45).as_deref(), Some("mailto:me@x.org"));
        let mut t = Term::new(10, 3);
        feed(&mut t, "http://abcdefghijklmn");
        assert_eq!(t.url_at(1, 3).as_deref(), Some("http://abcdefghijklmn"));
    }
    #[test]
    fn kitty_keyboard_protocol() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "\x1b[?u");
        assert_eq!(String::from_utf8_lossy(&std::mem::take(&mut t.replies)), "\x1b[?0u");
        feed(&mut t, "\x1b[>31u\x1b[?u");
        assert_eq!(String::from_utf8_lossy(&std::mem::take(&mut t.replies)), "\x1b[?1u");
        feed(&mut t, "\x1b[?1049h");
        assert_eq!(t.kb_flags, 0);
        feed(&mut t, "\x1b[?1049l");
        assert_eq!(t.kb_flags, 1);
        feed(&mut t, "\x1b[<u");
        assert_eq!(t.kb_flags, 0);
        feed(&mut t, "\x1b[=1;1u");
        assert_eq!(t.kb_flags, 1);
        feed(&mut t, "\x1b[=1;3u");
        assert_eq!(t.kb_flags, 0);
    }
    #[test]
    fn pixel_size_reports() {
        let mut t = Term::new(10, 5);
        t.cell_px = (8, 16);
        feed(&mut t, "\x1b[14t\x1b[16t\x1b[18t");
        assert_eq!(String::from_utf8_lossy(&t.replies), "\x1b[4;80;80t\x1b[6;16;8t\x1b[8;5;10t");
    }
    #[test]
    fn osc4_query_uses_configured_palette() {
        let mut cfg = crate::config::Config::defaults();
        cfg.palette.ansi[200] = [1, 2, 3];
        let mut t = Term::new(10, 1);
        t.apply_config(&cfg);
        feed(&mut t, "\x1b]4;200;?\x07");
        assert_eq!(
            String::from_utf8_lossy(&std::mem::take(&mut t.replies)),
            "\x1b]4;200;rgb:0101/0202/0303\x1b\\"
        );
        feed(&mut t, "\x1b]4;200;#0a0b0c\x07\x1bc\x1b]4;200;?\x07");
        assert_eq!(String::from_utf8_lossy(&t.replies), "\x1b]4;200;rgb:0101/0202/0303\x1b\\");
    }
    #[test]
    fn decrqm_reports_new_modes() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "\x1b[4h\x1b[4$p\x1b[?6$p");
        assert_eq!(String::from_utf8_lossy(&t.replies), "\x1b[4;1$y\x1b[?6;2$y");
    }
    #[test]
    fn sgr_reset_keeps_link_and_underline_styles() {
        let mut t = Term::new(10, 1);
        feed(&mut t, "\x1b[4:3ma\x1b[4:0mb");
        assert_ne!(t.grid.row(0)[0].flags & flags::UNDERLINE, 0);
        assert_eq!(t.grid.row(0)[1].flags & flags::UNDERLINE, 0);
    }
    #[test]
    fn reflow_round_trip() {
        let mut t = Term::new(10, 4);
        feed(&mut t, "0123456789abcdef\r\nxy");
        t.resize(5, 6);
        t.resize(10, 4);
        assert_eq!(row_text(&t, 0), "0123456789");
        assert_eq!(row_text(&t, 1), "abcdef    ");
        assert_eq!(row_text(&t, 2), "xy        ");
    }
    #[test]
    fn utf8_split_across_reads() {
        let mut t = Term::new(10, 1);
        let mut p = Parser::new();
        let bytes = "é你".as_bytes();
        for b in bytes {
            p.advance(&mut t, std::slice::from_ref(b));
        }
        assert_eq!(t.grid.row(0)[0].ch, 'é');
        assert_eq!(t.grid.row(0)[1].ch, '你');
    }
    #[test]
    fn long_osc_is_bounded() {
        let mut t = Term::new(10, 1);
        let title = "x".repeat(100_000);
        feed(&mut t, &format!("\x1b]2;{title}\x07ok"));
        assert!(t.title.len() <= 4096);
        assert_eq!(row_text(&t, 0), "ok        ");
    }
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }
    #[test]
    fn random_input_never_panics() {
        let pieces: [&[u8]; 24] = [
            b"\x1b[",
            b"\x1b]",
            b"\x1b(0",
            b"\x1b)0",
            b"\x0e",
            b"\x0f",
            b"\x1b#8",
            b"\x1b[?1049h",
            b"\x1b[?6h",
            b"\x1b[4h",
            b";",
            b":",
            b"9999",
            b"\x07",
            b"\x1b\\",
            "你".as_bytes(),
            "\u{0301}".as_bytes(),
            "\u{200D}".as_bytes(),
            "👍".as_bytes(),
            b"\r\n",
            b"\t",
            b"b",
            b"\x1b[3g",
            b"\x1b[>1u",
        ];
        let mut rng = Rng(0x9e3779b97f4a7c15);
        for round in 0..300 {
            let cols = 1 + (rng.next() % 40) as usize;
            let rows = 1 + (rng.next() % 12) as usize;
            let mut t = Term::new(cols, rows);
            let mut p = Parser::new();
            for _ in 0..400 {
                let r = rng.next();
                if r.is_multiple_of(3) {
                    let b = (r >> 8) as u8;
                    p.advance(&mut t, &[b]);
                } else {
                    p.advance(&mut t, pieces[(r >> 8) as usize % pieces.len()]);
                }
                if r.is_multiple_of(97) {
                    t.resize(1 + (rng.next() % 40) as usize, 1 + (rng.next() % 12) as usize);
                }
                if r.is_multiple_of(89) {
                    let rr = (rng.next() % t.rows() as u64) as usize;
                    let cc = (rng.next() % t.cols() as u64) as usize;
                    let _ = t.url_at(rr, cc);
                    t.selection_start(0, 0, SelMode::Word);
                    t.selection_update(rr, cc);
                    let _ = t.selection_text();
                }
                t.replies.clear();
            }
            assert!(t.cursor.row < t.rows() && t.cursor.col < t.cols(), "round {round}");
        }
    }
}

#[cfg(test)]
mod size_tests {
    #[test]
    fn cell_stays_small() {
        assert_eq!(std::mem::size_of::<crate::cell::Cell>(), 16);
    }
}
