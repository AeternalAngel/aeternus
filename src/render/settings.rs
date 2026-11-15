use std::time::{Duration, Instant};

use winit::event::KeyEvent;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::{hex_str, hsv_to_rgb, luma, mix, outline, rgb_to_hsv, square_outline, Instance, Renderer};
use crate::config::Config;
use crate::term::CursorShape;

const TOGGLE_ANIM: f32 = 0.12;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SettingsOutcome {
    Saved,
    Cancelled,
}
#[derive(Clone, Copy, PartialEq)]
enum FieldKey {
    FontFamily,
    FontSize,
    LineHeight,
    WinWidth,
    WinHeight,
    Padding,
    Opacity,
    Toolbar,
    Shell,
    WorkDir,
    Scrollback,
    Osc52,
    SmartCopy,
    VisualBell,
    CursorShape,
    CursorBlink,
    CursorTrail,
    Foreground,
    Background,
    Selection,
    CursorColor,
}
#[derive(Clone, Copy)]
enum FieldKind {
    Text,
    Number,
    Toggle,
    Color,
    Choice(&'static [&'static str]),
}
struct FieldSpec {
    key: FieldKey,
    label: &'static str,
    hint: &'static str,
    simple: bool,
    kind: FieldKind,
}
const fn spec(
    key: FieldKey,
    label: &'static str,
    hint: &'static str,
    simple: bool,
    kind: FieldKind,
) -> FieldSpec {
    FieldSpec { key, label, hint, simple, kind }
}
const FIELDS: [FieldSpec; 21] = [
    spec(
        FieldKey::FontFamily,
        "Font family",
        "Name of an installed font, or \"monospace\".",
        true,
        FieldKind::Text,
    ),
    spec(FieldKey::FontSize, "Font size", "Logical pixels (4 to 128).", true, FieldKind::Number),
    spec(
        FieldKey::LineHeight,
        "Line height",
        "Multiplier of the font size (1.0 to 3.0).",
        false,
        FieldKind::Number,
    ),
    spec(FieldKey::WinWidth, "Window width", "In pixels; applied on next launch.", false, FieldKind::Number),
    spec(
        FieldKey::WinHeight,
        "Window height",
        "In pixels; applied on next launch.",
        false,
        FieldKind::Number,
    ),
    spec(
        FieldKey::Padding,
        "Padding",
        "Space between the border and the text (0 to 200).",
        false,
        FieldKind::Number,
    ),
    spec(
        FieldKey::Opacity,
        "Background opacity",
        "0.0 (transparent) to 1.0 (opaque); text stays solid.",
        true,
        FieldKind::Number,
    ),
    spec(FieldKey::Toolbar, "Menu bar", "Show the menus at the top of the window.", true, FieldKind::Toggle),
    spec(
        FieldKey::Shell,
        "Shell",
        "Empty uses $SHELL; accepts arguments, e.g. bash -l.",
        false,
        FieldKind::Text,
    ),
    spec(FieldKey::WorkDir, "Working directory", "Empty = current directory.", false, FieldKind::Text),
    spec(
        FieldKey::Scrollback,
        "Scrollback lines",
        "History kept per tab (up to 1,000,000).",
        false,
        FieldKind::Number,
    ),
    spec(
        FieldKey::Osc52,
        "OSC 52 (clipboard)",
        "Programs may copy through OSC 52.",
        false,
        FieldKind::Toggle,
    ),
    spec(
        FieldKey::SmartCopy,
        "Smart Ctrl+C / Ctrl+V",
        "Ctrl+C copies a selection, Ctrl+V pastes.",
        false,
        FieldKind::Toggle,
    ),
    spec(FieldKey::VisualBell, "Visual bell", "Flash the window on BEL.", false, FieldKind::Toggle),
    spec(
        FieldKey::CursorShape,
        "Cursor shape",
        "Click to cycle: block, underline, bar.",
        false,
        FieldKind::Choice(&["block", "underline", "bar"]),
    ),
    spec(FieldKey::CursorBlink, "Blinking cursor", "Blink the cursor while idle.", false, FieldKind::Toggle),
    spec(
        FieldKey::CursorTrail,
        "Cursor trail",
        "Milliseconds of rest before a move animates (0 = off).",
        true,
        FieldKind::Number,
    ),
    spec(FieldKey::Foreground, "Text color", "Hex #rrggbb.", true, FieldKind::Color),
    spec(FieldKey::Background, "Background color", "Hex #rrggbb.", true, FieldKind::Color),
    spec(FieldKey::Selection, "Selection color", "Hex #rrggbb.", true, FieldKind::Color),
    spec(FieldKey::CursorColor, "Cursor color", "Empty = text color. Hex #rrggbb.", true, FieldKind::Color),
];
const SECTIONS: &[(&str, &[FieldKey])] = &[
    ("Font", &[FieldKey::FontFamily, FieldKey::FontSize, FieldKey::LineHeight]),
    (
        "Window",
        &[FieldKey::WinWidth, FieldKey::WinHeight, FieldKey::Padding, FieldKey::Opacity, FieldKey::Toolbar],
    ),
    (
        "Terminal",
        &[
            FieldKey::Shell,
            FieldKey::WorkDir,
            FieldKey::Scrollback,
            FieldKey::Osc52,
            FieldKey::SmartCopy,
            FieldKey::VisualBell,
        ],
    ),
    ("Cursor", &[FieldKey::CursorShape, FieldKey::CursorBlink, FieldKey::CursorTrail]),
    ("Colors", &[FieldKey::Foreground, FieldKey::Background, FieldKey::Selection, FieldKey::CursorColor]),
];
enum FieldValue {
    Text(String),
    Toggle(bool),
    Choice(usize),
}
pub(super) struct SettingsState {
    scroll: f32,
    focus: Option<usize>,
    initial: Vec<Option<String>>,
    draft: Vec<FieldValue>,
    simple: bool,
    picker: Option<PickerState>,
    anims: Vec<(usize, Instant, f32)>,
}
#[derive(Clone, Copy)]
struct PickerState {
    field: usize,
    h: f32,
    s: f32,
    v: f32,
    drag: u8,
}
struct SRow {
    sec: usize,
    fld: Option<usize>,
    y: f32,
    h: f32,
}
struct SettingsGeom {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rows_top: f32,
    rows_bottom: f32,
}
type Rect = (f32, f32, f32, f32);
struct PickerGeom {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    sv: (f32, f32, f32, f32),
    hue: (f32, f32, f32, f32),
    preview: (f32, f32, f32, f32),
    ok: (f32, f32, f32, f32),
    cancel: (f32, f32, f32, f32),
}
fn settings_rows(simple: bool) -> Vec<(usize, Option<usize>)> {
    let mut rows = Vec::new();
    for (si, (_, keys)) in SECTIONS.iter().enumerate() {
        let flds: Vec<Option<usize>> = keys
            .iter()
            .filter_map(|k| FIELDS.iter().position(|f| f.key == *k))
            .filter(|&fi| !simple || FIELDS[fi].simple)
            .map(Some)
            .collect();
        if !flds.is_empty() {
            rows.push((si, None));
            rows.extend(flds.into_iter().map(|f| (si, f)));
        }
    }
    rows
}
fn color_shown(cfg: &Config, key: &str, effective: [u8; 3]) -> String {
    cfg.colors.values.get(key).cloned().unwrap_or_else(|| hex_str(effective))
}
fn in_rect(r: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

impl Renderer {
    pub fn open_settings(&mut self, cfg: &Config) {
        let draft: Vec<FieldValue> = FIELDS
            .iter()
            .map(|f| match f.key {
                FieldKey::FontFamily => FieldValue::Text(cfg.font.family.clone()),
                FieldKey::FontSize => FieldValue::Text(cfg.font.size.to_string()),
                FieldKey::LineHeight => FieldValue::Text(cfg.font.line_height.to_string()),
                FieldKey::WinWidth => FieldValue::Text(cfg.window.width.to_string()),
                FieldKey::WinHeight => FieldValue::Text(cfg.window.height.to_string()),
                FieldKey::Padding => FieldValue::Text(cfg.window.padding.to_string()),
                FieldKey::Opacity => FieldValue::Text(cfg.window.opacity.to_string()),
                FieldKey::Toolbar => FieldValue::Toggle(cfg.window.toolbar),
                FieldKey::Shell => FieldValue::Text(cfg.terminal.shell.clone()),
                FieldKey::WorkDir => FieldValue::Text(cfg.terminal.working_directory.clone()),
                FieldKey::Scrollback => FieldValue::Text(cfg.terminal.scrollback.to_string()),
                FieldKey::Osc52 => FieldValue::Toggle(cfg.terminal.osc52),
                FieldKey::SmartCopy => FieldValue::Toggle(cfg.terminal.smart_copy_paste),
                FieldKey::VisualBell => FieldValue::Toggle(cfg.bell.visual),
                FieldKey::CursorShape => FieldValue::Choice(match cfg.cursor.shape {
                    CursorShape::Block => 0,
                    CursorShape::Underline => 1,
                    CursorShape::Bar => 2,
                }),
                FieldKey::CursorBlink => FieldValue::Toggle(cfg.cursor.blink),
                FieldKey::CursorTrail => FieldValue::Text(cfg.cursor.trail.to_string()),
                FieldKey::Foreground => FieldValue::Text(color_shown(cfg, "foreground", cfg.palette.fg)),
                FieldKey::Background => FieldValue::Text(color_shown(cfg, "background", cfg.palette.bg)),
                FieldKey::Selection => {
                    FieldValue::Text(color_shown(cfg, "selection_background", cfg.palette.selection))
                }
                FieldKey::CursorColor => FieldValue::Text(match cfg.colors.values.get("cursor") {
                    Some(v) => v.clone(),
                    None if cfg.palette.cursor == cfg.palette.fg => String::new(),
                    None => hex_str(cfg.palette.cursor),
                }),
            })
            .collect();
        let initial = draft
            .iter()
            .map(|v| match v {
                FieldValue::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        self.settings = Some(SettingsState {
            scroll: 0.0,
            focus: None,
            initial,
            draft,
            simple: true,
            picker: None,
            anims: Vec::new(),
        });
    }
    pub fn close_settings(&mut self) {
        self.settings = None;
    }
    pub fn settings_open(&self) -> bool {
        self.settings.is_some()
    }
    pub fn picker_release(&mut self) -> bool {
        let Some(s) = self.settings.as_mut() else { return false };
        let Some(pk) = s.picker.as_mut() else { return false };
        if pk.drag == 0 {
            return false;
        }
        pk.drag = 0;
        true
    }
    pub fn settings_anim_wake(&mut self, now: Instant) -> Option<Instant> {
        let s = self.settings.as_mut()?;
        let mut next: Option<Instant> = None;
        s.anims.retain(|&(_, t0, _)| {
            let end = t0 + Duration::from_secs_f32(TOGGLE_ANIM);
            if now >= end {
                false
            } else {
                next = Some(next.map_or(end, |n| n.min(end)));
                true
            }
        });
        next
    }
    pub fn settings_scroll(&mut self, delta: f32) -> bool {
        let Some(st) = self.settings.as_ref() else { return false };
        if st.picker.is_some() {
            return true;
        }
        let simple = st.simple;
        let Some(g) = self.settings_geom() else { return true };
        let content = self.settings_layout(&g, simple, 0.0).1;
        let cell_h = self.cell_h;
        let Some(s) = &mut self.settings else { return false };
        s.scroll = (s.scroll + delta * cell_h).clamp(0.0, (content - (g.rows_bottom - g.rows_top)).max(0.0));
        true
    }
    fn save_cancel_rects(&self, g: &SettingsGeom) -> (Rect, Rect) {
        let (cw, ch) = (self.cell_w, self.cell_h);
        let bw = 10.0 * cw;
        let bh = ch * 1.8;
        let by = g.y + g.h - ch * 0.7 - bh;
        let save_x = g.x + g.w - cw * 1.5 - bw;
        let cancel_x = save_x - cw * 0.75 - bw;
        ((save_x, by, bw, bh), (cancel_x, by, bw, bh))
    }
    pub fn settings_click(&mut self, x: f64, y: f64) -> Option<SettingsOutcome> {
        let g = self.settings_geom()?;
        let ch = self.cell_h;
        let (x, y) = (x as f32, y as f32);
        if self.settings.as_ref().is_some_and(|s| s.picker.is_some()) {
            self.picker_click(x, y);
            return None;
        }
        if !in_rect((g.x, g.y, g.w, g.h), x, y) {
            return Some(SettingsOutcome::Cancelled);
        }
        let simple = self.settings.as_ref().is_none_or(|s| s.simple);
        let scroll = self.settings.as_ref().map_or(0.0, |s| s.scroll);
        let rows = self.settings_layout(&g, simple, scroll).0;
        let (sx, sy, sw, _) = self.settings_seg(&g);
        let (save, cancel) = self.save_cancel_rects(&g);
        let s = self.settings.as_mut()?;
        if in_rect(save, x, y) {
            return Some(SettingsOutcome::Saved);
        }
        if in_rect(cancel, x, y) {
            return Some(SettingsOutcome::Cancelled);
        }
        if in_rect((sx, sy, 2.0 * sw, ch * 1.7), x, y) {
            let simple_now = x < sx + sw;
            if simple_now != s.simple {
                s.simple = simple_now;
                s.scroll = 0.0;
                s.focus = None;
            }
            return None;
        }
        if y < g.rows_top || y >= g.rows_bottom {
            return None;
        }
        let row = rows.iter().find(|r| y >= r.y && y < r.y + r.h)?;
        let fi = row.fld?;
        s.focus = Some(fi);
        match FIELDS[fi].kind {
            FieldKind::Toggle => {
                if let FieldValue::Toggle(v) = &mut s.draft[fi] {
                    let from = if *v { 1.0 } else { 0.0 };
                    *v = !*v;
                    s.anims.retain(|a| a.0 != fi);
                    s.anims.push((fi, Instant::now(), from));
                }
            }
            FieldKind::Choice(opts) => {
                if let FieldValue::Choice(i) = &mut s.draft[fi] {
                    *i = (*i + 1) % opts.len();
                }
            }
            FieldKind::Color => {
                let cur = match &s.draft[fi] {
                    FieldValue::Text(t) => crate::colors::parse_hex(t).unwrap_or([255, 255, 255]),
                    _ => [255, 255, 255],
                };
                let (h, sa, v) = rgb_to_hsv(cur);
                s.picker = Some(PickerState { field: fi, h, s: sa, v, drag: 0 });
            }
            FieldKind::Text | FieldKind::Number => {}
        }
        None
    }
    pub fn settings_key(&mut self, ev: &KeyEvent, mods: ModifiersState) -> Option<SettingsOutcome> {
        let s = self.settings.as_mut()?;
        if let Some(pk) = s.picker {
            match &ev.logical_key {
                Key::Named(NamedKey::Escape) => s.picker = None,
                Key::Named(NamedKey::Enter) => {
                    if let FieldValue::Text(t) = &mut s.draft[pk.field] {
                        *t = hex_str(hsv_to_rgb(pk.h, pk.s, pk.v));
                    }
                    s.picker = None;
                }
                _ => {}
            }
            return None;
        }
        match &ev.logical_key {
            Key::Named(NamedKey::Escape) => return Some(SettingsOutcome::Cancelled),
            Key::Named(NamedKey::Enter) => return Some(SettingsOutcome::Saved),
            Key::Named(NamedKey::Tab) => {
                let rows: Vec<usize> = settings_rows(s.simple).into_iter().filter_map(|(_, f)| f).collect();
                if rows.is_empty() {
                    return None;
                }
                let cur = s.focus.and_then(|f| rows.iter().position(|&r| r == f));
                let next = match (cur, mods.shift_key()) {
                    (None, false) => 0,
                    (None, true) => rows.len() - 1,
                    (Some(i), false) => (i + 1) % rows.len(),
                    (Some(i), true) => (i + rows.len() - 1) % rows.len(),
                };
                s.focus = Some(rows[next]);
                return None;
            }
            _ => {}
        }
        let idx = s.focus?;
        if !matches!(FIELDS[idx].kind, FieldKind::Text | FieldKind::Number | FieldKind::Color) {
            if let (Key::Named(NamedKey::Space), FieldValue::Toggle(v)) = (&ev.logical_key, &mut s.draft[idx])
            {
                let from = if *v { 1.0 } else { 0.0 };
                *v = !*v;
                s.anims.retain(|a| a.0 != idx);
                s.anims.push((idx, Instant::now(), from));
            }
            return None;
        }
        let FieldValue::Text(text) = &mut s.draft[idx] else { return None };
        match &ev.logical_key {
            Key::Named(NamedKey::Backspace) => {
                if mods.control_key() {
                    text.clear();
                } else {
                    text.pop();
                }
            }
            Key::Named(NamedKey::Space) => text.push(' '),
            Key::Named(_) => {}
            Key::Character(c) => {
                if !mods.control_key() && !mods.alt_key() && !mods.super_key() {
                    text.push_str(c.as_str());
                }
            }
            Key::Dead(_) | Key::Unidentified(_) => {
                if let Some(t) = &ev.text {
                    text.push_str(t);
                }
            }
        }
        None
    }
    pub fn settings_paste(&mut self, pasted: &str) {
        let Some(s) = self.settings.as_mut() else { return };
        let Some(idx) = s.focus else { return };
        if let FieldValue::Text(text) = &mut s.draft[idx] {
            text.push_str(pasted.lines().next().unwrap_or(""));
        }
    }
    pub fn settings_writeback(&self, cfg: &mut Config) {
        let Some(s) = &self.settings else { return };
        for (i, spec) in FIELDS.iter().enumerate() {
            match &s.draft[i] {
                FieldValue::Toggle(v) => match spec.key {
                    FieldKey::Toolbar => cfg.window.toolbar = *v,
                    FieldKey::Osc52 => cfg.terminal.osc52 = *v,
                    FieldKey::SmartCopy => cfg.terminal.smart_copy_paste = *v,
                    FieldKey::VisualBell => cfg.bell.visual = *v,
                    FieldKey::CursorBlink => cfg.cursor.blink = *v,
                    _ => {}
                },
                FieldValue::Choice(idx) => {
                    if spec.key == FieldKey::CursorShape {
                        cfg.cursor.shape = match idx {
                            1 => CursorShape::Underline,
                            2 => CursorShape::Bar,
                            _ => CursorShape::Block,
                        };
                    }
                }
                FieldValue::Text(text) => {
                    let text = text.trim();
                    let num = |t: &str| t.parse::<f64>().ok().filter(|v| v.is_finite());
                    match spec.key {
                        FieldKey::FontFamily => cfg.font.family = text.to_string(),
                        FieldKey::FontSize => {
                            if let Some(v) = num(text) {
                                cfg.font.size = v as f32;
                            }
                        }
                        FieldKey::LineHeight => {
                            if let Some(v) = num(text) {
                                cfg.font.line_height = v as f32;
                            }
                        }
                        FieldKey::WinWidth => {
                            if let Ok(v) = text.parse::<u32>() {
                                cfg.window.width = v;
                            }
                        }
                        FieldKey::WinHeight => {
                            if let Ok(v) = text.parse::<u32>() {
                                cfg.window.height = v;
                            }
                        }
                        FieldKey::Padding => {
                            if let Some(v) = num(text) {
                                cfg.window.padding = v as f32;
                            }
                        }
                        FieldKey::Opacity => {
                            if let Some(v) = num(text) {
                                cfg.window.opacity = v.clamp(0.0, 1.0) as f32;
                            }
                        }
                        FieldKey::Shell => cfg.terminal.shell = text.to_string(),
                        FieldKey::WorkDir => cfg.terminal.working_directory = text.to_string(),
                        FieldKey::Scrollback => {
                            if let Ok(v) = text.parse::<usize>() {
                                cfg.terminal.scrollback = v;
                            }
                        }
                        FieldKey::CursorTrail => {
                            if let Ok(v) = text.parse::<u32>() {
                                cfg.cursor.trail = v;
                            }
                        }
                        FieldKey::Foreground
                        | FieldKey::Background
                        | FieldKey::Selection
                        | FieldKey::CursorColor
                            if s.initial[i].as_deref().map(str::trim) != Some(text) =>
                        {
                            let key = match spec.key {
                                FieldKey::Foreground => "foreground",
                                FieldKey::Background => "background",
                                FieldKey::Selection => "selection_background",
                                _ => "cursor",
                            };
                            if text.is_empty() {
                                cfg.colors.values.remove(key);
                            } else {
                                cfg.colors.values.insert(key.to_string(), text.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    fn settings_geom(&self) -> Option<SettingsGeom> {
        self.settings.as_ref()?;
        let (cw, ch) = (self.cell_w, self.cell_h);
        let pad = self.pad + cw;
        let x = pad;
        let y = self.top_inset() + self.pad;
        let w = self.config.width as f32 - 2.0 * pad;
        let h = self.config.height as f32 - y - self.pad;
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        Some(SettingsGeom { x, y, w, h, rows_top: y + ch * 3.0, rows_bottom: y + h - ch * 2.8 })
    }
    fn settings_layout(&self, g: &SettingsGeom, simple: bool, scroll: f32) -> (Vec<SRow>, f32) {
        let ch = self.cell_h;
        let sec_h = (ch * 2.4).round();
        let row_h = (ch * 2.6).round();
        let gap = (ch * 0.8).round();
        let mut rows = Vec::new();
        let mut y = g.rows_top;
        let mut prev_fld = false;
        for (si, fld) in settings_rows(simple) {
            if fld.is_none() && prev_fld {
                y += gap;
            }
            let h = if fld.is_none() { sec_h } else { row_h };
            rows.push(SRow { sec: si, fld, y: y - scroll, h });
            y += h;
            prev_fld = fld.is_some();
        }
        (rows, y + gap - g.rows_top)
    }
    fn settings_seg(&self, g: &SettingsGeom) -> (f32, f32, f32, f32) {
        let (cw, ch) = (self.cell_w, self.cell_h);
        let w = 8.5 * cw;
        (g.x + g.w - cw * 1.5 - 2.0 * w, g.y + ch * 0.35, w, ch * 1.7)
    }
    fn picker_geom(&self) -> Option<PickerGeom> {
        self.settings.as_ref()?.picker.as_ref()?;
        let (cw, ch) = (self.cell_w, self.cell_h);
        let w = self.config.width as f32;
        let h = self.config.height as f32;
        let pw = (30.0 * cw).min(w - 2.0 * self.pad).max(20.0 * cw);
        let ph = (ch * 15.5).min(h - 2.0 * self.pad);
        if ph < ch * 10.0 {
            return None;
        }
        let x = ((w - pw) / 2.0).floor();
        let y = ((h - ph) / 2.0).floor();
        let inner_x = x + cw;
        let inner_w = pw - 2.0 * cw;
        Some(PickerGeom {
            x,
            y,
            w: pw,
            h: ph,
            sv: (inner_x, y + ch * 1.9, inner_w, ch * 6.5),
            hue: (inner_x, y + ch * 9.0, inner_w, ch * 1.5),
            preview: (inner_x, y + ch * 11.2, inner_w, ch * 1.9),
            ok: (x + pw - cw - 10.0 * cw, y + ph - ch * 2.2, 10.0 * cw, ch * 1.7),
            cancel: (x + pw - cw - 21.0 * cw, y + ph - ch * 2.2, 10.0 * cw, ch * 1.7),
        })
    }
    fn picker_click(&mut self, x: f32, y: f32) {
        let Some(g) = self.picker_geom() else { return };
        let Some(s) = self.settings.as_mut() else { return };
        let Some(pk) = s.picker.as_mut() else { return };
        let (sx, sy, sw, sh) = g.sv;
        if in_rect(g.sv, x, y) {
            pk.s = ((x - sx) / sw).clamp(0.0, 1.0);
            pk.v = 1.0 - ((y - sy) / sh).clamp(0.0, 1.0);
            pk.drag = 1;
            return;
        }
        let (hx, _, hw, _) = g.hue;
        if in_rect(g.hue, x, y) {
            pk.h = ((x - hx) / hw * 360.0).clamp(0.0, 359.99);
            pk.drag = 2;
            return;
        }
        if in_rect(g.ok, x, y) {
            let pk = *pk;
            if let FieldValue::Text(t) = &mut s.draft[pk.field] {
                *t = hex_str(hsv_to_rgb(pk.h, pk.s, pk.v));
            }
            s.picker = None;
            return;
        }
        if in_rect(g.cancel, x, y) || !in_rect((g.x, g.y, g.w, g.h), x, y) {
            s.picker = None;
        }
    }
    pub fn settings_move(&mut self, x: f64, y: f64) -> bool {
        let Some(g) = self.picker_geom() else { return false };
        let Some(pk) = self.settings.as_mut().and_then(|s| s.picker.as_mut()) else {
            return false;
        };
        match pk.drag {
            1 => {
                let (sx, sy, sw, sh) = g.sv;
                pk.s = ((x as f32 - sx) / sw).clamp(0.0, 1.0);
                pk.v = 1.0 - ((y as f32 - sy) / sh).clamp(0.0, 1.0);
                true
            }
            2 => {
                let (hx, _, hw, _) = g.hue;
                pk.h = ((x as f32 - hx) / hw * 360.0).clamp(0.0, 359.99);
                true
            }
            _ => false,
        }
    }
    pub(super) fn push_settings(&mut self, fgs: &mut Vec<Instance>, base: [u8; 3], fg: [u8; 3], thick: f32) {
        let Some(g) = self.settings_geom() else { return };
        let Some(st) = self.settings.take() else { return };
        let (cw, ch) = (self.cell_w, self.cell_h);
        fgs.push(Instance::rect(g.x, g.y, g.w, g.h, mix(base, fg, 0.07)));
        outline(fgs, g.x, g.y, g.w, g.h, thick, mix(base, fg, 0.35));
        self.push_text(fgs, "Preferences", g.x + cw * 1.5, g.y + ch * 0.55, fg);
        self.push_text(
            fgs,
            "saved to config.toml and applied at once; Esc cancels, Enter saves",
            g.x + cw * 1.5,
            g.y + ch * 1.75,
            mix(base, fg, 0.5),
        );
        let (sx, sy, sw, sh) = self.settings_seg(&g);
        let on = (mix(base, fg, 0.22), base);
        let off = (mix(base, fg, 0.06), mix(base, fg, 0.55));
        let ((bg1, tx1), (bg2, tx2)) = if st.simple { (on, off) } else { (off, on) };
        fgs.push(Instance::rect(sx, sy, sw, sh, bg1));
        self.push_text(fgs, "Simple", sx + cw * 1.25, sy + (sh - ch) / 2.0, tx1);
        fgs.push(Instance::rect(sx + sw, sy, sw, sh, bg2));
        self.push_text(fgs, "Advanced", sx + sw + cw * 0.25, sy + (sh - ch) / 2.0, tx2);
        let rows = self.settings_layout(&g, st.simple, st.scroll).0;
        let card_pad = (ch * 0.5).round();
        let mut i = 0;
        while i < rows.len() {
            let sec = rows[i].sec;
            let mut j = i;
            while j < rows.len() && rows[j].sec == sec {
                j += 1;
            }
            let y0 = (rows[i].y - card_pad).max(g.rows_top);
            let y1 = (rows[j - 1].y + rows[j - 1].h + card_pad).min(g.rows_bottom);
            if y1 > y0 {
                let cx0 = g.x + cw * 0.75;
                let cwd = g.w - cw * 1.5;
                fgs.push(Instance::rect(cx0, y0, cwd, y1 - y0, mix(base, fg, 0.035)));
                outline(fgs, cx0, y0, cwd, y1 - y0, thick, mix(base, fg, 0.12));
                let sec_y = rows[i].y + ch * 0.45;
                if sec_y >= g.rows_top && sec_y + ch <= g.rows_bottom {
                    self.push_text(fgs, SECTIONS[sec].0, g.x + cw * 1.75, sec_y, mix(base, fg, 0.6));
                }
            }
            i = j;
        }
        let box_w = (22.0 * cw).min((g.w - cw * 4.0).max(6.0 * cw));
        let in_rows = |y: f32, h: f32| y >= g.rows_top && y + h <= g.rows_bottom;
        for r in &rows {
            let Some(fi) = r.fld else { continue };
            let spec = &FIELDS[fi];
            let focused = st.focus == Some(fi);
            let label_y = r.y + ch * 0.35;
            let hint_y = r.y + ch * 1.45;
            if in_rows(label_y, ch) {
                self.push_text(fgs, spec.label, g.x + cw * 1.75, label_y, fg);
            }
            if in_rows(hint_y, ch) {
                self.push_text(fgs, spec.hint, g.x + cw * 1.75, hint_y, mix(base, fg, 0.45));
            }
            let box_x = g.x + g.w - cw * 1.75 - box_w;
            let box_y = label_y - thick;
            let box_h = ch + 2.0 * thick;
            let bcol = if focused { mix(base, fg, 0.55) } else { mix(base, fg, 0.28) };
            match (&st.draft[fi], spec.kind) {
                (FieldValue::Text(t), FieldKind::Color) => {
                    if !in_rows(box_y, box_h) {
                        continue;
                    }
                    let col = crate::colors::parse_hex(t).unwrap_or_else(|| mix(base, fg, 0.10));
                    fgs.push(Instance::rect(box_x, box_y, box_w, box_h, col));
                    if focused {
                        outline(fgs, box_x, box_y, box_w, box_h, thick, bcol);
                    }
                    let tcol = if luma(col) > 140 { [0, 0, 0] } else { [255, 255, 255] };
                    let shown: String = t.chars().take((box_w / cw) as usize).collect();
                    self.push_text(
                        fgs,
                        &shown,
                        box_x + (box_w - shown.chars().count() as f32 * cw) / 2.0,
                        label_y,
                        tcol,
                    );
                }
                (FieldValue::Text(t), _) => {
                    if !in_rows(box_y, box_h) {
                        continue;
                    }
                    fgs.push(Instance::rect(box_x, box_y, box_w, box_h, mix(base, fg, 0.03)));
                    outline(fgs, box_x, box_y, box_w, box_h, thick, bcol);
                    let max = ((box_w - 2.0 * cw) / cw).max(0.0) as usize;
                    let count = t.chars().count();
                    let shown: String = t.chars().skip(count.saturating_sub(max)).collect();
                    self.push_text(fgs, &shown, box_x + cw * 0.5, label_y, fg);
                    if focused {
                        let cx = box_x + cw * 0.5 + shown.chars().count() as f32 * cw;
                        fgs.push(Instance::rect(cx, label_y + thick, thick, ch - 2.0 * thick, fg));
                    }
                }
                (FieldValue::Toggle(v), _) => {
                    if !in_rows(label_y, ch) {
                        continue;
                    }
                    let pill_w = 4.0 * cw;
                    let pill_x = box_x;
                    let pill_y = label_y;
                    let target = if *v { 1.0f32 } else { 0.0 };
                    let mut pos = target;
                    if let Some(&(_, t0, from)) = st.anims.iter().find(|a| a.0 == fi) {
                        let p = ((Instant::now() - t0).as_secs_f32() / TOGGLE_ANIM).clamp(0.0, 1.0);
                        let e = p * p * (3.0 - 2.0 * p);
                        pos = from + (target - from) * e;
                    }
                    let pill_bg = mix(mix(base, fg, 0.12), mix(base, fg, 0.60), pos);
                    let knob_col = mix(mix(base, fg, 0.55), base, pos);
                    fgs.push(Instance::rect(pill_x, pill_y, pill_w, ch, pill_bg));
                    let rim = if focused { mix(base, fg, 0.7) } else { mix(base, fg, 0.32) };
                    outline(fgs, pill_x, pill_y, pill_w, ch, thick, rim);
                    let knob = (ch - 2.0 * thick).max(2.0);
                    let knob_x = pill_x + thick + pos * (pill_w - 2.0 * thick - knob).max(0.0);
                    fgs.push(Instance::rect(knob_x, pill_y + thick, knob, knob, knob_col));
                }
                (FieldValue::Choice(ci), FieldKind::Choice(opts)) => {
                    if !in_rows(box_y, box_h) {
                        continue;
                    }
                    fgs.push(Instance::rect(box_x, box_y, box_w, box_h, mix(base, fg, 0.03)));
                    outline(fgs, box_x, box_y, box_w, box_h, thick, bcol);
                    self.push_text(fgs, opts[*ci], box_x + cw * 0.5, label_y, fg);
                }
                (FieldValue::Choice(_), _) => {}
            }
        }
        let (save, cancel) = self.save_cancel_rects(&g);
        fgs.push(Instance::rect(save.0, save.1, save.2, save.3, mix(base, fg, 0.22)));
        self.push_text(fgs, "Save", save.0 + cw * 3.0, save.1 + (save.3 - ch) / 2.0, base);
        fgs.push(Instance::rect(cancel.0, cancel.1, cancel.2, cancel.3, mix(base, fg, 0.08)));
        self.push_text(
            fgs,
            "Cancel",
            cancel.0 + cw * 2.0,
            cancel.1 + (cancel.3 - ch) / 2.0,
            mix(base, fg, 0.55),
        );
        let has_picker = st.picker.is_some();
        self.settings = Some(st);
        if has_picker {
            self.push_picker(fgs, base, fg, thick);
        }
    }
    fn push_picker(&mut self, fgs: &mut Vec<Instance>, base: [u8; 3], fg: [u8; 3], thick: f32) {
        let Some(g) = self.picker_geom() else { return };
        let Some(pk) = self.settings.as_ref().and_then(|s| s.picker) else { return };
        let (cw, ch) = (self.cell_w, self.cell_h);
        fgs.push(Instance::rect(g.x, g.y, g.w, g.h, mix(base, fg, 0.09)));
        outline(fgs, g.x, g.y, g.w, g.h, thick, mix(base, fg, 0.45));
        self.push_text(fgs, "Pick a color", g.x + cw, g.y + ch * 0.55, fg);
        let new = hsv_to_rgb(pk.h, pk.s, pk.v);
        let (sx, sy, sw, sh) = g.sv;
        let (cols, rown) = (24usize, 12usize);
        for r in 0..rown {
            for c in 0..cols {
                let col = hsv_to_rgb(pk.h, c as f32 / cols as f32, 1.0 - (r as f32 + 0.5) / rown as f32);
                fgs.push(Instance::rect(
                    sx + c as f32 * sw / cols as f32,
                    sy + r as f32 * sh / rown as f32,
                    (sw / cols as f32).ceil() + 1.0,
                    (sh / rown as f32).ceil() + 1.0,
                    col,
                ));
            }
        }
        let mx = sx + pk.s * sw;
        let my = sy + (1.0 - pk.v) * sh;
        let mk = (ch * 0.9).round().max(thick * 4.0);
        square_outline(fgs, mx - mk / 2.0 - thick, my - mk / 2.0 - thick, mk + 2.0 * thick, thick, [0, 0, 0]);
        square_outline(fgs, mx - mk / 2.0, my - mk / 2.0, mk, thick, [255, 255, 255]);
        let (hx, hy, hw, hh) = g.hue;
        for c in 0..24usize {
            let col = hsv_to_rgb(c as f32 * 15.0, 1.0, 1.0);
            fgs.push(Instance::rect(hx + c as f32 * hw / 24.0, hy, (hw / 24.0).ceil() + 1.0, hh, col));
        }
        let hxx = hx + pk.h / 360.0 * hw;
        let hcy = hy + hh / 2.0 - mk / 2.0;
        square_outline(fgs, hxx - mk / 2.0 - thick, hcy - thick, mk + 2.0 * thick, thick, [0, 0, 0]);
        square_outline(fgs, hxx - mk / 2.0, hcy, mk, thick, [255, 255, 255]);
        let (px, py, pw2, ph2) = g.preview;
        let half = (pw2 - thick) / 2.0;
        let cur = match self.settings.as_ref().and_then(|s| s.draft.get(pk.field)) {
            Some(FieldValue::Text(t)) => crate::colors::parse_hex(t).unwrap_or(base),
            _ => base,
        };
        fgs.push(Instance::rect(px, py, half, ph2, cur));
        fgs.push(Instance::rect(px + half + thick, py, half, ph2, new));
        let ink = |c: [u8; 3]| if luma(c) > 140 { [0, 0, 0] } else { [255, 255, 255] };
        self.push_text(fgs, "current", px + (half - 7.0 * cw) / 2.0, py + (ph2 - ch) / 2.0, ink(cur));
        self.push_text(
            fgs,
            "new",
            px + half + thick + (half - 3.0 * cw) / 2.0,
            py + (ph2 - ch) / 2.0,
            ink(new),
        );
        self.push_text(fgs, &format!("Hex {}", hex_str(new)), g.x + cw, g.y + ch * 13.55, fg);
        let rgb = format!("RGB {}, {}, {}", new[0], new[1], new[2]);
        self.push_text(fgs, &rgb, g.x + cw * 13.0, g.y + ch * 13.55, mix(base, fg, 0.6));
        let (ox, oy, ow, oh) = g.ok;
        fgs.push(Instance::rect(ox, oy, ow, oh, mix(base, fg, 0.25)));
        self.push_text(fgs, "Confirm", ox + cw * 1.5, oy + (oh - ch) / 2.0, base);
        let (cx, cy, cw2, ch2) = g.cancel;
        fgs.push(Instance::rect(cx, cy, cw2, ch2, mix(base, fg, 0.10)));
        self.push_text(fgs, "Cancel", cx + cw * 2.0, cy + (ch2 - ch) / 2.0, mix(base, fg, 0.55));
    }
}
