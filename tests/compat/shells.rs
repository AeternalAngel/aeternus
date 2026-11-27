use std::time::{Duration, Instant};

use winit::keyboard::NamedKey;

use crate::harness::Session;
use crate::require;

const PROMPT: &str = "aet$ ";

fn start(shell: &str) -> Session {
    let s = match shell {
        "bash" => Session::spawn_env(&["bash", "--norc", "--noprofile", "-i"], &[("PS1", PROMPT)], 80, 24),
        "zsh" => Session::spawn_env(&["zsh", "-f", "-i"], &[("PS1", PROMPT)], 80, 24),
        "fish" => Session::spawn(
            &[
                "fish",
                "--no-config",
                "-i",
                "-C",
                "function fish_prompt; printf 'aet$ '; end; set -g fish_greeting",
            ],
            80,
            24,
        ),
        _ => Session::spawn_env(&[shell, "-i"], &[("PS1", PROMPT), ("ENV", "")], 80, 24),
    };
    s.wait_text(PROMPT.trim_end());
    s.settle();
    s
}

fn run(s: &Session, cmd: &str) {
    s.type_str(cmd);
    s.key(NamedKey::Enter);
}

pub fn runs_commands(shell: &str) {
    let s = start(shell);
    run(&s, "echo hello-world | tr o 0");
    s.wait_text("hell0-w0rld");
    run(&s, "printf '%s\\n' one two three");
    s.wait_line("three");
    let one = s.find_line("one").unwrap();
    assert_eq!(s.row(one + 1), "two");
    assert_eq!(s.row(one + 2), "three");
}

pub fn line_editing(shell: &str) {
    let s = start(shell);
    s.type_str("echo 1234");
    s.key(NamedKey::ArrowLeft);
    s.key(NamedKey::ArrowLeft);
    s.type_str("-");
    s.key(NamedKey::Enter);
    s.wait_text("12-34");
    s.type_str("echo abcX");
    s.key(NamedKey::Backspace);
    s.type_str("d");
    if shell == "zsh" {
        s.ctrl('a');
    } else {
        s.key(NamedKey::Home);
    }
    s.type_str(" ");
    if shell == "zsh" {
        s.ctrl('e');
    } else {
        s.key(NamedKey::End);
    }
    s.type_str("e");
    s.key(NamedKey::Enter);
    s.wait_line("abcde");
}

pub fn history(shell: &str) {
    let s = start(shell);
    run(&s, "echo first-x | tr x 2");
    s.wait_text("first-2");
    s.key(NamedKey::ArrowUp);
    s.key(NamedKey::Enter);
    s.wait(r#"the recalled command to run again"#, |t| {
        crate::harness::screen_of(t).iter().filter(|l| *l == "first-2").count() == 2
    });
}

pub fn tab_completion(shell: &str) {
    let s = start(shell);
    std::fs::write(s.home.join("completeme-7781.txt"), "").unwrap();
    s.type_str("ls complet");
    s.key(NamedKey::Tab);
    s.wait_text("ls completeme-7781.txt");
}

pub fn resize(shell: &str) {
    let s = start(shell);
    run(&s, "stty size");
    s.wait_text("24 80");
    s.resize(100, 30);
    s.settle();
    run(&s, "stty size");
    s.wait_text("30 100");
}

pub fn interrupt_and_exit(shell: &str) {
    let s = start(shell);
    run(&s, "sleep 30");
    std::thread::sleep(Duration::from_millis(200));
    let t0 = Instant::now();
    s.ctrl('c');
    run(&s, "echo after-x | tr x 6");
    s.wait_text("after-6");
    assert!(t0.elapsed() < Duration::from_secs(10));
    s.ctrl('d');
    s.wait_exit();
}

macro_rules! shell_tests {
    ($shell:ident: $($test:ident),+) => {
        mod $shell {
            $(
                #[test]
                fn $test() {
                    crate::require!(stringify!($shell));
                    super::$test(stringify!($shell));
                }
            )+
        }
    };
}

shell_tests!(bash: runs_commands, line_editing, history, tab_completion, resize, interrupt_and_exit);
shell_tests!(zsh: runs_commands, line_editing, history, tab_completion, resize, interrupt_and_exit);
shell_tests!(fish: runs_commands, line_editing, history, tab_completion, resize, interrupt_and_exit);
shell_tests!(dash: runs_commands, resize, interrupt_and_exit);

#[test]
fn fish_marks_prompts() {
    require!("fish");
    let s = start("fish");
    let v = std::process::Command::new("fish").arg("--version").output().unwrap();
    let ver = String::from_utf8_lossy(&v.stdout);
    let major: u32 = ver
        .split_whitespace()
        .last()
        .and_then(|v| v.split('.').next())
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    if major < 4 {
        eprintln!("skipped: fish {major} does not emit OSC 133");
        return;
    }
    run(&s, "echo marked-output");
    s.wait_text("marked-output");
    s.settle();
    assert!(!s.with(|t| t.command_outputs()).is_empty(), "no OSC 133 marks\n{}", s.dump());
}
