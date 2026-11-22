use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use arboard::{Clipboard, GetExtLinux, LinuxClipboardKind, SetExtLinux};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::platform::x11::WindowAttributesExtX11;
use winit::window::{CursorIcon, Fullscreen, UserAttentionType, Window, WindowId};

use crate::about::about_art;
use crate::config::{self, Action as KeyAction, Config};
use crate::input::{self, KeyInput, KeyModes};
use crate::ipc::{self, Command};
use crate::log;
use crate::notify;
use crate::parser::Parser;
use crate::pty::{self, Pty, Size};
use crate::render::{Action, Renderer, SettingsOutcome, View};
use crate::search::Search;
use crate::term::{MouseMode, SelMode, Term};
use crate::terminfo;

pub const APP_ID: &str = "io.github.AeternalAngel.Aeternus";
const BLINK_INTERVAL: Duration = Duration::from_millis(530);
const MULTI_CLICK: Duration = Duration::from_millis(400);
const CONFIG_POLL_INTERVAL: Duration = Duration::from_millis(1000);
const FRAME: Duration = Duration::from_millis(8);
const OPENABLE: [&str; 6] = ["https://", "http://", "file://", "ftp://", "mailto:", "ssh://"];

type ClipKind = LinuxClipboardKind;

#[derive(Debug)]
enum UserEvent {
    Wakeup(u64),
    CloseTab(u64),
    Ipc(Command, UnixStream),
    Quit,
}

struct Tab {
    id: u64,
    term: Arc<Mutex<Term>>,
    pty: Option<Pty>,
    size: Size,
    dirty: Arc<AtomicBool>,
    want_lock: Arc<AtomicBool>,
    reader: thread::Thread,
}

impl Tab {
    fn write(&self, bytes: &[u8]) {
        if let Some(p) = &self.pty {
            if let Ok(mut w) = p.writer.lock() {
                let _ = w.write_all(bytes);
            }
        }
    }
    fn reap(&mut self) -> Option<thread::JoinHandle<()>> {
        let mut pty = self.pty.take()?;
        Some(thread::spawn(move || {
            let _ = pty.child.kill();
            let _ = pty.child.wait();
        }))
    }
    fn cwd(&self) -> Option<PathBuf> {
        if let Some(c) = self.term.lock().ok().and_then(|t| t.cwd.clone()) {
            return Some(PathBuf::from(c));
        }
        let pid = self.pty.as_ref()?.pid()?;
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }
}

fn reader_loop(
    id: u64,
    mut reader: Box<dyn Read + Send>,
    term: Arc<Mutex<Term>>,
    writer: pty::Writer,
    dirty: Arc<AtomicBool>,
    want_lock: Arc<AtomicBool>,
    proxy: EventLoopProxy<UserEvent>,
) {
    let mut parser = Parser::new();
    let mut buf = vec![0u8; 65536];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
            Ok(n) => {
                while want_lock.load(Ordering::Acquire) {
                    thread::park_timeout(Duration::from_millis(4));
                }
                let replies = {
                    let Ok(mut term) = term.lock() else { break };
                    parser.advance(&mut *term, &buf[..n]);
                    std::mem::take(&mut term.replies)
                };
                if !replies.is_empty() {
                    if let Ok(mut w) = writer.lock() {
                        let _ = w.write_all(&replies);
                    }
                }
                if !dirty.swap(true, Ordering::AcqRel) {
                    let _ = proxy.send_event(UserEvent::Wakeup(id));
                }
            }
        }
    }
    let _ = proxy.send_event(UserEvent::CloseTab(id));
}

fn ipc_listener(l: UnixListener, proxy: EventLoopProxy<UserEvent>) {
    for conn in l.incoming() {
        let Ok(mut s) = conn else { break };
        let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
        let Ok(r) = s.try_clone() else { continue };
        let mut line = String::new();
        if BufReader::new(r).take(256).read_line(&mut line).is_err() {
            continue;
        }
        match Command::parse(&line) {
            Some(cmd) => {
                if proxy.send_event(UserEvent::Ipc(cmd, s)).is_err() {
                    break;
                }
            }
            None => {
                let _ = s.write_all(b"err unknown command\n");
            }
        }
    }
}

struct IpcPaths {
    sock: PathBuf,
    last: PathBuf,
}

impl IpcPaths {
    fn bind(proxy: EventLoopProxy<UserEvent>) -> Option<Self> {
        let sock = ipc::socket_path(std::process::id());
        let last = ipc::last_path();
        let _ = std::fs::remove_file(&sock);
        let l = match UnixListener::bind(&sock) {
            Ok(l) => l,
            Err(e) => {
                log::error(&format!("aeternus: aet socket unavailable ({}): {e}", sock.display()));
                return None;
            }
        };
        let paths = Self { sock, last };
        paths.mark_focused();
        thread::spawn(move || ipc_listener(l, proxy));
        Some(paths)
    }
    fn mark_focused(&self) {
        let _ = std::fs::write(&self.last, self.sock.to_string_lossy().as_bytes());
    }
    fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.sock);
        let mine = self.sock.to_string_lossy().to_string();
        if std::fs::read_to_string(&self.last).is_ok_and(|c| c.trim() == mine) {
            let _ = std::fs::remove_file(&self.last);
        }
    }
}

fn open_url(url: &str) {
    if !OPENABLE.iter().any(|s| url.starts_with(s)) {
        log::error(&format!("aeternus: refusing to open {url:?}"));
        return;
    }
    match std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => log::error(&format!("aeternus: could not run xdg-open: {e}")),
    }
}

