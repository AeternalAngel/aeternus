#![allow(dead_code)]
mod bench;
mod cell;
mod colors;
mod config;
mod grid;
mod parser;
mod pty;
mod render;
mod term;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use arboard::{Clipboard, GetExtLinux, LinuxClipboardKind, SetExtLinux};
type ClipKind = LinuxClipboardKind;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, KeyLocation, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Fullscreen, Window, WindowId};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::platform::x11::WindowAttributesExtX11;
use config::{Action as KeyAction, Config, KeyCombo, Keymap};
use parser::Parser;
use pty::Pty;
use render::{Action, Renderer, SettingsOutcome, View};
use term::{MouseMode, SelMode, Term};
const BLINK_INTERVAL: Duration = Duration::from_millis(530);
const MULTI_CLICK: Duration = Duration::from_millis(400);
const ABOUT_BLOB: [&str; 11] = [
    "@@@@@@@@@@@@@@@@@@@@",
    "#@@@@@@@@@@@@@@@@@@#",
    "@@@@@@@@@@@@@@@@@@@@",
    "@@@@..@@@@@@@@@@@@@@",
    "@@@@@@...@@@@@@@@@@@",
    "@@@@@@@@...@@@@@@@@@",
    "@@@@@@...@@@@@@@@@@@",
    "@@@@..@@@@@#====@@@@",
    "@@@@@@@@@@@@@@@@@@@@",
    "#@@@@@@@@@@@@@@@@@@#",
    "@@@@@@@@@@@@@@@@@@@@",
];
const ABOUT_LOGO: [&str; 6] = [
    r"     _        _                            ",
    r"    / \   ___| |_ ___ _ __ _ __  _   _ ___ ",
    r"   / _ \ / _ \ __/ _ \ '__| '_ \| | | / __|",
    r"  / ___ \  __/ ||  __/ |  | | | | |_| \__ \",
    r" /_/   \_\___\__\ ___|_|  |_| |_|\__,_|___/",
    ""
];

fn sgr_run(line: &mut String, text: &str, col: [u8; 3]) {
    if text.is_empty() {
        return;
    }
    line.push_str(&format!("\x1b[38;2;{};{};{}m{}", col[0], col[1], col[2], text));
}

fn about_art() -> String {
    const INK: [u8; 3] = [2, 4, 6];
    const SENTINEL: [u8; 3] = [1, 2, 3];
    let blob_w = ABOUT_BLOB.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let logo_w = ABOUT_LOGO.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let top = (ABOUT_BLOB.len() - ABOUT_LOGO.len()) / 2;
    let mut out = String::from("\r\n");
    for r in 0..ABOUT_BLOB.len() {
        let ver = format!("v{}", env!("CARGO_PKG_VERSION"));
        let blob = format!("{:<blob_w$}", ABOUT_BLOB[r], blob_w = blob_w);
        let logo = match r.checked_sub(top).and_then(|i| ABOUT_LOGO.get(i)) {
            Some(l) => format!("{:<logo_w$}", l, logo_w = logo_w),
            None if r == top + ABOUT_LOGO.len() => {
                format!("{:^logo_w$}", ver, logo_w = logo_w)
            }
            None => " ".repeat(logo_w),
        };
        let mut line = String::new();
        for (text, colored) in [(&blob, true), (&logo, false)] {
            let mut run = String::new();
            let mut cur: Option<[u8; 3]> = None;
            for c in text.chars() {
                let col = if colored && (c == '@' || c == '#') { SENTINEL } else { INK };
                if cur == Some(col) {
                    run.push(c);
                } else {
                    sgr_run(&mut line, &run, cur.unwrap_or(INK));
                    run.clear();
                    run.push(c);
                    cur = Some(col);
                }
            }
            sgr_run(&mut line, &run, cur.unwrap_or(INK));
        }
        let trimmed = line.trim_end();
        out.push_str(trimmed);
        out.push_str("\r\n");
    }
    out
}
const CONFIG_POLL_INTERVAL: Duration = Duration::from_millis(1000);

