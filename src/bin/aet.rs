use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use aeternus::ipc::{self, Command};

fn usage() {
    eprintln!("usage: aet <command>");
    eprintln!();
    eprintln!("commands:");
    for (name, _, help) in Command::ALL {
        eprintln!("  {name:<9}{help}");
    }
    eprintln!();
    eprintln!("options:");
    eprintln!("  -h, --help     show this help");
    eprintln!("  -V, --version  print the aet version");
}

fn candidates() -> Vec<PathBuf> {
    let dir = ipc::runtime_dir();
    let mut out = Vec::new();
    if let Ok(last) = fs::read_to_string(ipc::last_path()) {
        let p = PathBuf::from(last.trim());
        if p.exists() {
            out.push(p);
        }
    }
    let prefix = ipc::socket_prefix();
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
    socks.sort_by_key(|s| std::cmp::Reverse(s.0));
    for (_, p) in socks {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

fn run(cmd: Command) -> Result<(), String> {
    let list = candidates();
    if list.is_empty() {
        return Err(String::from("no running Aeternus instance found"));
    }
    let mut last = String::from("no instance answered");
    for path in list {
        let Ok(mut stream) = UnixStream::connect(&path) else {
            last = format!("no answer from {}", path.display());
            let _ = fs::remove_file(&path);
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
        let msg = format!("{}\n", cmd.name());
        if stream.write_all(msg.as_bytes()).is_err() {
            last = format!("could not send to {}", path.display());
            continue;
        }
        let mut r = BufReader::new(stream);
        let mut line = String::new();
        match r.read_line(&mut line) {
            Ok(0) => last = format!("no answer from {}", path.display()),
            Ok(_) => {
                let reply = line.trim_end();
                if reply == "ok" {
                    return Ok(());
                }
                return Err(reply.strip_prefix("err ").unwrap_or("Aeternus error").to_string());
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let cmd = match args.first().map(String::as_str) {
        Some("-h" | "--help" | "help") => {
            usage();
            return;
        }
        Some("-V" | "--version") => {
            println!("aet {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some(s) => match Command::parse(s) {
            Some(c) if args.len() == 1 => c,
            Some(_) => {
                eprintln!("aet: too many arguments");
                usage();
                std::process::exit(2);
            }
            None => {
                eprintln!("aet: unknown command: {s}");
                usage();
                std::process::exit(2);
            }
        },
        None => {
            usage();
            std::process::exit(2);
        }
    };
    if let Err(e) = run(cmd) {
        eprintln!("aet: {e}");
        std::process::exit(1);
    }
}
