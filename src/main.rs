#![allow(dead_code)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod bench;
mod cell;
mod colors;
mod config;
mod grid;
mod parser;
mod pty;
mod render;
mod term;

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use arboard::{Clipboard, GetExtLinux, LinuxClipboardKind, SetExtLinux};
#[cfg(windows)]
use arboard::Clipboard;

#[cfg(windows)]
enum ClipKind {
    Clipboard,
    Primary,
}
#[cfg(unix)]
type ClipKind = LinuxClipboardKind;

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};
#[cfg(unix)]
use winit::platform::wayland::WindowAttributesExtWayland;
#[cfg(unix)]
use winit::platform::x11::WindowAttributesExtX11;

use config::Config;
use parser::Parser;
use pty::Pty;
use render::{Renderer, View};
use term::{MouseMode, SelMode, Term};

const BLINK_INTERVAL: Duration = Duration::from_millis(530);
const MULTI_CLICK: Duration = Duration::from_millis(400);

#[derive(Debug)]
enum UserEvent {
    Wakeup,
    Exit,
}

struct State {
    window: Arc<Window>,
    renderer: Renderer,
    term: Arc<Mutex<Term>>,
    pty: Pty,
    dirty: Arc<AtomicBool>,
    want_lock: Arc<AtomicBool>,
    last_title: String,
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
}