fn key_input(event: &KeyEvent) -> KeyInput {
    let base = match event.key_without_modifiers() {
        Key::Character(s) => s.chars().next(),
        _ => None,
    };
    KeyInput {
        key: event.logical_key.clone(),
        base,
        text: event.text.as_ref().map(|t| t.to_string()),
        location: event.location,
    }
}

fn resolve_shell(cfg: &Config) -> Vec<String> {
    let configured = cfg.terminal.shell.trim();
    if !configured.is_empty() {
        match pty::parse_command(configured) {
            Ok(argv) => return argv,
            Err(e) => log::error(&format!("aeternus: {e}; using $SHELL")),
        }
    }
    let shell = std::env::var("SHELL").ok().filter(|s| !s.trim().is_empty());
    vec![shell.unwrap_or_else(|| "/bin/sh".into())]
}

struct State {
    window: Arc<Window>,
    renderer: Renderer,
    proxy: EventLoopProxy<UserEvent>,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    last_title: String,
    last_pointer: Option<CursorIcon>,
    cfg: Config,
    scroll_accum: f32,
    clipboard: Option<Clipboard>,
    focused: bool,
    blink_on: bool,
    last_blink: Instant,
    text_blink_on: bool,
    last_text_blink: Instant,
    mouse_pos: (f64, f64),
    mouse_cell: (usize, usize),
    selecting: bool,
    report_button: Option<u8>,
    last_click: Instant,
    last_click_cell: (usize, usize),
    click_count: u8,
    config_mtime: Option<std::time::SystemTime>,
    last_config_check: Instant,
    search: Option<Search>,
    quit: bool,
}

