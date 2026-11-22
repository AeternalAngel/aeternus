use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const NAME: &str = "xterm-aeternus";
pub const FALLBACK: &str = "xterm-256color";
const SOURCE: &str = include_str!("../packaging/terminfo/xterm-aeternus.terminfo");

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cap {
    Bool,
    Num(u32),
    Str(Vec<u8>),
}

struct Entry {
    name: String,
    caps: HashMap<String, Cap>,
}

fn entry() -> &'static Entry {
    static ENTRY: OnceLock<Entry> = OnceLock::new();
    ENTRY.get_or_init(|| parse_source(SOURCE))
}

pub fn capability(name: &str) -> Option<Cap> {
    let e = entry();
    match name {
        "TN" | "name" => Some(Cap::Str(e.name.clone().into_bytes())),
        "Co" => e.caps.get("colors").cloned(),
        "RGB" => e.caps.get("RGB").or_else(|| e.caps.get("Tc")).map(|_| Cap::Bool),
        n => e.caps.get(n).cloned(),
    }
}

fn parse_source(src: &str) -> Entry {
    let text: String =
        src.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ',' => fields.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    let mut fields = fields.into_iter().map(|f| f.trim().to_string());
    let name = fields.next().and_then(|n| n.split('|').next().map(str::to_string)).unwrap_or_default();
    let mut caps = HashMap::new();
    for f in fields.filter(|f| !f.is_empty() && !f.starts_with("use=") && !f.ends_with('@')) {
        if let Some((k, v)) = f.split_once('=') {
            caps.insert(k.to_string(), Cap::Str(unescape(v)));
        } else if let Some((k, v)) = f.split_once('#') {
            if let Some(n) = parse_num(v) {
                caps.insert(k.to_string(), Cap::Num(n));
            }
        } else {
            caps.insert(f, Cap::Bool);
        }
    }
    Entry { name, caps }
}

fn parse_num(v: &str) -> Option<u32> {
    if let Some(hex) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else if v.len() > 1 && v.starts_with('0') {
        u32::from_str_radix(&v[1..], 8).ok()
    } else {
        v.parse().ok()
    }
}

fn unescape(v: &str) -> Vec<u8> {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if i + 1 < b.len() => {
                i += 1;
                let c = b[i];
                match c {
                    b'E' | b'e' => out.push(0x1b),
                    b'n' | b'l' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b's' => out.push(b' '),
                    b'0'..=b'7' => {
                        let end = (i + 3).min(b.len());
                        let digits = b[i..end].iter().take_while(|d| (b'0'..=b'7').contains(d)).count();
                        let n = b[i..i + digits].iter().fold(0u32, |a, d| a * 8 + (d - b'0') as u32);
                        out.push(if n == 0 { 0x80 } else { n as u8 });
                        i += digits - 1;
                    }
                    c => out.push(c),
                }
            }
            b'^' if i + 1 < b.len() => {
                i += 1;
                out.push(if b[i] == b'?' { 0x7f } else { b[i] & 0x1f });
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermEnv {
    pub term: String,
    pub terminfo: Option<PathBuf>,
}

pub fn term_env(configured: &str) -> TermEnv {
    resolve(configured, &system_dirs(), &bundled_dirs())
}

pub fn resolve(configured: &str, system: &[PathBuf], bundled: &[PathBuf]) -> TermEnv {
    let configured = configured.trim();
    if !configured.is_empty() && configured != NAME {
        return TermEnv { term: configured.to_string(), terminfo: None };
    }
    if system.iter().any(|d| has_entry(d, NAME)) {
        return TermEnv { term: NAME.to_string(), terminfo: None };
    }
    if let Some(dir) = bundled.iter().find(|d| has_entry(d, NAME)) {
        return TermEnv { term: NAME.to_string(), terminfo: Some(dir.clone()) };
    }
    let term = if configured.is_empty() { FALLBACK } else { NAME };
    TermEnv { term: term.to_string(), terminfo: None }
}

fn has_entry(dir: &Path, name: &str) -> bool {
    let Some(first) = name.chars().next() else { return false };
    let hex = format!("{:02x}", first as u32);
    [first.to_string(), hex].iter().any(|sub| dir.join(sub).join(name).is_file())
}

fn system_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(t) = std::env::var_os("TERMINFO").filter(|t| !t.is_empty()) {
        dirs.push(PathBuf::from(t));
    }
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        dirs.push(PathBuf::from(home).join(".terminfo"));
    }
    if let Some(list) = std::env::var_os("TERMINFO_DIRS") {
        dirs.extend(std::env::split_paths(&list).filter(|p| !p.as_os_str().is_empty()));
    }
    dirs.extend(
        ["/etc/terminfo", "/lib/terminfo", "/usr/share/terminfo", "/usr/lib/terminfo"].map(PathBuf::from),
    );
    dirs
}

