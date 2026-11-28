use std::process::Command;

use aeternus::term::{Term, WIDE_SPACER, WRAPLINE};
use winit::keyboard::NamedKey;

use crate::harness::{have, Session};
use crate::require;

const PROBE: &str = r#"stty raw -echo; printf '\033[H\033[2J%s\033[6n' "$1"; IFS= read -rs -d R p; printf '\033[24;1H<%s>' "${p#*[}"; exec sleep 60"#;

struct Probe {
    s: Session,
    cursor: (usize, usize),
}

fn probe(text: &str) -> Probe {
    let s = Session::spawn(&["bash", "-c", PROBE, "probe", text], 80, 24);
    s.wait("the cursor position report", |t| {
        let r = crate::harness::row_text(t, 23);
        r.starts_with('<') && r.ends_with('>')
    });
    let r = s.row(23);
    let (row, col) = r.trim_matches(['<', '>']).split_once(';').expect("row;col");
    let cursor = (row.parse().unwrap(), col.parse().unwrap());
    Probe { s, cursor }
}

#[derive(Debug, PartialEq)]
enum C<'a> {
    Ch(char),
    Cl(char, &'a str),
    Spacer,
}

fn cells(p: &Probe, row: usize, n: usize) -> Vec<String> {
    p.s.with(|t| {
        t.grid.row(row)[..n]
            .iter()
            .map(|c| {
                if c.flags & WIDE_SPACER != 0 {
                    format!("{:?}", C::Spacer)
                } else if let Some(cl) = t.cluster(c) {
                    format!("{:?}", C::Cl(c.ch, cl))
                } else {
                    format!("{:?}", C::Ch(c.ch))
                }
            })
            .collect()
    })
}

fn expect(v: &[C]) -> Vec<String> {
    v.iter().map(|c| format!("{c:?}")).collect()
}

