use crate::harness::Session;
use crate::require;

fn start() -> Session {
    let s = Session::spawn(&["vttest", "24x80.80"], 80, 24);
    s.wait_text("Enter choice number");
    s
}

fn choose(s: &Session, n: &str) {
    s.send(format!("{n}\r"));
}

fn next(s: &Session, marker: &str) {
    s.send("\r");
    s.wait_text(marker);
    s.settle();
}

fn check_border_and_frame(s: &Session) {
    let rows = s.screen();
    let full = |c: char, n: usize| std::iter::repeat_n(c, n).collect::<String>();
    assert_eq!(rows[0], full('*', 80), "{}", s.dump());
    assert_eq!(rows[23], full('*', 80), "{}", s.dump());
    s.with(|t| {
        for r in 1..23 {
            let row = t.grid.row(r);
            assert_eq!((row[0].ch, row[79].ch), ('*', '*'), "row {r}\n{}", t.dump());
            assert_eq!((row[1].ch, row[78].ch), ('+', '+'), "row {r}\n{}", t.dump());
        }
        for r in [1, 22] {
            assert!(t.grid.row(r)[1..79].iter().all(|c| c.ch == '+'), "row {r}\n{}", t.dump());
        }
        for r in [8, 15] {
            assert!(t.grid.row(r)[10..70].iter().all(|c| c.ch == 'E'), "row {r}\n{}", t.dump());
        }
        for r in 9..15 {
            let row = t.grid.row(r);
            assert_eq!(
                (row[10].ch, row[11].ch, row[68].ch, row[69].ch),
                ('E', ' ', ' ', 'E'),
                "row {r}\n{}",
                t.dump()
            );
        }
    });
}

fn check_autowrap(s: &Session) {
    s.with(|t| {
        let mut letters = Vec::new();
        for r in 2..21 {
            let row = t.grid.row(r);
            let (l, rt) = (row[0].ch, row[79].ch);
            if l.is_ascii_uppercase() {
                assert_eq!(rt, l.to_ascii_lowercase(), "row {r}\n{}", t.dump());
                letters.push(l);
            }
        }
        assert!(letters.len() >= 15, "{}", t.dump());
        assert!(letters.windows(2).all(|w| w[1] as u8 == w[0] as u8 + 1), "{letters:?}\n{}", t.dump());
        assert_eq!(letters.last(), Some(&'Z'), "{}", t.dump());
    });
}

#[test]
fn cursor_movements() {
    require!("vttest");
    let s = start();
    choose(&s, "1");
    s.wait_text("The screen should be cleared");
    s.settle();
    check_border_and_frame(&s);
    next(&s, "The screen should be cleared");
    check_border_and_frame(&s);
    next(&s, "Test of autowrap");
    check_autowrap(&s);
    next(&s, "Test of autowrap");
    check_autowrap(&s);
    next(&s, "Below should be four identical lines");
    let same = s.screen().iter().filter(|l| *l == "A B C D E F G H I").count();
    assert_eq!(same, 4, "{}", s.dump());
    next(&s, "Test of leading zeros");
    assert!(s.row(3).starts_with("This is a correct sentence"), "{}", s.dump());
    next(&s, "Enter choice number");
    choose(&s, "0");
    s.wait_exit();
}

#[test]
fn terminal_reports() {
    require!("vttest");
    let s = start();
    choose(&s, "6");
    s.wait_text("Terminal Reports/Responses");
    choose(&s, "3");
    s.wait_text("report cursor position");
    s.wait("both cursor position reports", |t| {
        crate::harness::screen_of(t)
            .iter()
            .filter(|l| l.contains(" -- OK") || l.contains("Unknown") || l.contains("Ignores"))
            .count()
            >= 2
    });
    s.settle();
    let screen = s.screen().join("\n");
    assert!(screen.contains("-- means \"TERMINAL OK\""), "{}", s.dump());
    assert_eq!(screen.matches(" -- OK").count(), 2, "{}", s.dump());
    next(&s, "Terminal Reports/Responses");
    choose(&s, "4");
    s.wait_text("Report is");
    s.settle();
    assert!(!s.screen().join("\n").contains("Unknown response"), "{}", s.dump());
    next(&s, "Terminal Reports/Responses");
    choose(&s, "0");
    s.wait_text("Enter choice number");
    choose(&s, "0");
    s.wait_exit();
}
