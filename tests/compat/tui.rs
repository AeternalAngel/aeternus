use aeternus::cell::flags;
use winit::keyboard::NamedKey;

use crate::harness::Session;
use crate::require;

#[test]
fn less_pages_searches_and_quits() {
    require!("less", "sh", "seq");
    let s = Session::spawn(
        &["sh", "-c", "seq -f 'line %g' 1 200 > lines.txt; less lines.txt; echo after-less; exec sleep 60"],
        80,
        24,
    );
    s.wait("less to take the alternate screen", |t| t.is_alt_screen());
    s.wait_line("line 23");
    s.type_str(" ");
    s.wait_line("line 46");
    s.type_str("/line 150");
    s.key(NamedKey::Enter);
    s.wait_line("line 150");
    let (r, c) = s.find("line 150").unwrap();
    assert_ne!(s.cell(r, c).flags & flags::INVERSE, 0, "{}", s.dump());
    s.type_str("G");
    s.wait_text("(END)");
    s.key(NamedKey::Home);
    s.wait_line("line 1");
    s.type_str("q");
    s.wait_text("after-less");
    assert!(!s.with(|t| t.is_alt_screen()));
}

#[test]
fn htop_draws_and_quits_with_f10() {
    require!("htop");
    let s = Session::spawn(&["htop"], 100, 30);
    s.wait_text("Tasks:");
    s.wait("the function key bar", |t| crate::harness::row_text(t, 29).contains("F10Quit"));
    s.key(NamedKey::F10);
    s.wait_exit();
}
