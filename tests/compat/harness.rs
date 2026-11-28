use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use aeternus::cell::Cell;
use aeternus::input::{key_bytes, mouse_report, paste_bytes, KeyInput, KeyModes};
use aeternus::parser::Parser;
use aeternus::pty::{Pty, Size};
use aeternus::term::{Term, WIDE_SPACER};
use aeternus::terminfo::TermEnv;
use winit::keyboard::{Key, KeyLocation, ModifiersState, NamedKey};

pub const TIMEOUT: Duration = Duration::from_secs(15);

pub fn have(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
        .unwrap_or(false)
}

#[macro_export]
macro_rules! require {
    ($($prog:expr),+) => {
        for p in [$($prog),+] {
            if !$crate::harness::have(p) {
                if std::env::var_os("AETERNUS_COMPAT_REQUIRE").is_some() {
                    panic!("{p} is not installed and AETERNUS_COMPAT_REQUIRE is set");
                }
                eprintln!("skipped: {p} is not installed");
                return;
            }
        }
    };
}

pub fn scratch_dir(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("aeternus-compat-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn term_env() -> TermEnv {
    static ENV: OnceLock<TermEnv> = OnceLock::new();
    ENV.get_or_init(|| {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/terminfo/xterm-aeternus.terminfo");
        let out = scratch_dir("terminfo");
        let ok = have("tic")
            && Command::new("tic").args(["-x", "-o"]).arg(&out).arg(&src).status().is_ok_and(|s| s.success());
        if ok {
            TermEnv { term: "xterm-aeternus".into(), terminfo: Some(out) }
        } else {
            TermEnv { term: "xterm-256color".into(), terminfo: None }
        }
    })
    .clone()
}

pub fn uses_aeternus_terminfo() -> bool {
    term_env().term == "xterm-aeternus"
}

struct Shared {
    term: Term,
    parser: Parser,
}

pub struct Session {
    pty: Pty,
    shared: Arc<Mutex<Shared>>,
    eof: Arc<AtomicBool>,
    read: Arc<AtomicUsize>,
    pub home: PathBuf,
}

impl Session {
    pub fn spawn(argv: &[&str], cols: usize, rows: usize) -> Self {
        Self::spawn_env(argv, &[], cols, rows)
    }

    pub fn spawn_env(argv: &[&str], env: &[(&str, &str)], cols: usize, rows: usize) -> Self {
        let home = scratch_dir(Path::new(argv[0]).file_name().and_then(|s| s.to_str()).unwrap_or("prog"));
        let home_s = home.to_str().unwrap().to_string();
        let mut full: Vec<String> = vec![
            "env".into(),
            "-u".into(),
            "TMUX".into(),
            "-u".into(),
            "NVIM".into(),
            "-u".into(),
            "VIMINIT".into(),
            "-u".into(),
            "LESS".into(),
            "-u".into(),
            "LESSOPEN".into(),
            "-u".into(),
            "LESSCLOSE".into(),
            "-u".into(),
            "MYVIMRC".into(),
            "-u".into(),
            "ZDOTDIR".into(),
            "-u".into(),
            "BASH_ENV".into(),
            "-u".into(),
            "PROMPT_COMMAND".into(),
            "-u".into(),
            "STARSHIP_CONFIG".into(),
            "-u".into(),
            "NVIM_APPNAME".into(),
            format!("HOME={home_s}"),
            format!("XDG_CONFIG_HOME={home_s}/.config"),
            format!("XDG_DATA_HOME={home_s}/.local/share"),
            format!("XDG_STATE_HOME={home_s}/.local/state"),
            format!("XDG_CACHE_HOME={home_s}/.cache"),
            format!("TMUX_TMPDIR={home_s}"),
            "LANG=C.UTF-8".into(),
            "LC_ALL=C.UTF-8".into(),
        ];
        full.extend(env.iter().map(|(k, v)| format!("{k}={v}")));
        full.extend(argv.iter().map(|s| s.to_string()));
        let size = Size { cols: cols as u16, rows: rows as u16, cell_w: 8, cell_h: 16 };
        let (pty, mut reader) = Pty::spawn(&full, Some(&home), size, &term_env()).expect("spawn");
        let mut term = Term::new(cols, rows);
        term.cell_px = (8, 16);
        let shared = Arc::new(Mutex::new(Shared { term, parser: Parser::new() }));
        let eof = Arc::new(AtomicBool::new(false));
        let read = Arc::new(AtomicUsize::new(0));
        let (s2, e2, r2, writer) = (shared.clone(), eof.clone(), read.clone(), pty.writer.clone());
        thread::spawn(move || {
            let mut buf = [0u8; 65536];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut s = s2.lock().unwrap();
                        let Shared { term, parser } = &mut *s;
                        parser.advance(term, &buf[..n]);
                        if !term.replies.is_empty() {
                            let _ = writer.send(std::mem::take(&mut term.replies));
                        }
                        r2.fetch_add(n, Ordering::SeqCst);
                    }
                }
            }
            e2.store(true, Ordering::SeqCst);
        });
        Self { pty, shared, eof, read, home }
    }

    pub fn with<R>(&self, f: impl FnOnce(&Term) -> R) -> R {
        f(&self.shared.lock().unwrap().term)
    }

    pub fn send(&self, bytes: impl AsRef<[u8]>) {
        let _ = self.pty.writer.send(bytes.as_ref().to_vec());
    }

    pub fn send_and_settle(&self, bytes: impl AsRef<[u8]>) {
        let before = self.read.load(Ordering::SeqCst);
        self.send(bytes);
        let deadline = Instant::now() + TIMEOUT;
        while self.read.load(Ordering::SeqCst) == before {
            if Instant::now() > deadline {
                panic!("timed out waiting for output\n{}", self.dump());
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.settle();
    }

    pub fn type_str(&self, s: &str) {
        for c in s.chars() {
            self.send(c.to_string());
            thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn key(&self, key: NamedKey) {
        self.key_mods(key, ModifiersState::empty());
    }

    pub fn key_mods(&self, key: NamedKey, mods: ModifiersState) {
        let modes = self.with(|t| KeyModes {
            app_cursor: t.app_cursor_keys,
            app_keypad: t.app_keypad,
            kitty: t.kb_flags,
        });
        let input =
            KeyInput { key: Key::Named(key), base: None, text: None, location: KeyLocation::Standard };
        let bytes = key_bytes(&input, mods, modes).expect("key has an encoding");
        self.send(bytes);
    }

    pub fn paste(&self, text: &str) {
        let bracketed = self.with(|t| t.bracketed_paste);
        self.send(paste_bytes(text, bracketed));
    }

    pub fn click(&self, row: usize, col: usize) {
        let sgr = self.with(|t| t.mouse_sgr);
        for release in [false, true] {
            let bytes = mouse_report(sgr, 0, row, col, release, false, ModifiersState::empty())
                .expect("position has an encoding");
            self.send(bytes);
        }
    }

    pub fn ctrl(&self, c: char) {
        self.send([(c.to_ascii_lowercase() as u8) & 0x1f]);
    }

    pub fn resize(&self, cols: usize, rows: usize) {
        self.shared.lock().unwrap().term.resize(cols, rows);
        self.pty.resize(Size { cols: cols as u16, rows: rows as u16, cell_w: 8, cell_h: 16 });
    }

    pub fn exited(&self) -> bool {
        self.eof.load(Ordering::SeqCst)
    }

    pub fn screen(&self) -> Vec<String> {
        self.with(screen_of)
    }

    pub fn row(&self, r: usize) -> String {
        self.with(|t| row_text(t, r))
    }

    pub fn dump(&self) -> String {
        self.with(|t| t.dump())
    }

    pub fn wait(&self, what: &str, pred: impl Fn(&Term) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if self.with(&pred) {
                return;
            }
            if Instant::now() > deadline {
                panic!("timed out waiting for {what}\n{}", self.dump());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn wait_text(&self, text: &str) {
        self.wait(&format!("{text:?} on screen"), |t| screen_of(t).iter().any(|l| l.contains(text)));
    }

    pub fn wait_row(&self, r: usize, text: &str) {
        self.wait(&format!("{text:?} on row {r}"), |t| r < t.rows() && row_text(t, r).contains(text));
    }

    pub fn wait_line(&self, line: &str) {
        self.wait(&format!("a line equal to {line:?}"), |t| screen_of(t).iter().any(|l| l == line));
    }

    pub fn wait_exit(&self) {
        let deadline = Instant::now() + TIMEOUT;
        while !self.exited() {
            if Instant::now() > deadline {
                panic!("timed out waiting for the program to exit\n{}", self.dump());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn settle(&self) {
        let mut last = self.dump();
        let mut stable_since = Instant::now();
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            thread::sleep(Duration::from_millis(30));
            let now = self.dump();
            if now != last {
                last = now;
                stable_since = Instant::now();
            } else if stable_since.elapsed() > Duration::from_millis(250) {
                return;
            }
        }
    }

    pub fn find_line(&self, line: &str) -> Option<usize> {
        self.screen().iter().position(|l| l == line)
    }

    pub fn find(&self, text: &str) -> Option<(usize, usize)> {
        self.with(|t| {
            (0..t.rows()).find_map(|r| {
                let cells = t.grid.row(r);
                let chars: Vec<(usize, char)> = cells
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.flags & WIDE_SPACER == 0)
                    .map(|(i, c)| (i, c.ch))
                    .collect();
                let s: String = chars.iter().map(|(_, c)| c).collect();
                s.find(text).map(|b| (r, chars[s[..b].chars().count()].0))
            })
        })
    }

    pub fn cell(&self, r: usize, c: usize) -> Cell {
        self.with(|t| t.grid.row(r)[c])
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.pty.child.kill();
        let _ = self.pty.child.wait();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

pub fn row_text(t: &Term, r: usize) -> String {
    let mut s = String::new();
    for c in t.grid.row(r).iter().filter(|c| c.flags & WIDE_SPACER == 0) {
        s.push(c.ch);
        if let Some(cl) = t.cluster(c) {
            s.push_str(cl);
        }
    }
    s.trim_end().to_string()
}

pub fn is_wide(c: char) -> bool {
    unicode_width::UnicodeWidthChar::width(c) == Some(2)
}

pub fn screen_of(t: &Term) -> Vec<String> {
    (0..t.rows()).map(|r| row_text(t, r)).collect()
}