impl State {
    fn new(el: &ActiveEventLoop, proxy: EventLoopProxy<UserEvent>) -> Self {
        let cfg = config::load().unwrap_or_else(|e| {
            eprintln!("aeternus: erro na configuração, usando os padrões:\n{e}");
            Config::default()
        });

        let attrs = Window::default_attributes()
            .with_title("aeternus")
            .with_inner_size(LogicalSize::new(
                cfg.window.width as f64,
                cfg.window.height as f64,
            ));
        #[cfg(unix)]
        let attrs = WindowAttributesExtWayland::with_name(attrs, "aeternus", "aeternus");
        #[cfg(unix)]
        let attrs = WindowAttributesExtX11::with_name(attrs, "aeternus", "aeternus");
        let window = Arc::new(el.create_window(attrs).expect("janela"));
        window.set_cursor(CursorIcon::Text);
        let renderer = Renderer::new(window.clone(), &cfg);
        let (cols, rows) = renderer.grid_size();

        let mut t = Term::new(cols, rows);
        t.apply_config(&cfg);
        let term = Arc::new(Mutex::new(t));

        let shell = if cfg.terminal.shell.trim().is_empty() {
        #[cfg(windows)]
        let fallback = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
        #[cfg(not(windows))]
        let fallback = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            fallback
        } else {
            cfg.terminal.shell.trim().to_string()
        };
        let (pty, mut reader) =
            Pty::spawn(&shell, &cfg.terminal.working_directory, cols as u16, rows as u16)
                .expect("pty");
        let dirty = Arc::new(AtomicBool::new(false));
        let want_lock = Arc::new(AtomicBool::new(false));

        let writer = pty.writer.clone();
        let (t, d, wl) = (term.clone(), dirty.clone(), want_lock.clone());
        thread::spawn(move || {
            let mut parser = Parser::new();
            let mut buf = vec![0u8; 65536];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        while wl.load(Ordering::Relaxed) {
                            thread::yield_now();
                        }
                        let replies = {
                            let mut term = t.lock().unwrap();
                            parser.advance(&mut *term, &buf[..n]);
                            std::mem::take(&mut term.replies)
                        };
                        if !replies.is_empty() {
                            let _ = writer.lock().unwrap().write_all(&replies);
                        }
                        if !d.swap(true, Ordering::AcqRel) {
                            let _ = proxy.send_event(UserEvent::Wakeup);
                        }
                    }
                }
            }
            let _ = proxy.send_event(UserEvent::Exit);
        });

        window.request_redraw();
        let now = Instant::now();
        Self {
            window,
            renderer,
            term,
            pty,
            dirty,
            want_lock,
            last_title: String::new(),
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
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        let _ = self.pty.writer.lock().unwrap().write_all(bytes);
    }

    fn reset_blink(&mut self) {
        self.blink_on = true;
        self.last_blink = Instant::now();
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.renderer.resize(size.width, size.height);
        let (cols, rows) = self.renderer.grid_size();
        {
            let mut t = self.term.lock().unwrap();
            if cols != t.cols() || rows != t.rows() {
                t.resize(cols, rows);
                self.pty.resize(cols as u16, rows as u16);
            }
        }
        self.window.request_redraw();
    }

    fn set_zoom(&mut self, z: f32) {
        self.renderer.set_zoom(z);
        let size = self.window.inner_size();
        self.resize(size);
    }

    /// Relê o config.toml e aplica fonte, cores, padding, cursor e scrollback.
    /// Tamanho inicial da janela e shell só valem ao abrir. Com erro, mantém a config atual.
    fn reload_config(&mut self) {
        match config::load() {
            Ok(cfg) => {
                self.renderer.apply_config(&cfg);
                self.term.lock().unwrap().apply_config(&cfg);
                let size = self.window.inner_size();
                self.resize(size);
                eprintln!("aeternus: configuração recarregada");
            }
            Err(e) => eprintln!("aeternus: erro na configuração (mantida a atual):\n{e}"),
        }
        self.window.request_redraw();
    }

    #[cfg_attr(windows, allow(unused_variables))]
    fn copy_selection(&mut self, kind: ClipKind) {
        let text = self.term.lock().unwrap().selection_text();
        if let (Some(text), Some(cb)) = (text, self.clipboard.as_mut()) {
            if !text.is_empty() {
                #[cfg(unix)]
                let _ = cb.set().clipboard(kind).text(text);
                #[cfg(windows)]
                let _ = cb.set_text(text);
            }
        }
    }
    
    #[cfg_attr(windows, allow(unused_variables))]
    fn osc52_copy(&mut self, kind: u8, text: String) {
        if text.is_empty() {
            return;
        }
        let Some(cb) = self.clipboard.as_mut() else { return };
        #[cfg(unix)]
        {
            let target = if kind == b'p' { LinuxClipboardKind::Primary } else { LinuxClipboardKind::Clipboard };
            let _ = cb.set().clipboard(target).text(text);
        }
        #[cfg(windows)]
        let _ = cb.set_text(text);
    }

    #[cfg_attr(windows, allow(unused_variables))]
    fn paste(&mut self, kind: ClipKind) {
        let bracketed = {
            let mut t = self.term.lock().unwrap();
            t.scroll_to_bottom();
            t.selection_clear();
            t.bracketed_paste
        };
        let clean = {
            let Some(cb) = self.clipboard.as_mut() else { return };
            #[cfg(unix)]
            let text = match cb.get().clipboard(kind).text() {
                Ok(t) => t,
                Err(_) => return,
            };
            #[cfg(windows)]
            let text = match cb.get_text() {
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

        if mods.control_key() && !mods.shift_key() && !mods.alt_key() && !mods.super_key() {
            if let Key::Character(c) = &event.logical_key {
                match c.as_str().to_lowercase().as_str() {
                    "c" => {
                        let has_selection =
                            self.term.lock().unwrap().selection_bounds().is_some();
                        if has_selection {
                            self.copy_selection(ClipKind::Clipboard);
                            self.term.lock().unwrap().selection_clear();
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

        let mut t = self.term.lock().unwrap();

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

        if let Some(bytes) = key_bytes(event, mods, t.app_cursor_keys) {
            t.scroll_to_bottom();
            t.selection_clear();
            drop(t);
            self.reset_blink();
            self.write(&bytes);
            self.window.request_redraw();
        }
    }


    fn mouse_input(&mut self, st: ElementState, button: MouseButton, mods: ModifiersState) {
        let pressed = st == ElementState::Pressed;
        let (row, col) = self.mouse_cell;

        let mode = self.term.lock().unwrap().mouse_mode;
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
                let t = self.term.lock().unwrap();
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
                self.term.lock().unwrap().selection_start(row, col, sel_mode);
                self.selecting = true;
                self.window.request_redraw();
            }
            (MouseButton::Left, false) => {
                self.selecting = false;
                let has = {
                    let mut t = self.term.lock().unwrap();
                    let has = t.selection_bounds().is_some();
                    if !has {
                        t.selection_clear();
                    }
                    has
                };
                #[cfg(not(windows))]
                if has {
                    self.copy_selection(ClipKind::Primary);
                }
                self.window.request_redraw();
            }
            (MouseButton::Middle, true) => self.paste(ClipKind::Primary),
            _ => {}
        }
    }

    fn cursor_moved(&mut self, x: f64, y: f64, mods: ModifiersState) {
        self.mouse_pos = (x, y);
        let cell = self.renderer.cell_at(x, y);
        let changed = cell != self.mouse_cell;
        self.mouse_cell = cell;
        let (row, col) = cell;

        let mode = self.term.lock().unwrap().mouse_mode;
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
                let t = self.term.lock().unwrap();
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
            let mut t = self.term.lock().unwrap();
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
        let count = n.unsigned_abs().min(30);
        let (row, col) = self.mouse_cell;

        let (mode, alt, app_cursor, alt_scroll) = {
            let t = self.term.lock().unwrap();
            (t.mouse_mode, t.is_alt_screen(), t.app_cursor_keys, t.alt_scroll)
        };

        if mode != MouseMode::Off && !mods.shift_key() {
            let code = if n > 0 { 64 } else { 65 };
            let mut out = Vec::new();
            {
                let t = self.term.lock().unwrap();
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
        } else if self.term.lock().unwrap().scroll_display(n) {
            self.window.request_redraw();
        }
    }

    fn focus_changed(&mut self, focused: bool) {
        self.focused = focused;
        self.reset_blink();
        let report = self.term.lock().unwrap().focus_reporting;
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
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.state.is_none() {
            self.state = Some(State::new(el, self.proxy.clone()));
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Wakeup => {
                if let Some(s) = &self.state {
                    s.window.request_redraw();
                }
            }
            UserEvent::Exit => std::process::exit(0),
        }
    }

    /// Controla o piscar do cursor: acorda só quando precisa alternar.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(s) = self.state.as_mut() else { return };
        let blinking = {
            let t = s.term.lock().unwrap();
            s.focused && t.cursor_blink && t.cursor_visible
        };
        if blinking {
            let now = Instant::now();
            let mut next = s.last_blink + BLINK_INTERVAL;
            if now >= next {
                s.blink_on = !s.blink_on;
                s.last_blink = now;
                next = now + BLINK_INTERVAL;
                s.window.request_redraw();
            }
            el.set_control_flow(ControlFlow::WaitUntil(next));
        } else {
            if !s.blink_on {
                s.blink_on = true;
                s.window.request_redraw();
            }
            el.set_control_flow(ControlFlow::Wait);
        }
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(s) = self.state.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => {
                let _ = s.pty.child.kill();
                std::process::exit(0);
            }
            WindowEvent::Resized(size) => s.resize(size),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } => s.key_input(&event, self.mods),
            WindowEvent::MouseInput { state, button, .. } => {
                s.mouse_input(state, button, self.mods)
            }
            WindowEvent::CursorMoved { position, .. } => {
                s.cursor_moved(position.x, position.y, self.mods)
            }
            WindowEvent::MouseWheel { delta, .. } => s.wheel(delta, self.mods),
            WindowEvent::Focused(f) => s.focus_changed(f),
            WindowEvent::RedrawRequested => {
                s.dirty.store(false, Ordering::Release);
                s.want_lock.store(true, Ordering::Relaxed);
                let mut skip_draw = false;
                let clip;
                {
                    let mut t = s.term.lock().unwrap();
                    s.want_lock.store(false, Ordering::Relaxed);
                    if !t.title.is_empty() && t.title != s.last_title {
                        s.window.set_title(&t.title);
                        s.last_title = t.title.clone();
                    }
                    clip = t.clipboard_write.take();
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

/// Codifica um evento de mouse para o app (SGR 1006 ou o formato X10 clássico).
/// `button`: 0/1/2 = esquerdo/meio/direito, 3 = nenhum, 64/65 = roda cima/baixo.
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

fn key_bytes(ev: &KeyEvent, mods: ModifiersState, app_cursor: bool) -> Option<Vec<u8>> {
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
            NamedKey::Space => b" ".to_vec(),
            _ => ev.text.as_ref()?.as_bytes().to_vec(),
        },
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--bench") {
        bench::run_all();
        return Ok(());
    }

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        proxy: event_loop.create_proxy(),
        state: None,
        mods: ModifiersState::default(),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