#[derive(Debug)]
enum UserEvent {
    Wakeup(u64),
    CloseTab(u64),
    Aet(AetCmd, UnixStream),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AetCmd {
    Version,
    Pref,
    Reload,
}

fn parse_aet(line: &[u8]) -> Option<AetCmd> {
    let s = std::str::from_utf8(line).ok()?.trim();
    match s {
        "version" => Some(AetCmd::Version),
        "pref" => Some(AetCmd::Pref),
        "reload" => Some(AetCmd::Reload),
        _ => None,
    }
}

fn aet_tag() -> String {
    let d = std::env::var("WAYLAND_DISPLAY")
        .or_else(|_| std::env::var("DISPLAY"))
        .unwrap_or_else(|_| String::from("default"));
    d.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

fn aet_paths() -> (PathBuf, PathBuf) {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let tag = aet_tag();
    (
        dir.join(format!("aeternus-{tag}-{}.sock", std::process::id())),
        dir.join(format!("aeternus-{tag}.last")),
    )
}

fn aet_listener(l: UnixListener, proxy: EventLoopProxy<UserEvent>) {
    for conn in l.incoming() {
        let Ok(mut s) = conn else { break };
        let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
        let Ok(r) = s.try_clone() else { continue };
        let mut line = String::new();
        let mut r = BufReader::new(r);
        if r.read_line(&mut line).is_err() {
            continue;
        }
        match parse_aet(line.as_bytes()) {
            Some(cmd) => {
                let _ = proxy.send_event(UserEvent::Aet(cmd, s));
            }
            None => {
                let _ = s.write_all(b"err comando desconhecido\n");
            }
        }
    }
}

fn aet_cleanup(sock: &PathBuf, last: &PathBuf) {
    let _ = std::fs::remove_file(sock);
    let mine = sock.to_string_lossy().to_string();
    if std::fs::read_to_string(last).map_or(false, |c| c.trim() == mine) {
        let _ = std::fs::remove_file(last);
    }
}

fn aet_bind(app: &mut App) {
    let (sock, last) = aet_paths();
    let _ = std::fs::remove_file(&sock);
    let Ok(l) = UnixListener::bind(&sock) else { return };
    let _ = std::fs::write(&last, sock.to_string_lossy().as_bytes());
    let proxy = app.proxy.clone();
    app.sock = Some((sock, last));
    thread::spawn(move || aet_listener(l, proxy));
}
struct Tab {
    id: u64,
    term: Arc<Mutex<Term>>,
    pty: Pty,
    dirty: Arc<AtomicBool>,
    want_lock: Arc<AtomicBool>,
}
struct State {
    window: Arc<Window>,
    renderer: Renderer,
    proxy: EventLoopProxy<UserEvent>,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    last_title: String,
    cfg: Config,
    scroll_accum: f32,
    clipboard: Option<Clipboard>,
    focused: bool,
    blink_on: bool,
    last_blink: Instant,
    mouse_pos: (f64, f64),
    mouse_cell: (usize, usize),
    selecting: bool,
    report_button: Option<u8>,
    last_reported: Option<(usize, usize)>,
    last_click: Instant,
    last_click_cell: (usize, usize),
    click_count: u8,
    sock: Option<(PathBuf, PathBuf)>,
    keymap: Keymap,
    config_mtime: Option<std::time::SystemTime>,
    last_config_check: Instant,
}
fn reader_loop(
    id: u64,
    mut reader: Box<dyn Read + Send>,
    term: Arc<Mutex<Term>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    dirty: Arc<AtomicBool>,
    want_lock: Arc<AtomicBool>,
    proxy: EventLoopProxy<UserEvent>,
) {
    let mut parser = Parser::new();
    let mut buf = vec![0u8; 65536];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                while want_lock.load(Ordering::Relaxed) {
                    thread::yield_now();
                }
                let replies = {
                    let mut term = term.lock().unwrap();
                    parser.advance(&mut *term, &buf[..n]);
                    std::mem::take(&mut term.replies)
                };
                if !replies.is_empty() {
                    let _ = writer.lock().unwrap().write_all(&replies);
                }
                if !dirty.swap(true, Ordering::AcqRel) {
                    let _ = proxy.send_event(UserEvent::Wakeup(id));
                }
            }
        }
    }
    let _ = proxy.send_event(UserEvent::CloseTab(id));
}
impl State {
    fn new(el: &ActiveEventLoop, proxy: EventLoopProxy<UserEvent>) -> Self {
        let cfg = config::load().unwrap_or_else(|e| {
            config::report_error(&format!("aeternus: erro na configuração, usando os padrões:\n{e}"));
            Config::default()
        });
        let attrs = Window::default_attributes()
            .with_title("aeternus")
            .with_inner_size(LogicalSize::new(
                cfg.window.width as f64,
                cfg.window.height as f64,
            ))
            .with_decorations(true);
        let attrs = WindowAttributesExtWayland::with_name(attrs, "aeternus", "aeternus");
        let attrs = WindowAttributesExtX11::with_name(attrs, "aeternus", "aeternus");
        let window = Arc::new(el.create_window(attrs).expect("janela"));
        window.set_cursor(CursorIcon::Text);
        let renderer = Renderer::new(window.clone(), &cfg);
        let now = Instant::now();
        let keymap = cfg.keymap.clone();
        let config_mtime = config::mtime();
        let mut state = Self {
            window,
            renderer,
            proxy,
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            last_title: String::new(),
            cfg,
            scroll_accum: 0.0,
            clipboard: Clipboard::new().ok(),
            focused: true,
            blink_on: true,
            last_blink: now,
            mouse_pos: (0.0, 0.0),
            mouse_cell: (0, 0),
            selecting: false,
            report_button: None,
            last_reported: None,
            last_click: now,
            last_click_cell: (usize::MAX, usize::MAX),
            click_count: 0,
            sock: None,
            keymap,
            config_mtime,
            last_config_check: now,
        };
        state.create_tab();
        if state.tabs.is_empty() {
            eprintln!("aeternus: não foi possível abrir o shell");
            std::process::exit(1);
        }
        state.window.request_redraw();
        state
    }
    fn term(&self) -> Arc<Mutex<Term>> {
        self.tabs[self.active].term.clone()
    }
    fn write(&mut self, bytes: &[u8]) {
        let _ = self.tabs[self.active].pty.writer.lock().unwrap().write_all(bytes);
    }
    fn aet_handle(&mut self, cmd: AetCmd) -> Result<(), String> {
        match cmd {
            AetCmd::Version => {
                let term = self.term();
                let mut t = term.lock().unwrap();
                t.scroll_to_bottom();
                t.selection_clear();
                Parser::new().advance(&mut *t, b"aeternus 1.4.0\r\n");
                drop(t);
                self.window.request_redraw();
                Ok(())
            }
            AetCmd::Pref => {
                self.run_action(Action::Settings);
                Ok(())
            }
            AetCmd::Reload => {
                self.run_action(Action::Reload);
                Ok(())
            }
        }
    }
    fn kill_all(&mut self) {
        for tab in &mut self.tabs {
            let _ = tab.pty.child.kill();
        }
    }
    fn sync_tabs(&mut self) {
        let mut list = Vec::new();
        for (i, tab) in self.tabs.iter().enumerate() {
            let title = tab.term.lock().unwrap().title.clone();
            let label = if title.trim().is_empty() { format!("Shell {}", i + 1) } else { title };
            list.push((tab.id, label));
        }
        let active_id = self.tabs[self.active].id;
        if self.renderer.set_tabs(&list, active_id) {
            self.resize_active();
        }
    }
    fn create_tab(&mut self) {
        let (cols, rows) = self.renderer.grid_size();
        let shell = resolve_shell(&self.cfg);
        match Pty::spawn(&shell, &self.cfg.terminal.working_directory, cols as u16, rows as u16) {
            Ok((pty, reader)) => {
                let mut t = Term::new(cols, rows);
                t.apply_config(&self.cfg);
                let term = Arc::new(Mutex::new(t));
                let id = self.next_id;
                self.next_id += 1;
                let dirty = Arc::new(AtomicBool::new(false));
                let want_lock = Arc::new(AtomicBool::new(false));
                let writer = pty.writer.clone();
                let proxy = self.proxy.clone();
                let (t2, d2, w2) = (term.clone(), dirty.clone(), want_lock.clone());
                thread::spawn(move || reader_loop(id, reader, t2, writer, d2, w2, proxy));
                self.tabs.push(Tab { id, term, pty, dirty, want_lock });
                self.active = self.tabs.len() - 1;
                self.sync_tabs();
                self.renderer.ensure_tab_visible(id);
                self.window.request_redraw();
            }
            Err(e) => {
                config::report_error(&format!("aeternus: erro ao criar aba: {e}"));
            }
        }
    }
    fn close_tab(&mut self, id: u64) {
        let Some(idx) = self.tabs.iter().position(|t| t.id == id) else { return };
        let mut tab = self.tabs.remove(idx);
        let _ = tab.pty.child.kill();
        if self.tabs.is_empty() {
            std::process::exit(0);
        }
        if idx < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.selecting = false;
        self.sync_tabs();
        let id = self.tabs[self.active].id;
        self.renderer.ensure_tab_visible(id);
        self.window.request_redraw();
    }
    fn switch_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() || idx == self.active {
            return;
        }
        self.active = idx;
        self.selecting = false;
        self.reset_blink();
        self.sync_tabs();
        let id = self.tabs[self.active].id;
        self.renderer.ensure_tab_visible(id);
        self.resize_active();
        self.window.request_redraw();
    }
    fn switch_tab_by_id(&mut self, id: u64) {
        if let Some(idx) = self.tabs.iter().position(|t| t.id == id) {
            self.switch_tab(idx);
        }
    }
    fn cycle_tab(&mut self, dir: isize) {
        if self.tabs.is_empty() {
            return;
        }
        let n = self.tabs.len() as isize;
        let next = (self.active as isize + dir).rem_euclid(n) as usize;
        self.switch_tab(next);
    }
    fn resize_active(&mut self) {
        let (cols, rows) = self.renderer.grid_size();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let mut t = tab.term.lock().unwrap();
        if cols != t.cols() || rows != t.rows() {
            t.resize(cols, rows);
            tab.pty.resize(cols as u16, rows as u16);
        }
    }
    fn reset_blink(&mut self) {
        self.blink_on = true;
        self.last_blink = Instant::now();
    }
    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.renderer.set_open_menu(None);
        self.renderer.set_hover_item(None);
        self.renderer.close_settings();
        self.renderer.resize(size.width, size.height);
        self.resize_active();
        self.window.request_redraw();
    }
    fn set_zoom(&mut self, z: f32) {
        self.renderer.set_zoom(z);
        let size = self.window.inner_size();
        self.resize(size);
    }
    fn reload_config(&mut self) {
        match config::load() {
            Ok(cfg) => {
                self.cfg = cfg.clone();
                self.renderer.apply_config(&cfg);
                for tab in &self.tabs {
                    tab.term.lock().unwrap().apply_config(&cfg);
                }
                self.keymap = cfg.keymap;
                let size = self.window.inner_size();
                self.resize(size);
                eprintln!("aeternus: configuração recarregada");
            }
            Err(e) => {
                config::report_error(&format!("aeternus: erro na configuração (mantida a atual):\n{e}"))
            }
        }
        self.window.request_redraw();
    }
    fn settings_save(&mut self) {
        let mut cfg = self.cfg.clone();
        self.renderer.settings_writeback(&mut cfg);
        match config::save(&cfg) {
            Ok(()) => {
                self.cfg = cfg;
                self.config_mtime = config::mtime();
                self.reload_config();
            }
            Err(e) => {
                config::report_error(&format!("aeternus: erro ao salvar config: {e}"));
            }
        }
        self.renderer.close_settings();
        self.window.request_redraw();
    }
    fn copy_selection(&mut self, kind: ClipKind) {
        let text = self.term().lock().unwrap().selection_text();
        if let (Some(text), Some(cb)) = (text, self.clipboard.as_mut()) {
            if !text.is_empty() {
                let _ = cb.set().clipboard(kind).text(text);
            }
        }
    }
    fn osc52_copy(&mut self, kind: u8, text: String) {
        if text.is_empty() {
            return;
        }
        let Some(cb) = self.clipboard.as_mut() else { return };
        let target = if kind == b'p' { LinuxClipboardKind::Primary } else { LinuxClipboardKind::Clipboard };
        let _ = cb.set().clipboard(target).text(text);
    }
    fn paste(&mut self, kind: ClipKind) {
        let bracketed = {
            let term = self.term();
            let mut t = term.lock().unwrap();
            t.scroll_to_bottom();
            t.selection_clear();
            t.bracketed_paste
        };
        let clean = {
            let Some(cb) = self.clipboard.as_mut() else { return };
            let text = match cb.get().clipboard(kind).text() {
                Ok(t) => t,
                Err(_) => return,
            };
            text.replace('\x1b', "")
        };
        let mut data = Vec::with_capacity(clean.len() + 16);
        if bracketed {
            data.extend_from_slice(b"\x1b[200~");
            data.extend_from_slice(clean.as_bytes());
            data.extend_from_slice(b"\x1b[201~");
        } else {
            let fixed = clean.replace("\r\n", "\r").replace('\n', "\r");
            data.extend_from_slice(fixed.as_bytes());
        }
        self.write(&data);
        self.window.request_redraw();
    }
    fn key_input(&mut self, event: &KeyEvent, mods: ModifiersState) {
        if event.state != ElementState::Pressed {
            return;
        }
        if self.renderer.settings_open() {
            if let Key::Named(NamedKey::Escape) = &event.logical_key {
                self.renderer.close_settings();
            } else {
                self.renderer.settings_key(event, mods);
            }
            self.window.request_redraw();
            return;
        }
        if self.renderer.menu_open().is_some() {
            if let Key::Named(NamedKey::Escape) = &event.logical_key {
                self.close_menus();
                self.window.request_redraw();
            }
            return;
        }
        if mods.control_key() && !mods.shift_key() && !mods.alt_key() && !mods.super_key() {
            if let Key::Character(c) = &event.logical_key {
                match c.as_str().to_lowercase().as_str() {
                    "c" => {
                        let has_selection =
                            self.term().lock().unwrap().selection_bounds().is_some();
                        if has_selection {
                            self.copy_selection(ClipKind::Clipboard);
                            self.term().lock().unwrap().selection_clear();
                            self.window.request_redraw();
                            return;
                        }
                    }
                    "v" => {
                        self.paste(ClipKind::Clipboard);
                        return;
                    }
                    _ => {}
                }
            }
        }
        if let Some(action) =
            self.keymap.0.iter().find(|(c, _)| combo_matches(c, event, mods)).map(|(_, a)| *a)
        {
            self.run_key_action(action);
            return;
        }
        if mods.control_key() && mods.shift_key() {
            if let Key::Character(c) = &event.logical_key {
                match c.as_str().to_lowercase().as_str() {
                    "c" => {
                        self.copy_selection(ClipKind::Clipboard);
                        return;
                    }
                    "v" => {
                        self.paste(ClipKind::Clipboard);
                        return;
                    }
                    "t" => {
                        self.create_tab();
                        return;
                    }
                    "w" => {
                        let id = self.tabs[self.active].id;
                        self.close_tab(id);
                        return;
                    }
                    "r" => {
                        self.reload_config();
                        return;
                    }
                    "+" | "=" => {
                        self.set_zoom(self.renderer.zoom() + 0.1);
                        return;
                    }
                    "-" | "_" => {
                        self.set_zoom(self.renderer.zoom() - 0.1);
                        return;
                    }
                    "0" | ")" => {
                        self.set_zoom(1.0);
                        return;
                    }
                    _ => {}
                }
            }
        }
        if mods.shift_key() && event.logical_key == Key::Named(NamedKey::Insert) {
            self.paste(ClipKind::Primary);
            return;
        }
        if mods.control_key() && !mods.alt_key() && !mods.super_key() {
            if event.logical_key == Key::Named(NamedKey::Tab) {
                self.cycle_tab(if mods.shift_key() { -1 } else { 1 });
                return;
            }
            if !mods.shift_key() {
                match &event.logical_key {
                    Key::Named(NamedKey::PageUp) => {
                        self.cycle_tab(-1);
                        return;
                    }
                    Key::Named(NamedKey::PageDown) => {
                        self.cycle_tab(1);
                        return;
                    }
                    _ => {}
                }
            }
        }
        if !mods.control_key()
            && !mods.alt_key()
            && !mods.super_key()
            && event.logical_key == Key::Named(NamedKey::F11)
        {
            self.run_action(Action::Fullscreen);
            return;
        }
        let term = self.term();
        let mut t = term.lock().unwrap();
        if mods.shift_key() && !mods.control_key() && !t.is_alt_screen() {
            let page = t.rows() as isize - 1;
            let delta = match &event.logical_key {
                Key::Named(NamedKey::PageUp) => Some(page),
                Key::Named(NamedKey::PageDown) => Some(-page),
                Key::Named(NamedKey::Home) => Some(isize::MAX),
                Key::Named(NamedKey::End) => Some(isize::MIN),
                _ => None,
            };
            if let Some(d) = delta {
                if t.scroll_display(d) {
                    self.window.request_redraw();
                }
                return;
            }
        }
        if let Some(bytes) = key_bytes(event, mods, t.app_cursor_keys, t.app_keypad) {
            t.scroll_to_bottom();
            t.selection_clear();
            drop(t);
            self.reset_blink();
            self.write(&bytes);
            self.window.request_redraw();
        }
    }
    fn run_key_action(&mut self, action: KeyAction) {
        let a = match action {
            KeyAction::Copy => Action::Copy,
            KeyAction::Paste => Action::Paste,
            KeyAction::PastePrimary => Action::PastePrimary,
            KeyAction::ReloadConfig => Action::Reload,
            KeyAction::ZoomIn => Action::ZoomIn,
            KeyAction::ZoomOut => Action::ZoomOut,
            KeyAction::ZoomReset => Action::ZoomReset,
            KeyAction::ScrollPageUp
            | KeyAction::ScrollPageDown
            | KeyAction::ScrollToTop
            | KeyAction::ScrollToBottom => {
                let term = self.term();
                let mut t = term.lock().unwrap();
                if t.is_alt_screen() {
                    return;
                }
                let page = t.rows() as isize - 1;
                let delta = match action {
                    KeyAction::ScrollPageUp => page,
                    KeyAction::ScrollPageDown => -page,
                    KeyAction::ScrollToTop => isize::MAX,
                    _ => isize::MIN,
                };
                if t.scroll_display(delta) {
                    drop(t);
                    self.window.request_redraw();
                }
                return;
            }
        };
        self.run_action(a);
    }
    fn mouse_input(&mut self, st: ElementState, button: MouseButton, mods: ModifiersState) {
        let pressed = st == ElementState::Pressed;
        let (mx, my) = self.mouse_pos;
        if self.renderer.settings_open() {
            if pressed && button == MouseButton::Left {
                match self.renderer.settings_click(mx, my) {
                    Some(SettingsOutcome::Saved) => self.settings_save(),
                    Some(SettingsOutcome::Cancelled) => {
                        self.renderer.close_settings();
                        self.window.request_redraw();
                    }
                    None => self.window.request_redraw(),
                }
            } else if !pressed && button == MouseButton::Left {
                if self.renderer.picker_release() {
                    self.window.request_redraw();
                }
            }
            return;
        }
        if self.renderer.menu_open().is_some() {
            if pressed && button == MouseButton::Left {
                self.menu_click(mx, my);
            }
            return;
        }
        if pressed && button == MouseButton::Left && self.renderer.hit_gear(mx, my) {
            self.renderer.open_settings(&self.cfg);
            self.window.request_redraw();
            return;
        }
        if pressed && self.renderer.tabs_visible() {
            if let Some(id) = self.renderer.hit_tab_close(mx, my) {
                if button == MouseButton::Left || button == MouseButton::Middle {
                    self.close_tab(id);
                    return;
                }
            } else if button == MouseButton::Middle {
                if let Some(id) = self.renderer.hit_tab(mx, my) {
                    self.close_tab(id);
                    return;
                }
            } else if button == MouseButton::Left {
                if self.renderer.hit_tab_prev(mx, my) {
                    if self.renderer.tab_strip_scroll(-1) {
                        self.window.request_redraw();
                    }
                    return;
                }
                if self.renderer.hit_tab_next(mx, my) {
                    if self.renderer.tab_strip_scroll(1) {
                        self.window.request_redraw();
                    }
                    return;
                }
                if self.renderer.hit_new_tab(mx, my) {
                    self.create_tab();
                    return;
                }
                if let Some(id) = self.renderer.hit_tab(mx, my) {
                    self.switch_tab_by_id(id);
                    return;
                }
            }
        }
        if my <= self.renderer.top_inset() as f64 {
            if pressed && button == MouseButton::Left && my < self.renderer.toolbar_h() as f64 {
                self.menu_click(mx, my);
            }
            return;
        }
        let (row, col) = self.mouse_cell;
        let mode = self.term().lock().unwrap().mouse_mode;
        let reporting = (mode != MouseMode::Off && !mods.shift_key())
            || (!pressed && self.report_button.is_some());
        if reporting {
            let code = match button {
                MouseButton::Left => 0,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                _ => return,
            };
            let bytes = {
                let term = self.term();
                let t = term.lock().unwrap();
                mouse_report(&t, code, row, col, !pressed, false, mods)
            };
            self.report_button = if pressed { Some(code) } else { None };
            self.last_reported = Some((row, col));
            if let Some(b) = bytes {
                self.write(&b);
            }
            return;
        }
        match (button, pressed) {
            (MouseButton::Left, true) => {
                let now = Instant::now();
                if now.duration_since(self.last_click) < MULTI_CLICK
                    && self.last_click_cell == (row, col)
                {
                    self.click_count = self.click_count % 3 + 1;
                } else {
                    self.click_count = 1;
                }
                self.last_click = now;
                self.last_click_cell = (row, col);
                let sel_mode = match self.click_count {
                    2 => SelMode::Word,
                    3 => SelMode::Line,
                    _ => SelMode::Simple,
                };
                self.term().lock().unwrap().selection_start(row, col, sel_mode);
                self.selecting = true;
                self.window.request_redraw();
            }
            (MouseButton::Left, false) => {
                self.selecting = false;
                let has = {
                    let term = self.term();
                    let mut t = term.lock().unwrap();
                    let has = t.selection_bounds().is_some();
                    if !has {
                        t.selection_clear();
                    }
                    has
                };
                if has {
                    self.copy_selection(ClipKind::Primary);
                }
                self.window.request_redraw();
            }
            (MouseButton::Middle, true) => self.paste(ClipKind::Primary),
            _ => {}
        }
    }
    fn menu_click(&mut self, mx: f64, my: f64) {
        if let Some(m) = self.renderer.menu_open() {
            if let Some(i) = self.renderer.hit_item_index(mx, my) {
                let action = self.renderer.item_action(m, i);
                self.close_menus();
                if let Some(a) = action {
                    self.run_action(a);
                }
                return;
            }
            if let Some(t) = self.renderer.hit_title(mx, my) {
                let next = if t == m { None } else { Some(t) };
                self.close_menus();
                if let Some(n) = next {
                    self.renderer.set_open_menu(Some(n));
                }
                self.window.request_redraw();
                return;
            }
            self.close_menus();
            return;
        }
        if let Some(t) = self.renderer.hit_title(mx, my) {
            self.renderer.set_hover_title(Some(t));
            self.renderer.set_open_menu(Some(t));
            self.window.request_redraw();
        }
    }
    fn close_menus(&mut self) {
        self.renderer.set_open_menu(None);
        self.renderer.set_hover_item(None);
        self.renderer.set_hover_title(None);
        self.window.request_redraw();
    }
    fn run_action(&mut self, action: Action) {
        match action {
            Action::Copy => self.copy_selection(ClipKind::Clipboard),
            Action::Paste => self.paste(ClipKind::Clipboard),
            Action::PastePrimary => self.paste(ClipKind::Primary),
            Action::SelectAll => {
                self.term().lock().unwrap().selection_select_all();
            }
            Action::ZoomIn => self.set_zoom(self.renderer.zoom() + 0.1),
            Action::ZoomOut => self.set_zoom(self.renderer.zoom() - 0.1),
            Action::ZoomReset => self.set_zoom(1.0),
            Action::ScrollTop => {
                self.term().lock().unwrap().scroll_display(isize::MAX);
            }
            Action::ScrollBottom => {
                self.term().lock().unwrap().scroll_display(isize::MIN);
            }
            Action::ResetTerm => {
                self.term().lock().unwrap().reset();
            }
            Action::Reload => self.reload_config(),
            Action::Settings => {
                self.renderer.open_settings(&self.cfg);
            }
            Action::Quit => {
                if let Some((s, l)) = &self.sock {
                    aet_cleanup(s, l);
                }
                self.kill_all();
                std::process::exit(0);
            }
            Action::NewTab => self.create_tab(),
            Action::CloseTab => {
                let id = self.tabs[self.active].id;
                self.close_tab(id);
            }
            Action::Fullscreen => {
                let on = self.window.fullscreen().is_some();
                self.window.set_fullscreen(if on {
                    None
                } else {
                    Some(Fullscreen::Borderless(None))
                });
            }
            Action::About => {
                let term = self.term();
                let mut t = term.lock().unwrap();
                t.scroll_to_bottom();
                t.selection_clear();
                let art = about_art();
                Parser::new().advance(&mut *t, art.as_bytes());
                drop(t);
                self.write(b"\n");
            }
        }
        self.window.request_redraw();
    }
    fn cursor_moved(&mut self, x: f64, y: f64, mods: ModifiersState) {
        self.mouse_pos = (x, y);
        if self.renderer.settings_open() {
            self.window.set_cursor(CursorIcon::Default);
            if self.renderer.settings_move(x, y) {
                self.window.request_redraw();
            }
            return;
        }
        if self.renderer.menu_open().is_some() {
            if y <= self.renderer.top_inset() as f64 {
                if let Some(t) = self.renderer.hit_title(x, y) {
                    if t != self.renderer.menu_open().unwrap() {
                        self.renderer.set_hover_item(None);
                        self.renderer.set_open_menu(Some(t));
                        self.window.request_redraw();
                    }
                }
            } else if let Some(i) = self.renderer.hit_item_index(x, y) {
                if self.renderer.set_hover_item(Some(i)) {
                    self.window.request_redraw();
                }
            } else if self.renderer.set_hover_item(None) {
                self.window.request_redraw();
            }
            self.window.set_cursor(CursorIcon::Default);
            return;
        }
        if y <= self.renderer.top_inset() as f64 {
            if y >= self.renderer.toolbar_h() as f64 && self.renderer.tabs_visible() {
                let c = self.renderer.hit_tab_close(x, y);
                let t = if c.is_none() { self.renderer.hit_tab(x, y) } else { None };
                let n = self.renderer.hit_new_tab(x, y);
                let p = self.renderer.hit_tab_prev(x, y);
                let nx = self.renderer.hit_tab_next(x, y);
                let mut changed = self.renderer.set_hover_tab(t);
                changed |= self.renderer.set_hover_close(c);
                changed |= self.renderer.set_hover_new(n);
                changed |= self.renderer.set_hover_prev(p);
                changed |= self.renderer.set_hover_next(nx);
                changed |= self.renderer.set_hover_gear(false);
                changed |= self.renderer.set_hover_title(None);
                if changed {
                    self.window.request_redraw();
                }
                self.window.set_cursor(
                    if t.is_some() || n || c.is_some() || p || nx {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                );
                return;
            }
            let g = self.renderer.hit_gear(x, y);
            let mut changed = self.renderer.set_hover_gear(g);
            changed |= self.renderer.set_hover_new(false);
            changed |= self.renderer.set_hover_prev(false);
            changed |= self.renderer.set_hover_next(false);
            changed |= self.renderer.set_hover_tab(None);
            changed |= self.renderer.set_hover_close(None);
            let t = self.renderer.hit_title(x, y);
            changed |= self.renderer.set_hover_title(t);
            if changed {
                self.window.request_redraw();
            }
            self.window
                .set_cursor(if t.is_some() || g { CursorIcon::Pointer } else { CursorIcon::Default });
            return;
        }
        let mut ui_clear = self.renderer.set_hover_title(None);
        ui_clear |= self.renderer.set_hover_item(None);
        ui_clear |= self.renderer.clear_hover();
        if ui_clear {
            self.window.set_cursor(CursorIcon::Text);
            self.window.request_redraw();
        }
        let cell = self.renderer.cell_at(x, y);
        let changed = cell != self.mouse_cell;
        self.mouse_cell = cell;
        let (row, col) = cell;
        let mode = self.term().lock().unwrap().mouse_mode;
        if (mode != MouseMode::Off && !mods.shift_key()) || self.report_button.is_some() {
            if !changed {
                return;
            }
            let wanted = match mode {
                MouseMode::Motion => true,
                MouseMode::Drag => self.report_button.is_some(),
                _ => false,
            };
            if !wanted {
                return;
            }
            let btn = self.report_button.unwrap_or(3);
            let bytes = {
                let term = self.term();
                let t = term.lock().unwrap();
                mouse_report(&t, btn, row, col, false, true, mods)
            };
            if let Some(b) = bytes {
                self.write(&b);
            }
            return;
        }
        if self.selecting {
            let h = self.window.inner_size().height as f64;
            let pad = self.renderer.pad() as f64;
            let term = self.term();
            let mut t = term.lock().unwrap();
            if y < pad {
                t.scroll_display(1);
            } else if y > h - pad {
                t.scroll_display(-1);
            }
            t.selection_update(row, col);
            drop(t);
            self.window.request_redraw();
        }
    }
    fn wheel(&mut self, delta: MouseScrollDelta, mods: ModifiersState) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, y) => y * 3.0,
            MouseScrollDelta::PixelDelta(p) => p.y as f32 / self.renderer.cell_h,
        };
        if self.renderer.settings_open() {
            self.renderer.settings_scroll(-lines);
            self.window.request_redraw();
            return;
        }
        if mods.control_key() {
            if lines != 0.0 {
                let z = self.renderer.zoom() + 0.1 * lines.signum();
                self.set_zoom(z);
            }
            return;
        }
        self.scroll_accum += lines;
        let whole = self.scroll_accum.trunc();
        if whole == 0.0 {
            return;
        }
        self.scroll_accum -= whole;
        let n = whole as isize;
        let (mx, my) = self.mouse_pos;
        if self.renderer.tab_strip_hit(mx, my) {
            if self.renderer.tab_strip_scroll(-n) {
                self.window.request_redraw();
            }
            return;
        }
        let count = n.unsigned_abs().min(30);
        let (row, col) = self.mouse_cell;
        let (mode, alt, app_cursor, alt_scroll) = {
            let term = self.term();
            let t = term.lock().unwrap();
            (t.mouse_mode, t.is_alt_screen(), t.app_cursor_keys, t.alt_scroll)
        };
        if mode != MouseMode::Off && !mods.shift_key() {
            let code = if n > 0 { 64 } else { 65 };
            let mut out = Vec::new();
            {
                let term = self.term();
                let t = term.lock().unwrap();
                for _ in 0..count {
                    if let Some(b) = mouse_report(&t, code, row, col, false, false, mods) {
                        out.extend_from_slice(&b);
                    }
                }
            }
            self.write(&out);
        } else if alt && alt_scroll && !mods.shift_key() {
            let key = if n > 0 { b'A' } else { b'B' };
            let seq: [u8; 3] = if app_cursor { [0x1b, b'O', key] } else { [0x1b, b'[', key] };
            let mut out = Vec::with_capacity(count * 3);
            for _ in 0..count {
                out.extend_from_slice(&seq);
            }
            self.write(&out);
        } else if self.term().lock().unwrap().scroll_display(n) {
            self.window.request_redraw();
        }
    }
    fn focus_changed(&mut self, focused: bool) {
        self.focused = focused;
        self.reset_blink();
        let report = self.term().lock().unwrap().focus_reporting;
        if report {
            self.write(if focused { b"\x1b[I" } else { b"\x1b[O" });
        }
        self.window.request_redraw();
    }
}
struct App {
    proxy: EventLoopProxy<UserEvent>,
    state: Option<State>,
    mods: ModifiersState,
    sock: Option<(PathBuf, PathBuf)>,
}
impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.state.is_none() {
            let mut s = State::new(el, self.proxy.clone());
            s.sock = self.sock.clone();
            self.state = Some(s);
        }
    }
    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Wakeup(id) => {
                if let Some(s) = &self.state {
                    if s.tabs.get(s.active).map_or(false, |t| t.id == id) {
                        s.window.request_redraw();
                    }
                }
            }
            UserEvent::CloseTab(id) => {
                if let Some(s) = self.state.as_mut() {
                    s.close_tab(id);
                }
            }
            UserEvent::Aet(cmd, mut stream) => {
                let reply = match self.state.as_mut() {
                    Some(s) => match s.aet_handle(cmd) {
                        Ok(()) => String::from("ok"),
                        Err(e) => format!("err {e}"),
                    },
                    None => String::from("err sem janela"),
                };
                let _ = stream.write_all(reply.as_bytes());
                let _ = stream.write_all(b"\n");
            }
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(s) = self.state.as_mut() else { return };
        let now = Instant::now();
        let (blinking, sync_expired, sync_deadline) = {
            let mut t = s.tabs[s.active].term.lock().unwrap();
            let expired = t.expire_sync(now);
            (s.focused && t.cursor_blink && t.cursor_visible, expired, t.sync_deadline())
        };
        if sync_expired {
            s.window.request_redraw();
        }
        let mut next_config_check = s.last_config_check + CONFIG_POLL_INTERVAL;
        if now >= next_config_check {
            s.last_config_check = now;
            next_config_check = now + CONFIG_POLL_INTERVAL;
            let mtime = config::mtime();
            if mtime != s.config_mtime {
                s.config_mtime = mtime;
                s.reload_config();
            }
        }
        let mut wake = sync_deadline;
        wake = Some(wake.map_or(next_config_check, |w| w.min(next_config_check)));
        if blinking {
            let mut next = s.last_blink + BLINK_INTERVAL;
            if now >= next {
                s.blink_on = !s.blink_on;
                s.last_blink = now;
                next = now + BLINK_INTERVAL;
                s.window.request_redraw();
            }
            wake = Some(wake.map_or(next, |w| w.min(next)));
        } else if !s.blink_on {
            s.blink_on = true;
            s.window.request_redraw();
        }
        if let Some(end) = s.renderer.settings_anim_wake(now) {
            s.window.request_redraw();
            let step = (now + Duration::from_millis(16)).min(end);
            wake = Some(wake.map_or(step, |w| w.min(step)));
        }
        match wake {
            Some(w) => el.set_control_flow(ControlFlow::WaitUntil(w)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }
    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(s) = self.state.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => {
                if let Some((s, l)) = &self.sock {
                    aet_cleanup(s, l);
                }
                s.kill_all();
                std::process::exit(0);
            }
            WindowEvent::Resized(size) => s.resize(size),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                s.renderer.set_scale(scale_factor as f32);
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } => s.key_input(&event, self.mods),
            WindowEvent::MouseInput { state, button, .. } => {
                s.mouse_input(state, button, self.mods)
            }
            WindowEvent::CursorMoved { position, .. } => {
                s.cursor_moved(position.x, position.y, self.mods)
            }
            WindowEvent::MouseWheel { delta, .. } => s.wheel(delta, self.mods),
            WindowEvent::Focused(f) => {
                if f {
                    if let Some((sock, last)) = &self.sock {
                        let _ = std::fs::write(last, sock.to_string_lossy().as_bytes());
                    }
                }
                s.focus_changed(f);
            }
            WindowEvent::RedrawRequested => {
                s.sync_tabs();
                let dirty = s.tabs[s.active].dirty.clone();
                dirty.store(false, Ordering::Release);
                let want_lock = s.tabs[s.active].want_lock.clone();
                want_lock.store(true, Ordering::Relaxed);
                let mut skip_draw = false;
                let clip;
                {
                    let mut t = s.tabs[s.active].term.lock().unwrap();
                    want_lock.store(false, Ordering::Relaxed);
                    if !t.title.is_empty() && t.title != s.last_title {
                        s.window.set_title(&t.title);
                        s.last_title = t.title.clone();
                    }
                    clip = t.clipboard_write.take();
                    t.expire_sync(Instant::now());
                    if t.sync_output {
                        skip_draw = true;
                    } else {
                        let view = View { focused: s.focused, blink_on: s.blink_on };
                        s.renderer.build(&t, &view);
                    }
                }
                if let Some((kind, text)) = clip {
                    s.osc52_copy(kind, text);
                }
                if !skip_draw {
                    s.renderer.draw();
                }
            }
            _ => {}
        }
    }
}

