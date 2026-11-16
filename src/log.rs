use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG: u64 = 1 << 20;

static LOG: OnceLock<Mutex<Option<File>>> = OnceLock::new();

pub fn log_path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".local").join("state"),
    };
    Some(base.join("aeternus").join("aeternus.log"))
}

pub fn init() {
    let file = log_path().and_then(|path| {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok()?;
        }
        let too_big = std::fs::metadata(&path).map(|m| m.len() > MAX_LOG).unwrap_or(false);
        let mut opts = OpenOptions::new();
        opts.create(true);
        if too_big {
            opts.write(true).truncate(true);
        } else {
            opts.append(true);
        }
        opts.open(&path).ok()
    });
    let _ = LOG.set(Mutex::new(file));
}

fn write(level: &str, msg: &str) {
    eprintln!("{msg}");
    let Some(lock) = LOG.get() else { return };
    let Ok(mut guard) = lock.lock() else { return };
    if let Some(f) = guard.as_mut() {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "[{secs}] {level} {msg}");
    }
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

pub fn error(msg: &str) {
    write("ERROR", msg);
}

pub fn fatal(msg: &str) {
    write("FATAL", msg);
    let mut body = msg.to_string();
    if let Some(p) = log_path() {
        body.push_str(&format!("\n\nLog: {}", p.display()));
    }
    let _ = std::process::Command::new("notify-send")
        .arg("--app-name=Aeternus")
        .arg("--urgency=critical")
        .arg("Aeternus failed to start")
        .arg(body)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
