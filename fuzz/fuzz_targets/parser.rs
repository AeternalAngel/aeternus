#![no_main]

use aeternus::parser::Parser;
use aeternus::term::Term;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let cols = 1 + data[0] as usize % 120;
    let rows = 1 + data[1] as usize % 50;
    let mut term = Term::new(cols, rows);
    let mut parser = Parser::new();
    for chunk in data[2..].chunks(64) {
        parser.advance(&mut term, chunk);
        term.replies.clear();
        if chunk.first() == Some(&0xff) {
            term.resize(1 + chunk.len() % 80, 1 + chunk.len() % 30);
        }
    }
    let _ = term.selection_text();
});
