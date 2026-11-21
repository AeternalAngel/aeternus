use std::path::{Path, PathBuf};

pub const NAME: &str = "xterm-aeternus";
pub const FALLBACK: &str = "xterm-256color";

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
