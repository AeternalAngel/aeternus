use super::{mix, x_cross, Instance, Renderer};
use crate::colors::Palette;
use crate::config::Action as KeyAction;
use crate::search::Search;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Copy,
    Paste,
    PastePrimary,
    SelectAll,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ScrollTop,
    ScrollBottom,
    ResetTerm,
    Reload,
    Settings,
    Quit,
    About,
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    Fullscreen,
    Find,
}

impl Action {
    pub fn key_action(self) -> Option<KeyAction> {
        Some(match self {
            Action::Copy => KeyAction::Copy,
            Action::Paste => KeyAction::Paste,
            Action::PastePrimary => KeyAction::PastePrimary,
            Action::ZoomIn => KeyAction::ZoomIn,
            Action::ZoomOut => KeyAction::ZoomOut,
            Action::ZoomReset => KeyAction::ZoomReset,
            Action::ScrollTop => KeyAction::ScrollToTop,
            Action::ScrollBottom => KeyAction::ScrollToBottom,
            Action::Reload => KeyAction::ReloadConfig,
            Action::Settings => KeyAction::Preferences,
            Action::NewTab => KeyAction::NewTab,
            Action::CloseTab => KeyAction::CloseTab,
            Action::NextTab => KeyAction::NextTab,
            Action::PrevTab => KeyAction::PrevTab,
            Action::Fullscreen => KeyAction::Fullscreen,
            Action::Find => KeyAction::Search,
            Action::SelectAll | Action::ResetTerm | Action::Quit | Action::About => return None,
        })
    }
}

pub struct MenuItem {
    pub label: &'static str,
    pub action: Option<Action>,
}
const SEP: MenuItem = MenuItem { label: "", action: None };
const fn item(label: &'static str, action: Action) -> MenuItem {
    MenuItem { label, action: Some(action) }
}
pub const MENUS: [(&str, &[MenuItem]); 4] = [
    (
        "File",
        &[
            item("New Tab", Action::NewTab),
            item("Close Tab", Action::CloseTab),
            SEP,
            item("Reload Configuration", Action::Reload),
            SEP,
            item("Quit", Action::Quit),
        ],
    ),
    (
        "Edit",
        &[
            item("Copy", Action::Copy),
            item("Paste", Action::Paste),
            item("Paste Selection", Action::PastePrimary),
            item("Select All", Action::SelectAll),
            item("Find", Action::Find),
            SEP,
            item("Preferences", Action::Settings),
        ],
    ),
    (
        "View",
        &[
            item("Scroll to Top", Action::ScrollTop),
            item("Scroll to Bottom", Action::ScrollBottom),
            SEP,
            item("Zoom In", Action::ZoomIn),
            item("Zoom Out", Action::ZoomOut),
            item("Reset Zoom", Action::ZoomReset),
            SEP,
            item("Fullscreen", Action::Fullscreen),
            SEP,
            item("Reset Terminal", Action::ResetTerm),
        ],
    ),
    ("Help", &[item("About Aeternus", Action::About)]),
];
pub(super) struct PanelGeom {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub item_h: f32,
    pub sep_h: f32,
}