impl State {
    fn new(el: &ActiveEventLoop, proxy: EventLoopProxy<UserEvent>) -> Result<Self, String> {
        let cfg = config::load().unwrap_or_else(|e| {
            log::error(&format!("aeternus: configuration error, using defaults:\n{e}"));
            Config::defaults()
        });
        let attrs = Window::default_attributes()
            .with_title("Aeternus")
            .with_inner_size(LogicalSize::new(cfg.window.width as f64, cfg.window.height as f64))
            .with_min_inner_size(LogicalSize::new(200.0, 120.0))
            .with_decorations(true)
            .with_transparent(true);
        let attrs = WindowAttributesExtWayland::with_name(attrs, APP_ID, "aeternus");
        let attrs = WindowAttributesExtX11::with_name(attrs, APP_ID, "aeternus");
        let window = Arc::new(el.create_window(attrs).map_err(|e| format!("could not open a window: {e}"))?);
        window.set_cursor(CursorIcon::Text);
        let renderer = Renderer::new(window.clone(), &cfg)?;
        let now = Instant::now();
        let config_mtime = config::watched_mtime(&cfg);
        let mut state = Self {
            window,
            renderer,
            proxy,
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            last_title: String::new(),
            last_pointer: None,
            cfg,
            scroll_accum: 0.0,
            clipboard: Clipboard::new().ok(),
            focused: true,
            blink_on: true,
            last_blink: now,
            text_blink_on: true,
            last_text_blink: now,
            mouse_pos: (0.0, 0.0),
            mouse_cell: (0, 0),
            selecting: false,
            report_button: None,
            last_click: now,
            last_click_cell: (usize::MAX, usize::MAX),
            click_count: 0,
            config_mtime,
            last_config_check: now,
            search: None,
            quit: false,
        };
        let cwd =
            Some(state.cfg.terminal.working_directory.trim()).filter(|s| !s.is_empty()).map(PathBuf::from);
        state.create_tab(cwd)?;
        state.window.request_redraw();
        Ok(state)
    }
    fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }
    fn term(&self) -> Arc<Mutex<Term>> {
        self.active_tab().term.clone()
    }
    fn with_term<R>(&self, f: impl FnOnce(&mut Term) -> R) -> R {
        let term = self.term();
        let mut t = term.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut t)
    }
    fn write(&self, bytes: &[u8]) {
        self.active_tab().write(bytes);
    }
    fn pty_size(&self) -> Size {
        let (cols, rows) = self.renderer.grid_size();
        let (cw, ch) = self.renderer.cell_px();
        Size {
            cols: cols.min(u16::MAX as usize) as u16,
            rows: rows.min(u16::MAX as usize) as u16,
            cell_w: cw.min(u16::MAX as u32) as u16,
            cell_h: ch.min(u16::MAX as u32) as u16,
        }
    }
    fn ipc_handle(&mut self, cmd: Command) -> Result<(), String> {
        match cmd {
            Command::Version => {
                let msg = format!("aeternus {}\r\n", env!("CARGO_PKG_VERSION"));
                self.with_term(|t| {
                    t.scroll_to_bottom();
                    t.selection_clear();
                    Parser::new().advance(t, msg.as_bytes());
                });
                self.window.request_redraw();
            }
            Command::Pref => self.run_action(Action::Settings),
            Command::Reload => self.run_action(Action::Reload),
        }
        Ok(())
    }
    fn shutdown(&mut self) {
        let handles: Vec<_> = self.tabs.iter_mut().filter_map(Tab::reap).collect();
        for h in handles {
            let _ = h.join();
        }
    }
    fn sync_tabs(&mut self) {
        let list: Vec<(u64, String)> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let title = tab.term.lock().map(|t| t.title.clone()).unwrap_or_default();
                let label = if title.trim().is_empty() { format!("Shell {}", i + 1) } else { title };
                (tab.id, label)
            })
            .collect();
        let active_id = self.active_tab().id;
        if self.renderer.set_tabs(&list, active_id) {
            self.resize_active();
        }
    }
    fn create_tab(&mut self, cwd: Option<PathBuf>) -> Result<(), String> {
        let size = self.pty_size();
        let argv = resolve_shell(&self.cfg);
        let term_env = terminfo::term_env(&self.cfg.terminal.term);
        let (pty, reader) = Pty::spawn(&argv, cwd.as_deref(), size, &term_env)
            .map_err(|e| format!("could not start {:?}: {e}", argv[0]))?;
        let mut t = Term::new(size.cols as usize, size.rows as usize);
        t.apply_config(&self.cfg);
        t.cell_px = self.renderer.cell_px();
        let term = Arc::new(Mutex::new(t));
        let id = self.next_id;
        self.next_id += 1;
        let dirty = Arc::new(AtomicBool::new(false));
        let want_lock = Arc::new(AtomicBool::new(false));
        let writer = pty.writer.clone();
        let proxy = self.proxy.clone();
        let (t2, d2, w2) = (term.clone(), dirty.clone(), want_lock.clone());
        let handle = thread::Builder::new()
            .name(format!("pty-reader-{id}"))
            .spawn(move || reader_loop(id, reader, t2, writer, d2, w2, proxy))
            .map_err(|e| format!("could not start the reader thread: {e}"))?;
        self.tabs.push(Tab {
            id,
            term,
            pty: Some(pty),
            size,
            dirty,
            want_lock,
            reader: handle.thread().clone(),
        });
        self.active = self.tabs.len() - 1;
        self.search_retarget();
        self.renderer.reset_trail();
        self.sync_tabs();
        self.renderer.ensure_tab_visible(id);
        self.window.request_redraw();
        Ok(())
    }
    fn new_tab(&mut self) {
        let cwd = self.tabs.get(self.active).and_then(Tab::cwd).or_else(|| {
            Some(self.cfg.terminal.working_directory.trim()).filter(|s| !s.is_empty()).map(PathBuf::from)
        });
        if let Err(e) = self.create_tab(cwd) {
            log::error(&format!("aeternus: {e}"));
        }
    }
    fn close_tab(&mut self, id: u64) {
        let Some(idx) = self.tabs.iter().position(|t| t.id == id) else { return };
        let mut tab = self.tabs.remove(idx);
        drop(tab.reap());
        if self.tabs.is_empty() {
            self.quit = true;
            return;
        }
        if idx < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.selecting = false;
        self.search_retarget();
        self.renderer.reset_trail();
        self.sync_tabs();
        let id = self.active_tab().id;
        self.renderer.ensure_tab_visible(id);
        self.resize_active();
        self.window.request_redraw();
    }
    fn switch_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() || idx == self.active {
            return;
        }
        self.active = idx;
        self.selecting = false;
        self.search_retarget();
        self.reset_blink();
        self.renderer.reset_trail();
        self.sync_tabs();
        let id = self.active_tab().id;
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
        let n = self.tabs.len() as isize;
        if n < 2 {
            return;
        }
        let next = (self.active as isize + dir).rem_euclid(n) as usize;
        self.switch_tab(next);
    }
    fn resize_active(&mut self) {
        let size = self.pty_size();
        let cell_px = self.renderer.cell_px();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let Ok(mut t) = tab.term.lock() else { return };
        t.cell_px = cell_px;
        if size.cols as usize != t.cols() || size.rows as usize != t.rows() {
            t.resize(size.cols as usize, size.rows as usize);
            if let Some(s) = self.search.as_mut() {
                s.mark_stale();
            }
        }
        drop(t);
        if size != tab.size {
            tab.size = size;
            if let Some(p) = &tab.pty {
                p.resize(size);
            }
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
                self.renderer.apply_config(&cfg);
                for tab in &self.tabs {
                    if let Ok(mut t) = tab.term.lock() {
                        t.apply_config(&cfg);
                    }
                }
                self.cfg = cfg;
                let size = self.window.inner_size();
                self.resize(size);
                log::info("aeternus: configuration reloaded");
            }
            Err(e) => log::error(&format!("aeternus: configuration error (keeping the current one):\n{e}")),
        }
        self.window.request_redraw();
    }
    fn settings_save(&mut self) {
        let mut cfg = self.cfg.clone();
        self.renderer.settings_writeback(&mut cfg);
        match config::save(&cfg) {
            Ok(()) => {
                self.config_mtime = config::watched_mtime(&cfg);
                self.reload_config();
            }
            Err(e) => log::error(&format!("aeternus: could not save the configuration: {e}")),
        }
        self.renderer.close_settings();
        self.window.request_redraw();
    }
    fn settings_outcome(&mut self, outcome: Option<SettingsOutcome>) {
        match outcome {
            Some(SettingsOutcome::Saved) => self.settings_save(),
            Some(SettingsOutcome::Cancelled) => self.renderer.close_settings(),
            None => {}
        }
        self.window.request_redraw();
    }
    fn copy_selection(&mut self, kind: ClipKind) -> bool {
        let text = self.with_term(|t| t.selection_text());
        if let (Some(text), Some(cb)) = (text, self.clipboard.as_mut()) {
            if !text.is_empty() {
                let _ = cb.set().clipboard(kind).text(text);
                return true;
            }
        }
        false
    }
    fn osc52_copy(&mut self, kind: u8, text: String) {
        if text.is_empty() {
            return;
        }
        let Some(cb) = self.clipboard.as_mut() else { return };
        let target = if kind == b'p' { ClipKind::Primary } else { ClipKind::Clipboard };
        let _ = cb.set().clipboard(target).text(text);
    }
    fn clipboard_text(&mut self, kind: ClipKind) -> Option<String> {
        self.clipboard.as_mut()?.get().clipboard(kind).text().ok()
    }
    fn paste(&mut self, kind: ClipKind) {
        let Some(text) = self.clipboard_text(kind) else { return };
        if self.renderer.settings_open() {
            self.renderer.settings_paste(&text);
            self.window.request_redraw();
            return;
        }
        if self.search.is_some() {
            let line = text.lines().next().unwrap_or("").replace(|c: char| c.is_control(), "");
            if !line.is_empty() {
                let term = self.term();
                let mut t = term.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(s) = self.search.as_mut() {
                    s.push_str(&mut t, &line);
                }
            }
            self.window.request_redraw();
            return;
        }
        let bracketed = self.with_term(|t| {
            t.scroll_to_bottom();
            t.selection_clear();
            t.bracketed_paste
        });
        let clean: String = text.chars().filter(|&c| c != '\x1b' && c != '\u{9b}').collect();
        let mut data = Vec::with_capacity(clean.len() + 16);
        if bracketed {
            data.extend_from_slice(b"\x1b[200~");
            data.extend_from_slice(clean.as_bytes());
            data.extend_from_slice(b"\x1b[201~");
        } else {
            data.extend_from_slice(clean.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
        }
        self.write(&data);
        self.window.request_redraw();
    }
    fn key_input(&mut self, event: &KeyEvent, mods: ModifiersState) {
        if event.state != ElementState::Pressed {
            return;
        }
        let key = key_input(event);
        if self.renderer.settings_open() {
            if self.wants_paste(event, &key, mods) {
                self.paste(ClipKind::Clipboard);
                return;
            }
            let outcome = self.renderer.settings_key(event, mods);
            self.settings_outcome(outcome);
            return;
        }
        if self.renderer.menu_open().is_some() {
            if let Key::Named(NamedKey::Escape) = &event.logical_key {
                self.close_menus();
            }
            return;
        }
        if self.search.is_some() {
            self.search_key(event, &key, mods);
            return;
        }
        if self.cfg.terminal.smart_copy_paste
            && mods.control_key()
            && !mods.shift_key()
            && !mods.alt_key()
            && !mods.super_key()
        {
            if let Key::Character(c) = &event.logical_key {
                if c.eq_ignore_ascii_case("c") && self.with_term(|t| t.selection_bounds().is_some()) {
                    self.copy_selection(ClipKind::Clipboard);
                    self.with_term(|t| t.selection_clear());
                    self.window.request_redraw();
                    return;
                }
                if c.eq_ignore_ascii_case("v") {
                    self.paste(ClipKind::Clipboard);
                    return;
                }
            }
        }
        let action =
            self.cfg.keymap.0.iter().find(|(c, _)| input::combo_matches(c, &key, mods)).map(|(_, a)| *a);
        if let Some(action) = action {
            if self.run_key_action(action) {
                return;
            }
        }
        let modes = self.with_term(|t| KeyModes {
            app_cursor: t.app_cursor_keys,
            app_keypad: t.app_keypad,
            kitty: t.kb_flags,
        });
        if let Some(bytes) = input::key_bytes(&key, mods, modes) {
            self.with_term(|t| {
                t.scroll_to_bottom();
                t.selection_clear();
            });
            self.reset_blink();
            self.write(&bytes);
            self.window.request_redraw();
        }
    }
    fn wants_paste(&self, event: &KeyEvent, key: &KeyInput, mods: ModifiersState) -> bool {
        self.cfg.keymap.0.iter().any(|(c, a)| *a == KeyAction::Paste && input::combo_matches(c, key, mods))
            || (mods.control_key()
                && !mods.shift_key()
                && matches!(&event.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("v")))
    }
    fn open_search(&mut self) {
        if self.search.is_none() {
            self.search = Some(Search::new());
        }
        self.window.request_redraw();
    }
    fn close_search(&mut self) {
        self.search = None;
        self.window.request_redraw();
    }
    fn search_retarget(&mut self) {
        if self.search.is_none() {
            return;
        }
        let term = self.term();
        let mut t = term.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = self.search.as_mut() {
            let q = std::mem::take(&mut s.query);
            s.set_query(&mut t, q);
        }
    }
    fn search_key(&mut self, event: &KeyEvent, key: &KeyInput, mods: ModifiersState) {
        if self.wants_paste(event, key, mods) {
            self.paste(ClipKind::Clipboard);
            return;
        }
        enum Edit<'a> {
            Step(bool),
            Pop,
            Clear,
            Push(&'a str),
        }
        let plain = !mods.control_key() && !mods.alt_key() && !mods.super_key();
        let edit = match &event.logical_key {
            Key::Named(NamedKey::Escape) => return self.close_search(),
            Key::Named(NamedKey::Enter) => Edit::Step(!mods.shift_key()),
            Key::Named(NamedKey::ArrowUp) => Edit::Step(true),
            Key::Named(NamedKey::ArrowDown) => Edit::Step(false),
            Key::Named(NamedKey::Backspace) if mods.control_key() => Edit::Clear,
            Key::Named(NamedKey::Backspace) => Edit::Pop,
            _ => {
                let action = self
                    .cfg
                    .keymap
                    .0
                    .iter()
                    .find(|(c, _)| input::combo_matches(c, key, mods))
                    .map(|(_, a)| *a);
                if let Some(action) = action {
                    if action != KeyAction::Search {
                        self.run_key_action(action);
                    }
                    return;
                }
                match event.text.as_deref() {
                    Some(t) if plain && !t.is_empty() && !t.chars().any(char::is_control) => Edit::Push(t),
                    _ => return,
                }
            }
        };
        let term = self.term();
        let mut t = term.lock().unwrap_or_else(|e| e.into_inner());
        let Some(s) = self.search.as_mut() else { return };
        match edit {
            Edit::Step(up) => s.step(&mut t, up),
            Edit::Pop => s.pop_char(&mut t),
            Edit::Clear => s.clear_query(&mut t),
            Edit::Push(text) => s.push_str(&mut t, text),
        }
        drop(t);
        self.window.request_redraw();
    }
    fn run_key_action(&mut self, action: KeyAction) -> bool {
        let a = match action {
            KeyAction::Copy => Action::Copy,
            KeyAction::Paste => Action::Paste,
            KeyAction::PastePrimary => Action::PastePrimary,
            KeyAction::ReloadConfig => Action::Reload,
            KeyAction::ZoomIn => Action::ZoomIn,
            KeyAction::ZoomOut => Action::ZoomOut,
            KeyAction::ZoomReset => Action::ZoomReset,
            KeyAction::NewTab => Action::NewTab,
            KeyAction::CloseTab => Action::CloseTab,
            KeyAction::NextTab => Action::NextTab,
            KeyAction::PrevTab => Action::PrevTab,
            KeyAction::Fullscreen => Action::Fullscreen,
            KeyAction::Preferences => Action::Settings,
            KeyAction::Search => Action::Find,
            KeyAction::PrevPrompt | KeyAction::NextPrompt | KeyAction::SelectOutput => {
                if self.with_term(|t| t.is_alt_screen()) {
                    return false;
                }
                match action {
                    KeyAction::PrevPrompt => Action::PrevPrompt,
                    KeyAction::NextPrompt => Action::NextPrompt,
                    _ => Action::SelectOutput,
                }
            }
            KeyAction::ScrollPageUp
            | KeyAction::ScrollPageDown
            | KeyAction::ScrollToTop
            | KeyAction::ScrollToBottom => {
                let changed = self.with_term(|t| {
                    if t.is_alt_screen() {
                        return None;
                    }
                    let page = t.rows() as isize - 1;
                    let delta = match action {
                        KeyAction::ScrollPageUp => page,
                        KeyAction::ScrollPageDown => -page,
                        KeyAction::ScrollToTop => isize::MAX,
                        _ => isize::MIN,
                    };
                    Some(t.scroll_display(delta))
                });
                return match changed {
                    None => false,
                    Some(c) => {
                        if c {
                            self.window.request_redraw();
                        }
                        true
                    }
                };
            }
        };
        self.run_action(a);
        true
    }
    fn mouse_input(&mut self, st: ElementState, button: MouseButton, mods: ModifiersState) {
        let pressed = st == ElementState::Pressed;
        let (mx, my) = self.mouse_pos;
        if self.renderer.settings_open() {
            if pressed && button == MouseButton::Left {
                let outcome = self.renderer.settings_click(mx, my);
                self.settings_outcome(outcome);
            } else if !pressed && button == MouseButton::Left && self.renderer.picker_release() {
                self.window.request_redraw();
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
        if pressed && self.renderer.tabs_visible() && self.tab_click(button, mx, my) {
            return;
        }
        if my <= self.renderer.top_inset() as f64 {
            if pressed && button == MouseButton::Left && my < self.renderer.toolbar_h() as f64 {
                self.menu_click(mx, my);
            }
            return;
        }
        let (row, col) = self.mouse_cell;
        let (mode, sgr) = self.with_term(|t| (t.mouse_mode, t.mouse_sgr));
        if pressed
            && button == MouseButton::Left
            && mods.control_key()
            && (mode == MouseMode::Off || mods.shift_key())
        {
            if let Some(url) = self.with_term(|t| t.url_at(row, col)) {
                open_url(&url);
                return;
            }
        }
        let reporting =
            (mode != MouseMode::Off && !mods.shift_key()) || (!pressed && self.report_button.is_some());
        if reporting {
            let code = match button {
                MouseButton::Left => 0,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                _ => return,
            };
            self.report_button = if pressed { Some(code) } else { None };
            if let Some(b) = input::mouse_report(sgr, code, row, col, !pressed, false, mods) {
                self.write(&b);
            }
            return;
        }
        match (button, pressed) {
            (MouseButton::Left, true) => {
                let now = Instant::now();
                if now.duration_since(self.last_click) < MULTI_CLICK && self.last_click_cell == (row, col) {
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
                self.with_term(|t| t.selection_start(row, col, sel_mode));
                self.selecting = true;
                self.window.request_redraw();
            }
            (MouseButton::Left, false) => {
                self.selecting = false;
                let has = self.with_term(|t| {
                    let has = t.selection_bounds().is_some();
                    if !has {
                        t.selection_clear();
                    }
                    has
                });
                if has {
                    self.copy_selection(ClipKind::Primary);
                }
                self.window.request_redraw();
            }
            (MouseButton::Middle, true) => self.paste(ClipKind::Primary),
            _ => {}
        }
    }
    fn tab_click(&mut self, button: MouseButton, mx: f64, my: f64) -> bool {
        if let Some(id) = self.renderer.hit_tab_close(mx, my) {
            if matches!(button, MouseButton::Left | MouseButton::Middle) {
                self.close_tab(id);
                return true;
            }
            return false;
        }
        match button {
            MouseButton::Middle => {
                if let Some(id) = self.renderer.hit_tab(mx, my) {
                    self.close_tab(id);
                    return true;
                }
            }
            MouseButton::Left => {
                if self.renderer.hit_tab_prev(mx, my) {
                    if self.renderer.tab_strip_scroll(-1) {
                        self.window.request_redraw();
                    }
                    return true;
                }
                if self.renderer.hit_tab_next(mx, my) {
                    if self.renderer.tab_strip_scroll(1) {
                        self.window.request_redraw();
                    }
                    return true;
                }
                if self.renderer.hit_new_tab(mx, my) {
                    self.new_tab();
                    return true;
                }
                if let Some(id) = self.renderer.hit_tab(mx, my) {
                    self.switch_tab_by_id(id);
                    return true;
                }
            }
            _ => {}
        }
        false
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
            Action::Copy => {
                self.copy_selection(ClipKind::Clipboard);
            }
            Action::Paste => self.paste(ClipKind::Clipboard),
            Action::PastePrimary => self.paste(ClipKind::Primary),
            Action::SelectAll => self.with_term(|t| t.selection_select_all()),
            Action::ZoomIn => self.set_zoom(self.renderer.zoom() + 0.1),
            Action::ZoomOut => self.set_zoom(self.renderer.zoom() - 0.1),
            Action::ZoomReset => self.set_zoom(1.0),
            Action::ScrollTop => {
                self.with_term(|t| t.scroll_display(isize::MAX));
            }
            Action::ScrollBottom => {
                self.with_term(|t| t.scroll_display(isize::MIN));
            }
            Action::ResetTerm => {
                self.with_term(|t| t.reset());
                if let Some(s) = self.search.as_mut() {
                    s.mark_stale();
                }
            }
            Action::Reload => self.reload_config(),
            Action::Settings => self.renderer.open_settings(&self.cfg),
            Action::Quit => self.quit = true,
            Action::NewTab => self.new_tab(),
            Action::CloseTab => {
                let id = self.active_tab().id;
                self.close_tab(id);
            }
            Action::NextTab => self.cycle_tab(1),
            Action::PrevTab => self.cycle_tab(-1),
            Action::Find => self.open_search(),
            Action::PrevPrompt => {
                self.with_term(|t| t.jump_to_prompt(true));
            }
            Action::NextPrompt => {
                self.with_term(|t| t.jump_to_prompt(false));
            }
            Action::SelectOutput => {
                let text = self.with_term(|t| t.select_command_output());
                if let (Some(text), Some(cb)) = (text, self.clipboard.as_mut()) {
                    if !text.is_empty() {
                        let _ = cb.set().clipboard(ClipKind::Primary).text(text);
                    }
                }
            }
            Action::Fullscreen => {
                let on = self.window.fullscreen().is_some();
                self.window.set_fullscreen(if on { None } else { Some(Fullscreen::Borderless(None)) });
            }
            Action::About => {
                let art = about_art();
                self.with_term(|t| {
                    t.scroll_to_bottom();
                    t.selection_clear();
                    Parser::new().advance(t, art.as_bytes());
                });
                self.write(b"\n");
            }
        }
        self.window.request_redraw();
    }
    fn update_link_hover(&mut self, mods: ModifiersState) {
        if self.renderer.settings_open() || self.renderer.menu_open().is_some() {
            return;
        }
        if self.mouse_pos.1 <= self.renderer.top_inset() as f64 {
            return;
        }
        let (row, col) = self.mouse_cell;
        let over = mods.control_key()
            && self.with_term(|t| {
                (t.mouse_mode == MouseMode::Off || mods.shift_key()) && t.url_at(row, col).is_some()
            });
        let icon = if over { CursorIcon::Pointer } else { self.term_pointer() };
        self.window.set_cursor(icon);
    }
    fn term_pointer(&self) -> CursorIcon {
        self.with_term(|t| t.pointer_shape()).unwrap_or(CursorIcon::Text)
    }
    fn pointer_over_term(&self) -> bool {
        !self.renderer.settings_open()
            && self.renderer.menu_open().is_none()
            && self.mouse_pos.1 > self.renderer.top_inset() as f64
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
        if let Some(open) = self.renderer.menu_open() {
            if y <= self.renderer.top_inset() as f64 {
                if let Some(t) = self.renderer.hit_title(x, y) {
                    if t != open {
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
            self.hover_chrome(x, y);
            return;
        }
        let mut ui_clear = self.renderer.set_hover_title(None);
        ui_clear |= self.renderer.set_hover_item(None);
        ui_clear |= self.renderer.set_hover_gear(false);
        ui_clear |= self.renderer.clear_hover();
        if ui_clear {
            self.window.set_cursor(self.term_pointer());
            self.window.request_redraw();
        }
        let cell = self.renderer.cell_at(x, y);
        let changed = cell != self.mouse_cell;
        self.mouse_cell = cell;
        if changed {
            self.update_link_hover(mods);
        }
        let (row, col) = cell;
        let (mode, sgr) = self.with_term(|t| (t.mouse_mode, t.mouse_sgr));
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
            if let Some(b) = input::mouse_report(sgr, btn, row, col, false, true, mods) {
                self.write(&b);
            }
            return;
        }
        if self.selecting {
            let h = self.window.inner_size().height as f64;
            let pad = self.renderer.pad() as f64;
            let top = self.renderer.top_inset() as f64;
            self.with_term(|t| {
                if y < top + pad {
                    t.scroll_display(1);
                } else if y > h - pad {
                    t.scroll_display(-1);
                }
                t.selection_update(row, col);
            });
            self.window.request_redraw();
        }
    }
    fn hover_chrome(&mut self, x: f64, y: f64) {
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
            let pointer = t.is_some() || n || c.is_some() || p || nx;
            self.window.set_cursor(if pointer { CursorIcon::Pointer } else { CursorIcon::Default });
            return;
        }
        let g = self.renderer.hit_gear(x, y);
        let mut changed = self.renderer.set_hover_gear(g);
        changed |= self.renderer.clear_hover();
        let t = self.renderer.hit_title(x, y);
        changed |= self.renderer.set_hover_title(t);
        if changed {
            self.window.request_redraw();
        }
        self.window.set_cursor(if t.is_some() || g { CursorIcon::Pointer } else { CursorIcon::Default });
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
        if my <= self.renderer.top_inset() as f64 {
            return;
        }
        let count = n.unsigned_abs().min(30);
        let (row, col) = self.mouse_cell;
        let (mode, sgr, alt, app_cursor, alt_scroll) = self
            .with_term(|t| (t.mouse_mode, t.mouse_sgr, t.is_alt_screen(), t.app_cursor_keys, t.alt_scroll));
        if mode != MouseMode::Off && !mods.shift_key() {
            let code = if n > 0 { 64 } else { 65 };
            let mut out = Vec::new();
            for _ in 0..count {
                if let Some(b) = input::mouse_report(sgr, code, row, col, false, false, mods) {
                    out.extend_from_slice(&b);
                }
            }
            self.write(&out);
        } else if alt && alt_scroll && !mods.shift_key() {
            let key = if n > 0 { b'A' } else { b'B' };
            let seq: [u8; 3] = if app_cursor { [0x1b, b'O', key] } else { [0x1b, b'[', key] };
            let out: Vec<u8> = seq.iter().copied().cycle().take(count * 3).collect();
            self.write(&out);
        } else if self.with_term(|t| t.scroll_display(n)) {
            self.window.request_redraw();
        }
    }
    fn focus_changed(&mut self, focused: bool) {
        self.focused = focused;
        self.reset_blink();
        if self.with_term(|t| t.focus_reporting) {
            self.write(if focused { b"\x1b[I" } else { b"\x1b[O" });
        }
        self.window.request_redraw();
    }
    fn wakeup(&mut self, id: u64) {
        let Some(idx) = self.tabs.iter().position(|t| t.id == id) else { return };
        let (bell, clip, notes) = match self.tabs[idx].term.lock() {
            Ok(mut t) => {
                (std::mem::take(&mut t.bell), t.clipboard_write.take(), std::mem::take(&mut t.notifications))
            }
            Err(_) => return,
        };
        if self.cfg.terminal.notifications {
            let visible = self.focused && idx == self.active;
            for n in notes.iter().filter(|n| !(n.only_unfocused && visible)) {
                notify::send(n, APP_ID);
            }
        }
        if let Some((kind, text)) = clip {
            self.osc52_copy(kind, text);
        }
        if bell {
            if self.cfg.bell.visual && idx == self.active {
                self.renderer.flash(Instant::now());
            }
            if self.cfg.bell.urgent && !self.focused {
                self.window.request_user_attention(Some(UserAttentionType::Informational));
            }
        }
        if idx == self.active {
            if let Some(s) = self.search.as_mut() {
                s.mark_stale();
            }
            self.window.request_redraw();
        } else {
            self.tabs[idx].dirty.store(false, Ordering::Release);
        }
    }
    fn redraw(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.sync_tabs();
        let tab = &self.tabs[self.active];
        tab.dirty.store(false, Ordering::Release);
        tab.want_lock.store(true, Ordering::Release);
        let term = tab.term.clone();
        let reader = tab.reader.clone();
        let mut skip_draw = false;
        let mut title = None;
        let pointer;
        {
            let mut t = term.lock().unwrap_or_else(|e| e.into_inner());
            tab.want_lock.store(false, Ordering::Release);
            reader.unpark();
            if !t.title.is_empty() && t.title != self.last_title {
                title = Some(t.title.clone());
            }
            pointer = t.pointer_shape();
            let now = Instant::now();
            t.expire_sync(now);
            if let Some(s) = self.search.as_mut() {
                if s.refresh_due().is_some_and(|due| due <= now) {
                    s.refresh(&t, now);
                }
            }
            if t.sync_output {
                skip_draw = true;
            } else {
                let view = View {
                    focused: self.focused,
                    blink_on: self.blink_on,
                    text_blink_on: self.text_blink_on,
                    search: self.search.as_ref(),
                };
                self.renderer.build(&t, &view);
            }
        }
        if let Some(title) = title {
            self.window.set_title(&title);
            self.last_title = title;
        }
        if pointer != self.last_pointer {
            self.last_pointer = pointer;
            if self.pointer_over_term() {
                self.window.set_cursor(pointer.unwrap_or(CursorIcon::Text));
            }
        }
        if !skip_draw {
            self.window.pre_present_notify();
            self.renderer.draw();
        }
    }
    fn tick(&mut self, now: Instant) -> Option<Instant> {
        if self.tabs.is_empty() {
            return None;
        }
        let (blinking, sync_expired, sync_deadline) = {
            let mut t = self.tabs[self.active].term.lock().unwrap_or_else(|e| e.into_inner());
            let expired = t.expire_sync(now);
            (self.focused && t.cursor_blink && t.cursor_visible, expired, t.sync_deadline())
        };
        if sync_expired {
            self.window.request_redraw();
        }
        let mut next_config_check = self.last_config_check + CONFIG_POLL_INTERVAL;
        if now >= next_config_check {
            self.last_config_check = now;
            next_config_check = now + CONFIG_POLL_INTERVAL;
            let mtime = config::watched_mtime(&self.cfg);
            if mtime != self.config_mtime {
                self.config_mtime = mtime;
                self.reload_config();
            }
        }
        let mut wake = Some(sync_deadline.map_or(next_config_check, |w| w.min(next_config_check)));
        let mut at = |t: Instant| wake = Some(wake.map_or(t, |w| w.min(t)));
        if blinking {
            let mut next = self.last_blink + BLINK_INTERVAL;
            if now >= next {
                self.blink_on = !self.blink_on;
                self.last_blink = now;
                next = now + BLINK_INTERVAL;
                self.window.request_redraw();
            }
            at(next);
        } else if !self.blink_on {
            self.blink_on = true;
            self.window.request_redraw();
        }
        if self.focused && self.renderer.blinking_text() {
            let mut next = self.last_text_blink + BLINK_INTERVAL;
            if now >= next {
                self.text_blink_on = !self.text_blink_on;
                self.last_text_blink = now;
                next = now + BLINK_INTERVAL;
                self.window.request_redraw();
            }
            at(next);
        } else if !self.text_blink_on {
            self.text_blink_on = true;
            self.window.request_redraw();
        }
        if let Some(end) = self.renderer.settings_anim_wake(now) {
            self.window.request_redraw();
            at((now + FRAME).min(end));
        }
        if self.renderer.trail_animating() {
            self.window.request_redraw();
            at(now + FRAME);
        }
        if let Some(due) = self.search.as_ref().and_then(Search::refresh_due) {
            if due <= now {
                self.window.request_redraw();
            }
            at(due.max(now));
        }
        if let Some(end) = self.renderer.flash_deadline() {
            self.window.request_redraw();
            at(if now < end { end } else { now + FRAME });
        }
        wake
    }
}

struct App {
    proxy: EventLoopProxy<UserEvent>,
    state: Option<State>,
    mods: ModifiersState,
    ipc: Option<IpcPaths>,
    error: Option<String>,
}

impl App {
    fn finish(&mut self, el: &ActiveEventLoop) {
        if self.state.as_ref().is_some_and(|s| s.quit) {
            el.exit();
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match State::new(el, self.proxy.clone()) {
            Ok(s) => self.state = Some(s),
            Err(e) => {
                self.error = Some(e);
                el.exit();
            }
        }
    }
    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Wakeup(id) => {
                if let Some(s) = self.state.as_mut().filter(|s| !s.quit) {
                    s.wakeup(id);
                }
            }
            UserEvent::CloseTab(id) => {
                if let Some(s) = self.state.as_mut().filter(|s| !s.quit) {
                    s.close_tab(id);
                }
            }
            UserEvent::Quit => {
                if let Some(s) = self.state.as_mut() {
                    s.quit = true;
                } else {
                    el.exit();
                }
            }
            UserEvent::Ipc(cmd, mut stream) => {
                let reply = match self.state.as_mut().filter(|s| !s.quit) {
                    Some(s) => match s.ipc_handle(cmd) {
                        Ok(()) => String::from("ok\n"),
                        Err(e) => format!("err {e}\n"),
                    },
                    None => String::from("err no window\n"),
                };
                let _ = stream.write_all(reply.as_bytes());
            }
        }
        self.finish(el);
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.finish(el);
        let Some(s) = self.state.as_mut().filter(|s| !s.quit) else { return };
        match s.tick(Instant::now()) {
            Some(w) => el.set_control_flow(ControlFlow::WaitUntil(w)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
        self.finish(el);
    }
    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(s) = self.state.as_mut().filter(|s| !s.quit) else { return };
        match event {
            WindowEvent::CloseRequested => s.quit = true,
            WindowEvent::Resized(size) => s.resize(size),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                s.renderer.set_scale(scale_factor as f32);
                let size = s.window.inner_size();
                s.resize(size);
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
                s.update_link_hover(self.mods);
            }
            WindowEvent::KeyboardInput { event, .. } => s.key_input(&event, self.mods),
            WindowEvent::MouseInput { state, button, .. } => s.mouse_input(state, button, self.mods),
            WindowEvent::CursorMoved { position, .. } => s.cursor_moved(position.x, position.y, self.mods),
            WindowEvent::MouseWheel { delta, .. } => s.wheel(delta, self.mods),
            WindowEvent::Focused(f) => {
                if f {
                    if let Some(ipc) = &self.ipc {
                        ipc.mark_focused();
                    }
                }
                s.focus_changed(f);
            }
            WindowEvent::RedrawRequested => s.redraw(),
            _ => {}
        }
        self.finish(el);
    }
    fn exiting(&mut self, _el: &ActiveEventLoop) {
        if let Some(ipc) = &self.ipc {
            ipc.cleanup();
        }
        if let Some(mut s) = self.state.take() {
            s.shutdown();
        }
    }
}

fn watch_signals(proxy: EventLoopProxy<UserEvent>) {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    match signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP]) {
        Ok(mut signals) => {
            thread::spawn(move || {
                if signals.forever().next().is_some() {
                    let _ = proxy.send_event(UserEvent::Quit);
                }
            });
        }
        Err(e) => log::error(&format!("aeternus: could not install signal handlers: {e}")),
    }
}

pub fn run() -> Result<(), String> {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|e| format!("could not connect to the display server: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let ipc = IpcPaths::bind(proxy.clone());
    watch_signals(proxy.clone());
    let mut app = App { proxy, state: None, mods: ModifiersState::default(), ipc, error: None };
    let result = event_loop.run_app(&mut app).map_err(|e| e.to_string());
    if let Some(ipc) = &app.ipc {
        ipc.cleanup();
    }
    if let Some(e) = app.error.take() {
        return Err(e);
    }
    result
}

pub fn cleanup_ipc_files() {
    IpcPaths { sock: ipc::socket_path(std::process::id()), last: ipc::last_path() }.cleanup();
}