fn named_key_alias(key: &str) -> Option<NamedKey> {
    Some(match key {
        "pageup" => NamedKey::PageUp,
        "pagedown" => NamedKey::PageDown,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "insert" => NamedKey::Insert,
        "delete" => NamedKey::Delete,
        "enter" => NamedKey::Enter,
        "escape" => NamedKey::Escape,
        "tab" => NamedKey::Tab,
        "space" => NamedKey::Space,
        "up" => NamedKey::ArrowUp,
        "down" => NamedKey::ArrowDown,
        "left" => NamedKey::ArrowLeft,
        "right" => NamedKey::ArrowRight,
        "f1" => NamedKey::F1,
        "f2" => NamedKey::F2,
        "f3" => NamedKey::F3,
        "f4" => NamedKey::F4,
        "f5" => NamedKey::F5,
        "f6" => NamedKey::F6,
        "f7" => NamedKey::F7,
        "f8" => NamedKey::F8,
        "f9" => NamedKey::F9,
        "f10" => NamedKey::F10,
        "f11" => NamedKey::F11,
        "f12" => NamedKey::F12,
        _ => return None,
    })
}

fn char_alias(key: &str) -> Option<&'static str> {
    Some(match key {
        "plus" => "+",
        "equal" => "=",
        "minus" => "-",
        "underscore" => "_",
        "zero" => "0",
        "paren_right" => ")",
        _ => return None,
    })
}