fn glibc_width(s: &str) -> Option<usize> {
    if !have("wc") {
        return None;
    }
    let out =
        Command::new("sh").args(["-c", "printf '%s' \"$1\" | LC_ALL=C.UTF-8 wc -L", "w", s]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn width_case(text: &str, want_cells: &[C], aeternus: usize, glibc: usize) {
    let p = probe(text);
    assert_eq!(cells(&p, 0, want_cells.len()), expect(want_cells), "{}", p.s.dump());
    assert_eq!(p.cursor, (1, aeternus + 1), "{}", p.s.dump());
    if let Some(g) = glibc_width(text) {
        assert_eq!(g, glibc, "wcwidth changed for {text:?}; update tests/compat/README.md");
    }
}

#[test]
fn simple_emoji_takes_two_columns() {
    require!("bash");
    width_case("😀x", &[C::Ch('😀'), C::Spacer, C::Ch('x')], 3, 3);
}

#[test]
fn zwj_sequence_is_one_wide_cluster() {
    require!("bash");
    width_case(
        "👨\u{200D}👩\u{200D}👧x",
        &[C::Cl('👨', "\u{200D}👩\u{200D}👧"), C::Spacer, C::Ch('x')],
        3,
        7,
    );
}

#[test]
fn skin_tone_modifier_joins_the_emoji() {
    require!("bash");
    width_case("👍🏽x", &[C::Cl('👍', "🏽"), C::Spacer, C::Ch('x')], 3, 5);
}

#[test]
fn flags_are_two_narrow_regional_indicators() {
    require!("bash");
    width_case("🇧🇷x", &[C::Ch('🇧'), C::Ch('🇷'), C::Ch('x')], 3, 3);
}

#[test]
fn variation_selector_16_does_not_widen() {
    require!("bash");
    width_case("❤\u{FE0F}x", &[C::Cl('❤', "\u{FE0F}"), C::Ch('x')], 2, 2);
}

#[test]
fn cjk_takes_two_columns() {
    require!("bash");
    width_case("中文x", &[C::Ch('中'), C::Spacer, C::Ch('文'), C::Spacer, C::Ch('x')], 5, 5);
}

#[test]
fn combining_marks_stay_in_the_base_cell() {
    require!("bash");
    width_case("e\u{301}x", &[C::Cl('e', "\u{301}"), C::Ch('x')], 2, 2);
    width_case("\u{E9}x", &[C::Ch('\u{E9}'), C::Ch('x')], 2, 2);
}

#[test]
fn wide_char_that_does_not_fit_wraps() {
    require!("bash");
    let p = probe("\x1b[1;80H中");
    assert_eq!(p.cursor, (2, 3), "{}", p.s.dump());
    let last = p.s.cell(0, 79);
    assert_eq!(last.ch, ' ');
    assert_ne!(last.flags & WRAPLINE, 0);
    assert_eq!(cells(&p, 1, 3), expect(&[C::Ch('中'), C::Spacer, C::Ch(' ')]));
}

#[test]
fn overwriting_half_a_wide_char_blanks_the_other_half() {
    require!("bash");
    let cases: [(&str, &[C], (usize, usize)); 7] = [
        ("中文\x1b[1;2Hx", &[C::Ch(' '), C::Ch('x'), C::Ch('文'), C::Spacer], (1, 3)),
        ("中文\x1b[1;1Hx", &[C::Ch('x'), C::Ch(' '), C::Ch('文'), C::Spacer], (1, 2)),
        ("中文\x1b[1;2H\x1b[X", &[C::Ch(' '), C::Ch(' '), C::Ch('文'), C::Spacer], (1, 2)),
        ("中文\x1b[1;4H\x1b[K", &[C::Ch('中'), C::Spacer, C::Ch(' '), C::Ch(' ')], (1, 4)),
        ("中文\x1b[1;3H\x1b[1K", &[C::Ch(' '), C::Ch(' '), C::Ch(' '), C::Ch(' ')], (1, 3)),
        ("中文\x1b[1;2H\x1b[P", &[C::Ch(' '), C::Ch('文'), C::Spacer, C::Ch(' ')], (1, 2)),
        ("中文\x1b[1;2H\x1b[@", &[C::Ch(' '), C::Ch(' '), C::Ch(' '), C::Ch('文'), C::Spacer], (1, 2)),
    ];
    for (text, want, cursor) in cases {
        let p = probe(text);
        assert_eq!(cells(&p, 0, want.len()), expect(want), "{text:?}\n{}", p.s.dump());
        assert_eq!(p.cursor, cursor, "{text:?}\n{}", p.s.dump());
    }
}

fn assert_consistent(t: &Term) {
    for r in 0..t.rows() {
        let row = t.grid.row(r);
        for c in 0..row.len() {
            let spacer = row[c].flags & WIDE_SPACER != 0;
            if spacer {
                let prev = c.checked_sub(1).map(|p| &row[p]);
                assert!(
                    prev.is_some_and(|p| p.flags & WIDE_SPACER == 0 && crate::harness::is_wide(p.ch)),
                    "orphan spacer at {r},{c}\n{}",
                    t.dump()
                );
            } else if crate::harness::is_wide(row[c].ch) && c + 1 < row.len() {
                assert_ne!(
                    row[c + 1].flags & WIDE_SPACER,
                    0,
                    "wide char without spacer at {r},{c}\n{}",
                    t.dump()
                );
            }
        }
    }
}

#[test]
fn nvim_edits_wide_and_combined_text_cleanly() {
    require!("nvim", "sh");
    let s = Session::spawn(
        &[
            "sh",
            "-c",
            "while [ ! -e ready ]; do sleep 0.05; done; nvim --clean -n file.txt; echo after-editor; exec sleep 60",
        ],
        80,
        24,
    );
    std::fs::write(s.home.join("file.txt"), "a😀b中c\n日本語 e\u{301} 👍🏽 end\n").unwrap();
    std::fs::write(s.home.join("ready"), "").unwrap();
    s.wait_text("file.txt");
    s.wait_row(1, "end");
    s.settle();
    assert_eq!(s.row(0), "a😀b中c");
    assert_eq!(s.row(1), "日本語 e\u{301} 👍🏽 end");
    s.with(assert_consistent);
    s.type_str("0");
    for col in [1, 3, 4, 6] {
        s.type_str("l");
        s.wait(&format!("the cursor at column {col}"), |t| t.cursor.row == 0 && t.cursor.col == col);
    }
    s.type_str("0lx");
    s.wait_row(0, "ab中c");
    s.settle();
    assert_eq!(s.row(0), "ab中c");
    s.with(assert_consistent);
    s.type_str("j0i🎉");
    s.key(NamedKey::Escape);
    s.wait_row(1, "🎉日本語");
    s.type_str("$");
    s.wait("the cursor on the last letter", |t| t.cursor.row == 1 && t.cursor.col == 16);
    s.type_str("07l");
    s.wait("the cursor on the emoji", |t| t.cursor.row == 1 && t.cursor.col == 11);
    s.type_str("x");
    s.wait_row(1, "🎉日本語 e\u{301}  end");
    s.settle();
    assert_eq!(s.row(1), "🎉日本語 e\u{301}  end");
    s.with(assert_consistent);
    s.key(NamedKey::Escape);
    s.type_str(":wq");
    s.key(NamedKey::Enter);
    s.wait_text("after-editor");
    assert_eq!(std::fs::read_to_string(s.home.join("file.txt")).unwrap(), "ab中c\n🎉日本語 e\u{301}  end\n");
}