fn bundled_dirs() -> Vec<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent()?.parent().map(|p| p.join("share").join("terminfo")))
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!("aeternus-terminfo-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn with_entry(self, sub: &str) -> Self {
            std::fs::create_dir_all(self.0.join(sub)).unwrap();
            std::fs::write(self.0.join(sub).join(NAME), b"").unwrap();
            self
        }
    }
    fn one(d: &TempDir) -> &[PathBuf] {
        std::slice::from_ref(&d.0)
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn parses_the_bundled_entry() {
        assert_eq!(capability("TN"), Some(Cap::Str(NAME.as_bytes().to_vec())));
        assert_eq!(capability("Co"), Some(Cap::Num(256)));
        assert_eq!(capability("colors"), Some(Cap::Num(256)));
        assert_eq!(capability("Tc"), Some(Cap::Bool));
        assert_eq!(capability("RGB"), Some(Cap::Bool));
        assert_eq!(capability("Smulx"), Some(Cap::Str(b"\x1b[4:%p1%dm".to_vec())));
        assert_eq!(capability("Cr"), Some(Cap::Str(b"\x1b]112\x07".to_vec())));
        assert_eq!(capability("Se"), Some(Cap::Str(b"\x1b[0 q".to_vec())));
        assert_eq!(capability("use"), None);
        assert_eq!(capability("kcuu1"), None);
    }

    #[test]
    fn unescapes_terminfo_strings() {
        assert_eq!(unescape(r"\E[\,\:\^\\x^G^?\0\s\101"), b"\x1b[,:^\\x\x07\x7f\x80 A");
        let e = parse_source("# c\nfoo|Foo,\n\tam, it#8, xx@, ht=^I, ab=a\\,b,\n\tuse=bar,\n");
        assert_eq!(e.name, "foo");
        assert_eq!(e.caps.get("am"), Some(&Cap::Bool));
        assert_eq!(e.caps.get("it"), Some(&Cap::Num(8)));
        assert_eq!(e.caps.get("ht"), Some(&Cap::Str(b"\t".to_vec())));
        assert_eq!(e.caps.get("ab"), Some(&Cap::Str(b"a,b".to_vec())));
        assert!(!e.caps.contains_key("xx") && !e.caps.contains_key("use"));
    }

    #[test]
    fn falls_back_without_the_entry() {
        let empty = TempDir::new("empty");
        let env = resolve("", one(&empty), one(&empty));
        assert_eq!(env, TermEnv { term: FALLBACK.into(), terminfo: None });
    }

    #[test]
    fn uses_the_system_entry() {
        let sys = TempDir::new("sys").with_entry("x");
        let env = resolve("", one(&sys), &[]);
        assert_eq!(env, TermEnv { term: NAME.into(), terminfo: None });
    }

    #[test]
    fn finds_hashed_directories() {
        let sys = TempDir::new("hashed").with_entry("78");
        assert_eq!(resolve("", one(&sys), &[]).term, NAME);
    }

    #[test]
    fn bundled_entry_sets_terminfo() {
        let empty = TempDir::new("none");
        let bundled = TempDir::new("bundled").with_entry("x");
        let env = resolve("", one(&empty), one(&bundled));
        assert_eq!(env, TermEnv { term: NAME.into(), terminfo: Some(bundled.0.clone()) });
    }

    #[test]
    fn configured_value_wins() {
        let sys = TempDir::new("configured").with_entry("x");
        let env = resolve(" screen-256color ", one(&sys), &[]);
        assert_eq!(env, TermEnv { term: "screen-256color".into(), terminfo: None });
        let empty = TempDir::new("forced");
        assert_eq!(resolve(NAME, one(&empty), &[]).term, NAME);
    }
}