fn combo_matches(combo: &KeyCombo, event: &KeyEvent, mods: ModifiersState) -> bool {
    if combo.shift != mods.shift_key()
        || combo.ctrl != mods.control_key()
        || combo.alt != mods.alt_key()
        || combo.logo != mods.super_key()
    {
        return false;
    }
    if let Some(named) = named_key_alias(&combo.key) {
        return event.logical_key == Key::Named(named);
    }
    let want = char_alias(&combo.key).unwrap_or(&combo.key);
    if let Key::Character(s) = &event.logical_key {
        return s.as_str().eq_ignore_ascii_case(want);
    }
    false
}
fn mouse_report(
    t: &Term,
    button: u8,
    row: usize,
    col: usize,
    release: bool,
    motion: bool,
    mods: ModifiersState,
) -> Option<Vec<u8>> {
    let mut cb = if release && !t.mouse_sgr { 3 } else { button };
    if motion {
        cb += 32;
    }
    if mods.alt_key() {
        cb += 8;
    }
    if mods.control_key() {
        cb += 16;
    }
    if t.mouse_sgr {
        let end = if release { 'm' } else { 'M' };
        Some(format!("\x1b[<{};{};{}{}", cb, col + 1, row + 1, end).into_bytes())
    } else {
        if col >= 222 || row >= 222 {
            return None;
        }
        Some(vec![0x1b, b'[', b'M', 32 + cb, 33 + col as u8, 33 + row as u8])
    }
}
fn key_bytes(
    ev: &KeyEvent,
    mods: ModifiersState,
    app_cursor: bool,
    app_keypad: bool,
) -> Option<Vec<u8>> {
    let m = 1
        + mods.shift_key() as u8
        + 2 * mods.alt_key() as u8
        + 4 * mods.control_key() as u8
        + 8 * mods.super_key() as u8;
    let modified = m > 1;
    let csi_final = |c: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{}{}", m, c as char).into_bytes()
        } else if app_cursor {
            vec![0x1b, b'O', c]
        } else {
            vec![0x1b, b'[', c]
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{};{}~", n, m).into_bytes()
        } else {
            format!("\x1b[{}~", n).into_bytes()
        }
    };
    let ss3 = |c: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{}{}", m, c as char).into_bytes()
        } else {
            vec![0x1b, b'O', c]
        }
    };
    let alt_only = mods.alt_key() && !mods.shift_key() && !mods.control_key() && !mods.super_key();
    if alt_only {
        match &ev.logical_key {
            Key::Named(NamedKey::ArrowLeft) => return Some(vec![0x1b, b'b']),
            Key::Named(NamedKey::ArrowRight) => return Some(vec![0x1b, b'f']),
            _ => {}
        }
    }
    if app_keypad && !modified && ev.location == KeyLocation::Numpad {
        let code = match &ev.logical_key {
            Key::Named(NamedKey::Enter) => Some(b'M'),
            Key::Character(s) if s.len() == 1 => match s.as_bytes()[0] {
                d @ b'0'..=b'9' => Some(b'p' + (d - b'0')),
                b'.' => Some(b'n'),
                b',' => Some(b'l'),
                b'+' => Some(b'k'),
                b'-' => Some(b'm'),
                b'*' => Some(b'j'),
                b'/' => Some(b'o'),
                b'=' => Some(b'X'),
                _ => None,
            },
            _ => None,
        };
        if let Some(c) = code {
            return Some(vec![0x1b, b'O', c]);
        }
    }
    if let Key::Named(n) = &ev.logical_key {
        let special = match n {
            NamedKey::ArrowUp => Some(csi_final(b'A')),
            NamedKey::ArrowDown => Some(csi_final(b'B')),
            NamedKey::ArrowRight => Some(csi_final(b'C')),
            NamedKey::ArrowLeft => Some(csi_final(b'D')),
            NamedKey::Home => Some(csi_final(b'H')),
            NamedKey::End => Some(csi_final(b'F')),
            NamedKey::Insert => Some(tilde(2)),
            NamedKey::Delete => Some(tilde(3)),
            NamedKey::PageUp => Some(tilde(5)),
            NamedKey::PageDown => Some(tilde(6)),
            NamedKey::F1 => Some(ss3(b'P')),
            NamedKey::F2 => Some(ss3(b'Q')),
            NamedKey::F3 => Some(ss3(b'R')),
            NamedKey::F4 => Some(ss3(b'S')),
            NamedKey::F5 => Some(tilde(15)),
            NamedKey::F6 => Some(tilde(17)),
            NamedKey::F7 => Some(tilde(18)),
            NamedKey::F8 => Some(tilde(19)),
            NamedKey::F9 => Some(tilde(20)),
            NamedKey::F10 => Some(tilde(21)),
            NamedKey::F11 => Some(tilde(23)),
            NamedKey::F12 => Some(tilde(24)),
            _ => None,
        };
        if special.is_some() {
            return special;
        }
    }
    let mut out = match &ev.logical_key {
        Key::Named(n) => match n {
            NamedKey::Enter => b"\r".to_vec(),
            NamedKey::Backspace => vec![0x7f],
            NamedKey::Tab => {
                if mods.shift_key() { b"\x1b[Z".to_vec() } else { b"\t".to_vec() }
            }
            NamedKey::Escape => vec![0x1b],
            NamedKey::Space => {
                if mods.control_key() { vec![0x00] } else { b" ".to_vec() }
            }
            _ => ev.text.as_ref()?.as_bytes().to_vec(),
        },
        Key::Character(s) if mods.control_key() && s.as_str() == " " => vec![0x00],
        Key::Character(s) if mods.control_key() && s.as_str() == "/" => vec![0x1f],
        Key::Character(s)
            if mods.control_key()
                && s.len() == 1
                && matches!(s.as_bytes()[0], b'a'..=b'z' | b'A'..=b'Z' | b'['..=b'_') =>
        {
            vec![s.as_bytes()[0] & 0x1f]
        }
        _ => ev.text.as_ref()?.as_bytes().to_vec(),
    };
    if mods.alt_key() {
        out.insert(0, 0x1b);
    }
    Some(out)
}
fn resolve_shell(cfg: &Config) -> String {
    if cfg.terminal.shell.trim().is_empty() {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    } else {
        cfg.terminal.shell.trim().to_string()
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut run_bench = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" => {
                println!("aeternus {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--bench" => run_bench = true,
            "--config" => match args.next() {
                Some(p) => config::set_config_path(p.into()),
                None => {
                    eprintln!("aeternus: --config precisa de um caminho");
                    std::process::exit(2);
                }
            },
            s if s.starts_with("--config=") => {
                config::set_config_path(s["--config=".len()..].into());
            }
            _ => {}
        }
    }
    if run_bench {
        bench::run_all();
        return Ok(());
    }
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        proxy: event_loop.create_proxy(),
        state: None,
        mods: ModifiersState::default(),
        sock: None,
    };
    aet_bind(&mut app);
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod aet_tests {
    use super::{parse_aet, AetCmd};
    #[test]
    fn parse_aet_commands() {
        assert_eq!(parse_aet(b"version\n"), Some(AetCmd::Version));
        assert_eq!(parse_aet(b"pref\n"), Some(AetCmd::Pref));
        assert_eq!(parse_aet(b"reload \n"), Some(AetCmd::Reload));
        assert_eq!(parse_aet(b"echo version\n"), None);
        assert_eq!(parse_aet(b"\n"), None);
    }
}
