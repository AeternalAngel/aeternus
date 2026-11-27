use std::ops::Deref;
use std::path::PathBuf;
use std::process::Command;

use aeternus::cell::{flags, Color};
use aeternus::term::MouseMode;
use winit::keyboard::NamedKey;

use crate::harness::{screen_of, uses_aeternus_terminfo, Session};
use crate::require;

struct Tmux {
    s: Session,
    sock: PathBuf,
}

impl Deref for Tmux {
    type Target = Session;
    fn deref(&self) -> &Session {
        &self.s
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux").arg("-S").arg(&self.sock).arg("kill-server").output();
    }
}

impl Tmux {
    fn start() -> Self {
        let s = Session::spawn_env(
            &["sh", "-c", "exec tmux -f /dev/null -S \"$HOME/sock\" new-session -s t sh"],
            &[],
            80,
            24,
        );
        let sock = s.home.join("sock");
        let t = Tmux { s, sock };
        t.wait_row(23, "[t]");
        t.wait_row(0, "$");
        t
    }

    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").arg("-S").arg(&self.sock).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn run(&self, cmd: &str) {
        self.type_str(cmd);
        self.key(NamedKey::Enter);
    }

    fn prefix(&self, key: &str) {
        self.ctrl('b');
        self.send(key);
    }
}

#[test]
fn draws_status_line_and_runs_commands() {
    require!("tmux", "sh");
    let t = Tmux::start();
    assert!(t.with(|t| t.is_alt_screen()));
    t.run("echo in-$TERM");
    t.wait_line("in-tmux-256color");
    assert!(t.row(23).starts_with("[t] 0:"), "{}", t.dump());
}

#[test]
fn splits_panes_and_follows_mouse_clicks() {
    require!("tmux", "sh");
    let t = Tmux::start();
    t.prefix("\"");
    t.wait("a horizontal pane border", |term| {
        screen_of(term).iter().any(|l| l.chars().filter(|&c| c == '─').count() > 70)
    });
    let border = t.screen().iter().position(|l| l.chars().filter(|&c| c == '─').count() > 70).unwrap();
    t.prefix("%");
    t.wait("a vertical pane border", |term| {
        (border + 1..22).all(|r| term.grid.row(r).iter().any(|c| c.ch == '│'))
    });
    t.tmux(&["set", "-g", "mouse", "on"]);
    t.wait("tmux to enable mouse reporting", |term| term.mouse_mode != MouseMode::Off && term.mouse_sgr);
    t.send("\x1b[<0;5;3M\x1b[<0;5;3m");
    t.wait("the click to select the top pane", |_| t.tmux(&["display", "-p", "#{pane_index}"]) == "0");
    t.run("echo top-pane");
    t.wait_line("top-pane");
    assert!(t.find_line("top-pane").unwrap() < border);
}

#[test]
fn passes_truecolor_and_styled_underlines_through() {
    require!("tmux", "sh");
    if !uses_aeternus_terminfo() {
        eprintln!("skipped: needs xterm-aeternus for tmux to detect RGB and styled underlines");
        return;
    }
    let t = Tmux::start();
    t.run("printf '\\033[38;2;1;2;3mRGB\\033[0m \\033[4:3;58:2::200:100:50mCURLY\\033[0m\\n'");
    t.wait_line("RGB CURLY");
    let r = t.find_line("RGB CURLY").unwrap();
    assert_eq!(t.cell(r, 0).fg, Color::Rgb(1, 2, 3), "{}", t.dump());
    let c = t.cell(r, 4);
    assert_eq!(c.flags & flags::UNDERLINE_STYLE, flags::UNDERLINE_CURLY);
    assert_eq!(t.with(|term| term.underline_color(&c)), Some(Color::Rgb(200, 100, 50)));
}

#[test]
fn forwards_osc52_to_the_clipboard() {
    require!("tmux", "sh");
    let t = Tmux::start();
    t.tmux(&["set", "-s", "set-clipboard", "on"]);
    t.run("printf '\\033]52;c;%s\\a' $(printf copied-in-tmux | base64)");
    t.wait("tmux to forward OSC 52", |term| {
        term.clipboard_write.as_ref().is_some_and(|(_, text)| text.contains("copied-in-tmux"))
    });
}

#[test]
fn resizes_with_the_window() {
    require!("tmux", "sh");
    let t = Tmux::start();
    t.resize(100, 30);
    t.wait_row(29, "[t]");
    t.run("stty size");
    t.wait_line("29 100");
}

#[test]
fn leaves_the_alternate_screen_on_exit() {
    require!("tmux", "sh");
    let t = Tmux::start();
    t.run("exit");
    t.wait_text("[exited]");
    assert!(!t.with(|t| t.is_alt_screen()));
}