impl Renderer {
    pub fn top_inset(&self) -> f32 {
        self.toolbar_h + self.tab_h()
    }
    pub(super) fn tab_h(&self) -> f32 {
        if self.tabs.len() > 1 {
            (28.0 * self.scale).round()
        } else {
            0.0
        }
    }
    pub fn tabs_visible(&self) -> bool {
        self.tabs.len() > 1
    }
    pub fn toolbar_h(&self) -> f32 {
        self.toolbar_h
    }
    pub fn set_tabs(&mut self, tabs: &[(u64, String)], active: u64) -> bool {
        let changed = self.tabs != *tabs || self.active_tab != active;
        self.tabs = tabs.to_vec();
        self.active_tab = active;
        changed
    }
    pub fn set_hover_tab(&mut self, t: Option<u64>) -> bool {
        std::mem::replace(&mut self.hover_tab, t) != t
    }
    pub fn set_hover_close(&mut self, t: Option<u64>) -> bool {
        std::mem::replace(&mut self.hover_close, t) != t
    }
    pub fn set_hover_new(&mut self, h: bool) -> bool {
        std::mem::replace(&mut self.hover_new, h) != h
    }
    pub fn set_hover_gear(&mut self, h: bool) -> bool {
        std::mem::replace(&mut self.hover_gear, h) != h
    }
    pub fn set_hover_prev(&mut self, h: bool) -> bool {
        std::mem::replace(&mut self.hover_prev, h) != h
    }
    pub fn set_hover_next(&mut self, h: bool) -> bool {
        std::mem::replace(&mut self.hover_next, h) != h
    }
    pub fn clear_hover(&mut self) -> bool {
        let mut c = self.set_hover_tab(None);
        c |= self.set_hover_close(None);
        c |= self.set_hover_new(false);
        c |= self.set_hover_prev(false);
        c |= self.set_hover_next(false);
        c
    }
    fn in_tab_strip(&self, y: f64) -> bool {
        let y = y as f32;
        self.tabs.len() > 1 && y >= self.toolbar_h && y < self.toolbar_h + self.tab_h()
    }
    pub fn hit_tab(&self, x: f64, y: f64) -> Option<u64> {
        if !self.in_tab_strip(y) {
            return None;
        }
        let x = x as f32;
        self.tab_rects.iter().find(|&&(tx, tw, _)| x >= tx && x < tx + tw).map(|&(_, _, id)| id)
    }
    pub fn hit_tab_close(&self, x: f64, y: f64) -> Option<u64> {
        if !self.in_tab_strip(y) {
            return None;
        }
        let x = x as f32;
        self.close_rects.iter().find(|&&(cx, cw, _)| x >= cx && x < cx + cw).map(|&(_, _, id)| id)
    }
    pub fn hit_new_tab(&self, x: f64, y: f64) -> bool {
        self.in_tab_strip(y) && hit_span(self.new_tab_rect, x)
    }
    pub fn hit_gear(&self, x: f64, y: f64) -> bool {
        let y = y as f32;
        y >= 0.0 && y < self.toolbar_h && hit_span(self.gear_rect, x)
    }
    pub fn hit_tab_prev(&self, x: f64, y: f64) -> bool {
        self.in_tab_strip(y) && hit_span(self.tab_prev_rect, x)
    }
    pub fn hit_tab_next(&self, x: f64, y: f64) -> bool {
        self.in_tab_strip(y) && hit_span(self.tab_next_rect, x)
    }
    pub fn tab_strip_hit(&self, _x: f64, y: f64) -> bool {
        self.in_tab_strip(y)
    }
    fn tab_widths(&self) -> Vec<f32> {
        let cw = self.cell_w;
        self.tabs.iter().map(|(_, label)| (label.chars().count().min(18) as f32 + 4.0) * cw).collect()
    }
    pub fn tab_strip_scroll(&mut self, delta: isize) -> bool {
        if self.tabs.len() < 2 {
            return false;
        }
        let max = (self.tabs.len() - 1) as isize;
        let old = self.tab_scroll;
        self.tab_scroll = (self.tab_scroll as isize + delta).clamp(0, max) as usize;
        self.tab_scroll != old
    }
    pub fn ensure_tab_visible(&mut self, id: u64) {
        if self.tabs.len() < 2 {
            return;
        }
        let Some(idx) = self.tabs.iter().position(|t| t.0 == id) else {
            return;
        };
        let widths = self.tab_widths();
        let cw = self.cell_w;
        let w = self.config.width as f32;
        let nw = 2.0 * cw;
        let arrow_w = 2.0 * cw;
        let limit = w - self.pad - nw - cw * 0.25 - 2.0 * arrow_w - cw * 0.5;
        let mut scroll = self.tab_scroll.min(idx);
        loop {
            let mut x = self.pad;
            let mut k = 0usize;
            while scroll + k < widths.len() {
                if x + widths[scroll + k] > limit {
                    break;
                }
                x += widths[scroll + k];
                k += 1;
            }
            if idx < scroll + k || scroll >= idx {
                break;
            }
            scroll += 1;
        }
        self.tab_scroll = scroll;
    }
    pub fn menu_open(&self) -> Option<usize> {
        self.open_menu
    }
    pub fn set_open_menu(&mut self, m: Option<usize>) -> bool {
        std::mem::replace(&mut self.open_menu, m) != m
    }
    pub fn set_hover_title(&mut self, t: Option<usize>) -> bool {
        std::mem::replace(&mut self.hover_title, t) != t
    }
    pub fn set_hover_item(&mut self, i: Option<usize>) -> bool {
        std::mem::replace(&mut self.hover_item, i) != i
    }
    pub fn hit_title(&self, x: f64, y: f64) -> Option<usize> {
        if !self.toolbar || y as f32 > self.toolbar_h {
            return None;
        }
        let x = x as f32;
        self.title_rects.iter().position(|&(tx, tw)| x >= tx && x < tx + tw)
    }
    pub fn hit_item_index(&self, x: f64, y: f64) -> Option<usize> {
        let m = self.open_menu?;
        let g = self.panel_geom(m)?;
        let (x, y) = (x as f32, y as f32);
        if x < g.x || x >= g.x + g.w || y < g.y {
            return None;
        }
        let mut ry = g.y + self.cell_h / 2.0;
        for (i, it) in MENUS[m].1.iter().enumerate() {
            let rh = if it.action.is_none() { g.sep_h } else { g.item_h };
            if y >= ry && y < ry + rh {
                return if it.action.is_none() { None } else { Some(i) };
            }
            ry += rh;
        }
        None
    }
    pub fn item_action(&self, m: usize, i: usize) -> Option<Action> {
        MENUS.get(m)?.1.get(i)?.action
    }
    fn shortcut(&self, a: Option<Action>) -> Option<&str> {
        self.shortcuts.get(&a?).map(String::as_str)
    }
    pub(super) fn panel_geom(&self, m: usize) -> Option<PanelGeom> {
        let &(tx, _) = self.title_rects.get(m)?;
        let (cw, ch) = (self.cell_w, self.cell_h);
        let item_h = (ch * 1.6).round();
        let sep_h = (item_h / 3.0).round().max(2.0);
        let mut w: f32 = 0.0;
        for it in MENUS[m].1 {
            if it.action.is_none() {
                continue;
            }
            let mut need = it.label.chars().count() as f32 * cw + 2.0 * cw;
            if let Some(s) = self.shortcut(it.action) {
                need += s.chars().count() as f32 * cw + 2.0 * cw;
            }
            w = w.max(need);
        }
        w += 2.0 * cw;
        let mut h = 0.0;
        for it in MENUS[m].1 {
            h += if it.action.is_none() { sep_h } else { item_h };
        }
        h += ch;
        let x = tx.min((self.config.width as f32 - w).max(0.0));
        Some(PanelGeom { x, y: self.toolbar_h, w, h, item_h, sep_h })
    }
    pub(super) fn push_toolbar(
        &mut self,
        bgs: &mut Vec<Instance>,
        fgs: &mut Vec<Instance>,
        base: [u8; 3],
        fg: [u8; 3],
        thick: f32,
    ) {
        let (cw, ch, pad) = (self.cell_w, self.cell_h, self.pad);
        let w = self.config.width as f32;
        let mut title_rects = std::mem::take(&mut self.title_rects);
        title_rects.clear();
        self.gear_rect = None;
        if self.toolbar {
            let top = self.toolbar_h;
            let text_y = (top - ch) / 2.0;
            bgs.push(Instance::rect_alpha(0.0, 0.0, w, top, mix(base, fg, 0.06), self.bg_alpha()));
            bgs.push(Instance::rect(0.0, top - thick, w, thick, mix(base, fg, 0.25)));
            let mut tx = pad;
            for (mi, (title, _)) in MENUS.iter().enumerate() {
                let tw = title.chars().count() as f32 * cw + cw;
                if tx + tw > w - pad {
                    break;
                }
                let active = self.open_menu == Some(mi);
                let hot = self.hover_title == Some(mi);
                if active || hot {
                    bgs.push(Instance::rect(
                        tx,
                        0.0,
                        tw,
                        top - thick,
                        mix(base, fg, if active { 0.18 } else { 0.10 }),
                    ));
                }
                self.push_text(fgs, title, tx + cw * 0.5, text_y, fg);
                title_rects.push((tx, tw));
                tx += tw + cw * 0.5;
            }
        }
        self.title_rects = title_rects;
        if self.toolbar {
            self.push_gear(bgs, fgs, base, fg, w, pad, thick);
        }
        if self.tab_h() > 0.0 {
            self.push_tab_strip(bgs, fgs, base, fg, thick);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn push_gear(
        &mut self,
        bgs: &mut Vec<Instance>,
        fgs: &mut Vec<Instance>,
        base: [u8; 3],
        fg: [u8; 3],
        w: f32,
        pad: f32,
        thick: f32,
    ) {
        let gw = (30.0 * self.scale).round();
        let gx = w - pad - gw;
        self.gear_rect = Some((gx, gw));
        if self.hover_gear {
            bgs.push(Instance::rect(gx, 0.0, gw, self.toolbar_h - thick, mix(base, fg, 0.10)));
        }
        let icon = (12.0 * self.scale).round().max(8.0);
        let cx = gx + gw / 2.0;
        let cy = (self.toolbar_h - thick) / 2.0;
        let col = mix(base, fg, 0.75);
        let rows = [cy - icon / 2.0, cy, cy + icon / 2.0];
        let knobs = [cx + icon * 0.18, cx - icon * 0.22, cx + icon * 0.10];
        let ks = (thick * 3.0).max(3.0);
        for (row, knob) in rows.iter().zip(knobs) {
            fgs.push(Instance::rect(cx - icon / 2.0, row - thick / 2.0, icon, thick, col));
            fgs.push(Instance::rect(knob - ks / 2.0, row - ks / 2.0, ks, ks, col));
        }
    }
    fn push_tab_strip(
        &mut self,
        bgs: &mut Vec<Instance>,
        fgs: &mut Vec<Instance>,
        base: [u8; 3],
        fg: [u8; 3],
        thick: f32,
    ) {
        let (cw, ch, pad) = (self.cell_w, self.cell_h, self.pad);
        let w = self.config.width as f32;
        let ty = self.toolbar_h;
        let th = self.tab_h();
        bgs.push(Instance::rect_alpha(0.0, ty, w, th, mix(base, fg, 0.03), self.bg_alpha()));
        bgs.push(Instance::rect(0.0, ty + th - thick, w, thick, mix(base, fg, 0.18)));
        self.tab_rects.clear();
        self.close_rects.clear();
        self.new_tab_rect = None;
        self.tab_prev_rect = None;
        self.tab_next_rect = None;
        let widths = self.tab_widths();
        let n = widths.len();
        let nw = 2.0 * cw;
        let plus_x = w - pad - nw;
        let arrow_w = 2.0 * cw;
        let next_x = plus_x - cw * 0.25 - arrow_w;
        let prev_x = next_x - arrow_w;
        let limit_full = plus_x - cw * 0.5;
        let limit_arrows = prev_x - cw * 0.5;
        let fit = |start: usize, limit: f32| -> usize {
            let mut x = pad;
            let mut k = 0usize;
            while start + k < n {
                if x + widths[start + k] > limit {
                    break;
                }
                x += widths[start + k];
                k += 1;
            }
            k
        };
        let total = |a: usize| widths[a..].iter().sum::<f32>();
        let mut scroll = self.tab_scroll.min(n.saturating_sub(1));
        while scroll > 0 && total(scroll - 1) <= limit_full - pad {
            scroll -= 1;
        }
        let mut k = fit(scroll, limit_full);
        let arrows = scroll > 0 || scroll + k < n;
        if arrows {
            k = fit(scroll, limit_arrows);
        }
        self.tab_scroll = scroll;
        let tabs = self.tabs.clone();
        let active = self.active_tab;
        let mut tx = pad;
        for (i, (id, label)) in tabs.iter().enumerate().skip(scroll).take(k) {
            let mut shown: String = label.chars().take(18).collect();
            if label.chars().count() > 18 {
                shown = format!("{}..", shown.chars().take(16).collect::<String>());
            }
            let tab_w = widths[i];
            let active_tab = *id == active;
            let hover = self.hover_tab == Some(*id);
            if active_tab {
                bgs.push(Instance::rect(tx, ty, tab_w, th - thick, mix(base, fg, 0.10)));
            } else if hover {
                bgs.push(Instance::rect(tx, ty, tab_w, th - thick, mix(base, fg, 0.06)));
            }
            let text_col = if active_tab { fg } else { mix(base, fg, 0.55) };
            self.push_text(fgs, &shown, tx + cw * 0.75, ty + (th - thick - ch) / 2.0, text_col);
            self.tab_rects.push((tx, tab_w, *id));
            let cxx = tx + tab_w - cw * 1.5;
            let ccol = if self.hover_close == Some(*id) { [0xd8, 0x3a, 0x2a] } else { mix(base, fg, 0.45) };
            let s = (cw * 1.2).round().max(thick * 3.0);
            x_cross(fgs, cxx, ty + (th - thick) / 2.0, s, thick, ccol);
            self.close_rects.push((cxx - cw * 0.75, cw * 3.0, *id));
            tx += tab_w;
        }
        let text_y = ty + (th - thick - ch) / 2.0;
        if arrows {
            let arrow_col = |disabled: bool, hover: bool| {
                if disabled {
                    mix(base, fg, 0.25)
                } else if hover {
                    fg
                } else {
                    mix(base, fg, 0.7)
                }
            };
            let prev_col = arrow_col(scroll == 0, self.hover_prev);
            let next_col = arrow_col(scroll + k >= n, self.hover_next);
            self.push_text(fgs, "\u{2039}", prev_x + (arrow_w - cw) / 2.0, text_y, prev_col);
            self.push_text(fgs, "\u{203a}", next_x + (arrow_w - cw) / 2.0, text_y, next_col);
            self.tab_prev_rect = Some((prev_x, arrow_w));
            self.tab_next_rect = Some((next_x, arrow_w));
        }
        let nx = plus_x;
        if self.hover_new {
            bgs.push(Instance::rect(
                nx - cw * 0.25,
                ty + thick,
                nw + cw * 0.5,
                th - 2.0 * thick,
                mix(base, fg, 0.08),
            ));
        }
        let cy = ty + (th - thick) / 2.0;
        let pl = cw * 0.9;
        let pcol = mix(base, fg, 0.7);
        fgs.push(Instance::rect(nx + nw / 2.0 - pl / 2.0, cy - thick / 2.0, pl, thick, pcol));
        fgs.push(Instance::rect(nx + nw / 2.0 - thick / 2.0, cy - pl / 2.0, thick, pl, pcol));
        self.new_tab_rect = Some((nx - cw * 0.25, nw + cw * 0.5));
    }
    pub(super) fn push_menu_panel(
        &mut self,
        fgs: &mut Vec<Instance>,
        m: usize,
        base: [u8; 3],
        fg: [u8; 3],
        thick: f32,
    ) {
        let Some(g) = self.panel_geom(m) else { return };
        let (cw, ch) = (self.cell_w, self.cell_h);
        let border = mix(base, fg, 0.35);
        fgs.push(Instance::rect(g.x, g.y, g.w, g.h, mix(base, fg, 0.07)));
        super::outline(fgs, g.x, g.y, g.w, g.h, thick, border);
        let mut ry = g.y + ch / 2.0;
        for (i, it) in MENUS[m].1.iter().enumerate() {
            if it.action.is_none() {
                fgs.push(Instance::rect(
                    g.x + cw,
                    ry + g.sep_h / 2.0 - thick / 2.0,
                    g.w - 2.0 * cw,
                    thick,
                    mix(base, fg, 0.22),
                ));
                ry += g.sep_h;
                continue;
            }
            if self.hover_item == Some(i) {
                fgs.push(Instance::rect(g.x + thick, ry, g.w - 2.0 * thick, g.item_h, mix(base, fg, 0.16)));
            }
            let ty = ry + (g.item_h - ch) / 2.0;
            self.push_text(fgs, it.label, g.x + cw, ty, fg);
            if let Some(s) = self.shortcut(it.action).map(str::to_string) {
                let sw = s.chars().count() as f32 * cw;
                self.push_text(fgs, &s, g.x + g.w - cw - sw, ty, mix(base, fg, 0.55));
            }
            ry += g.item_h;
        }
    }
}

impl Renderer {
    pub(super) fn push_search_bar(&mut self, fgs: &mut Vec<Instance>, s: &Search, pal: &Palette, thick: f32) {
        let (cw, ch, pad) = (self.cell_w, self.cell_h, self.pad);
        let (base, fg) = (pal.bg, pal.fg);
        let muted = mix(base, fg, 0.55);
        let count = match (s.current, s.matches.len()) {
            _ if s.query.is_empty() => String::new(),
            (_, 0) => "No matches".to_string(),
            (cur, n) => {
                let plus = if s.capped() { "+" } else { "" };
                format!("{}/{n}{plus}", cur.map_or(0, |i| i + 1))
            }
        };
        let label = "Find";
        let label_w = label.chars().count() as f32 * cw;
        let count_w = count.chars().count().max(5) as f32 * cw;
        let avail = self.config.width as f32 - 2.0 * pad;
        let w = (cw * 44.0).min(avail).max(label_w + count_w + cw * 6.0);
        let h = (ch * 1.6).round();
        let x = (self.config.width as f32 - pad - w).max(0.0).round();
        let y = (self.top_inset() + pad).round();
        fgs.push(Instance::rect(x, y, w, h, mix(base, fg, 0.07)));
        super::outline(fgs, x, y, w, h, thick, mix(base, fg, 0.35));
        let ty = y + (h - ch) / 2.0;
        self.push_text(fgs, label, x + cw, ty, muted);
        let field_x = x + cw * 2.0 + label_w;
        let field_w = (x + w - cw * 2.0 - count_w - field_x).max(cw);
        let fits = ((field_w / cw).floor() as usize).saturating_sub(1).max(1);
        let n = s.query.chars().count();
        let shown: String = s.query.chars().skip(n.saturating_sub(fits)).collect();
        self.push_text(fgs, &shown, field_x, ty, fg);
        let caret_x = (field_x + shown.chars().count() as f32 * cw).round();
        fgs.push(Instance::rect(caret_x, ty, thick * 2.0, ch, fg));
        if !count.is_empty() {
            let color = if s.matches.is_empty() { pal.ansi[1] } else { muted };
            let cx = x + w - cw - count.chars().count() as f32 * cw;
            self.push_text(fgs, &count, cx, ty, color);
        }
    }
}

fn hit_span(r: Option<(f32, f32)>, x: f64) -> bool {
    let Some((sx, sw)) = r else { return false };
    let x = x as f32;
    x >= sx && x < sx + sw
}
