use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::log;
use crate::term::{Notification, Urgency};

const MAX_IN_FLIGHT: usize = 4;
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static MISSING: AtomicBool = AtomicBool::new(false);

pub fn args(n: &Notification, app_id: &str) -> Vec<String> {
    let urgency = match n.urgency {
        Urgency::Low => "low",
        Urgency::Normal => "normal",
        Urgency::Critical => "critical",
    };
    let mut v = vec![
        "--app-name=Aeternus".to_string(),
        format!("--icon={app_id}"),
        format!("--hint=string:desktop-entry:{app_id}"),
        format!("--urgency={urgency}"),
        "--".to_string(),
        clean(&n.title),
    ];
    let body = escape_markup(&clean(&n.body));
    if !body.is_empty() {
        v.push(body);
    }
    v
}

pub fn send(n: &Notification, app_id: &str) {
    if MISSING.load(Ordering::Relaxed) {
        return;
    }
    if IN_FLIGHT.fetch_add(1, Ordering::AcqRel) >= MAX_IN_FLIGHT {
        IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
        return;
    }
    let args = args(n, app_id);
    let spawned = std::thread::Builder::new().name("notify".into()).spawn(move || {
        let status = Command::new("notify-send")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if let Err(e) = status {
            if e.kind() == std::io::ErrorKind::NotFound && !MISSING.swap(true, Ordering::Relaxed) {
                log::error(
                    "aeternus: notify-send not found; install libnotify to show desktop notifications",
                );
            }
        }
        IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
    });
    if spawned.is_err() {
        IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}

fn clean(s: &str) -> String {
    s.chars().filter(|&c| c == '\n' || c == '\t' || !c.is_control()).collect::<String>().trim().to_string()
}

fn escape_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_notify_send_args() {
        let n = Notification {
            title: "-done\x1b".into(),
            body: "a < b & c".into(),
            urgency: Urgency::Critical,
            only_unfocused: false,
        };
        let a = args(&n, "app.id");
        assert_eq!(&a[3..], ["--urgency=critical", "--", "-done", "a &lt; b &amp; c"]);
        assert!(a.contains(&"--icon=app.id".to_string()));
        let n = Notification { body: String::new(), urgency: Urgency::Low, ..n };
        assert_eq!(args(&n, "app.id").last().map(String::as_str), Some("-done"));
    }
}
