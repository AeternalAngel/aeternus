use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn usage() {
    eprintln!("uso: aet [version|pref|reload]");
    eprintln!("  version  mostra a versão do aeternus no terminal ativo");
    eprintln!("  pref     abre as preferências da instância focada");
    eprintln!("  reload   recarrega o config.toml da instância focada");
}

fn sanitize(tag: &str) -> String {
    tag.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

fn display_tag() -> String {
    let d = env::var("WAYLAND_DISPLAY")
        .or_else(|_| env::var("DISPLAY"))
        .unwrap_or_else(|_| String::from("default"));
    sanitize(&d)
}

fn candidates() -> Vec<PathBuf> {
    let dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let tag = display_tag();
    let mut out = Vec::new();
    if let Ok(last) = fs::read_to_string(dir.join(format!("aeternus-{tag}.last"))) {
        let p = PathBuf::from(last.trim());
        if p.is_file() {
            out.push(p);
        }
    }
    let prefix = format!("aeternus-{tag}-");
    let mut socks: Vec<(SystemTime, PathBuf)> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !name.starts_with(&prefix) || !name.ends_with(".sock") {
                return None;
            }
            let mt = e.metadata().ok()?.modified().ok()?;
            Some((mt, e.path()))
        })
        .collect();
    socks.sort_by(|a, b| b.0.cmp(&a.0));
    out.extend(socks.into_iter().map(|(_, p)| p));
    out
}

fn run(cmd: &str) -> Result<(), String> {
    let list = candidates();
    if list.is_empty() {
        return Err(String::from("nenhuma instância do aeternus em execução"));
    }
    let mut last = String::from("nenhuma instância respondeu");
    for path in list {
        let Ok(mut stream) = UnixStream::connect(&path) else {
            last = format!("sem resposta em {}", path.display());
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
        let mut msg = Vec::new();
        msg.extend_from_slice(cmd.as_bytes());
        msg.push(b'\n');
        if stream.write_all(&msg).is_err() {
            last = format!("falha ao enviar para {}", path.display());
            continue;
        }
        let mut r = BufReader::new(stream);
        let mut line = String::new();
        match r.read_line(&mut line) {
            Ok(0) => last = format!("sem resposta de {}", path.display()),
            Ok(_) => {
                let reply = line.trim_end();
                if reply == "ok" {
                    return Ok(());
                }
                return Err(reply.strip_prefix("err ").unwrap_or("erro do aeternus").to_string());
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

fn main() {
    let arg = env::args().nth(1);
    match arg.as_deref() {
        Some("version") | Some("pref") | Some("reload") => {}
        Some(_) => {
            eprintln!("aet: comando desconhecido");
            usage();
            std::process::exit(2);
        }
        None => {
            usage();
            std::process::exit(2);
        }
    }
    let cmd = arg.unwrap();
    if let Err(e) = run(&cmd) {
        eprintln!("aet: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize;
    #[test]
    fn tag_sanitize() {
        assert_eq!(sanitize("wayland-1"), "wayland_1");
        assert_eq!(sanitize(":0"), "_0");
        assert_eq!(sanitize("ab"), "ab");
    }
}
