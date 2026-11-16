const ABOUT_BLOB: [&str; 11] = [
    "@@@@@@@@@@@@@@@@@@@@",
    "#@@@@@@@@@@@@@@@@@@#",
    "@@@@@@@@@@@@@@@@@@@@",
    "@@@@..@@@@@@@@@@@@@@",
    "@@@@@@...@@@@@@@@@@@",
    "@@@@@@@@...@@@@@@@@@",
    "@@@@@@...@@@@@@@@@@@",
    "@@@@..@@@@@#====@@@@",
    "@@@@@@@@@@@@@@@@@@@@",
    "#@@@@@@@@@@@@@@@@@@#",
    "@@@@@@@@@@@@@@@@@@@@",
];
const ABOUT_LOGO: [&str; 6] = [
    r"     _        _                            ",
    r"    / \   ___| |_ ___ _ __ _ __  _   _ ___ ",
    r"   / _ \ / _ \ __/ _ \ '__| '_ \| | | / __|",
    r"  / ___ \  __/ ||  __/ |  | | | | |_| \__ \",
    r" /_/   \_\___\__\ ___|_|  |_| |_|\__,_|___/",
    "",
];
const INK: [u8; 3] = [2, 4, 6];
const SENTINEL: [u8; 3] = [1, 2, 3];

fn sgr_run(line: &mut String, text: &str, col: [u8; 3]) {
    if text.is_empty() {
        return;
    }
    line.push_str(&format!("\x1b[38;2;{};{};{}m{}", col[0], col[1], col[2], text));
}

pub fn about_art() -> String {
    let blob_w = ABOUT_BLOB.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let logo_w = ABOUT_LOGO.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let top = (ABOUT_BLOB.len() - ABOUT_LOGO.len()) / 2;
    let ver = format!("v{}", env!("CARGO_PKG_VERSION"));
    let mut out = String::from("\r\n");
    for (r, blob_line) in ABOUT_BLOB.iter().enumerate() {
        let blob = format!("{blob_line:<blob_w$}");
        let logo = match r.checked_sub(top).and_then(|i| ABOUT_LOGO.get(i)) {
            Some(l) => format!("{l:<logo_w$}"),
            None if r == top + ABOUT_LOGO.len() => format!("{ver:^logo_w$}"),
            None => " ".repeat(logo_w),
        };
        let mut line = String::new();
        for (text, colored) in [(&blob, true), (&logo, false)] {
            let mut run = String::new();
            let mut cur: Option<[u8; 3]> = None;
            for c in text.chars() {
                let col = if colored && (c == '@' || c == '#') { SENTINEL } else { INK };
                if cur == Some(col) {
                    run.push(c);
                } else {
                    sgr_run(&mut line, &run, cur.unwrap_or(INK));
                    run.clear();
                    run.push(c);
                    cur = Some(col);
                }
            }
            sgr_run(&mut line, &run, cur.unwrap_or(INK));
        }
        out.push_str(line.trim_end());
        out.push_str("\x1b[0m\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn art_contains_version_and_resets_colors() {
        let art = super::about_art();
        assert!(art.contains(env!("CARGO_PKG_VERSION")));
        assert!(art.ends_with("\x1b[0m\r\n"));
    }
}
