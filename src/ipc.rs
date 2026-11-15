use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Version,
    Pref,
    Reload,
}

impl Command {
    pub const ALL: [(&'static str, Command, &'static str); 3] = [
        ("version", Command::Version, "print the Aeternus version in the active terminal"),
        ("pref", Command::Pref, "open Preferences in the focused instance"),
        ("reload", Command::Reload, "reload config.toml in the focused instance"),
    ];
    pub fn parse(s: &str) -> Option<Command> {
        let s = s.trim();
        Self::ALL.iter().find(|(name, _, _)| *name == s).map(|(_, c, _)| *c)
    }
    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|(_, c, _)| *c == self).map(|(n, _, _)| *n).unwrap_or("")
    }
}

pub fn sanitize(tag: &str) -> String {
    tag.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

pub fn display_tag() -> String {
    let d = std::env::var("WAYLAND_DISPLAY")
        .or_else(|_| std::env::var("DISPLAY"))
        .unwrap_or_else(|_| String::from("default"));
    sanitize(&d)
}

pub fn runtime_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    let uid = std::fs::metadata("/proc/self").map(|m| {
        use std::os::unix::fs::MetadataExt;
        m.uid()
    });
    let dir = std::env::temp_dir().join(format!("aeternus-{}", uid.unwrap_or(0)));
    let _ = std::fs::create_dir_all(&dir);
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    dir
}

pub fn socket_prefix() -> String {
    format!("aeternus-{}-", display_tag())
}

pub fn socket_path(pid: u32) -> PathBuf {
    runtime_dir().join(format!("{}{pid}.sock", socket_prefix()))
}

pub fn last_path() -> PathBuf {
    runtime_dir().join(format!("aeternus-{}.last", display_tag()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_sanitize() {
        assert_eq!(sanitize("wayland-1"), "wayland_1");
        assert_eq!(sanitize(":0"), "_0");
        assert_eq!(sanitize("ab"), "ab");
    }

    #[test]
    fn parse_commands() {
        assert_eq!(Command::parse("version\n"), Some(Command::Version));
        assert_eq!(Command::parse("pref"), Some(Command::Pref));
        assert_eq!(Command::parse("reload "), Some(Command::Reload));
        assert_eq!(Command::parse("echo version"), None);
        assert_eq!(Command::parse(""), None);
        assert_eq!(Command::Reload.name(), "reload");
    }
}
