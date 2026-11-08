use std::hint::black_box;
use std::time::Instant;

use crate::parser::Parser;
use crate::term::Term;

fn plain(size: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(size + 128);
    let mut n = 0u64;
    while v.len() < size {
        v.extend_from_slice(
            format!("linha {n:08} de texto simples com algumas palavras para encher a tela\r\n")
                .as_bytes(),
        );
        n += 1;
    }
    v
}

fn colored(size: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(size + 256);
    let mut n = 0u32;
    while v.len() < size {
        for w in ["src", "target", "Cargo.toml", "README.md", "main.rs"] {
            let (r, g, b) = ((n * 7) % 256, (n * 13) % 256, (n * 29) % 256);
            v.extend_from_slice(format!("\x1b[38;2;{r};{g};{b}m{w}\x1b[0m  ").as_bytes());
            n += 1;
        }
        v.extend_from_slice(b"\r\n");
    }
    v
}

fn tui(size: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(size + 4096);
    let mut frame = 0u32;
    while v.len() < size {
        v.extend_from_slice(b"\x1b[H");
        for row in 1..=40 {
            v.extend_from_slice(
                format!(
                    "\x1b[{row};1H\x1b[K\x1b[7m {frame:05} \x1b[0m item {row:02} da lista \x1b[1;32mok\x1b[0m"
                )
                .as_bytes(),
            );
        }
        frame += 1;
    }
    v
}

fn run(name: &str, data: &[u8]) {
    let mut best = f64::MAX;
    for _ in 0..3 {
        let mut term = Term::new(120, 40);
        let mut parser = Parser::new();
        let t0 = Instant::now();
        for chunk in data.chunks(65536) {
            parser.advance(&mut term, chunk);
            term.replies.clear();
        }
        black_box(&term);
        best = best.min(t0.elapsed().as_secs_f64());
    }
    println!(
        "{name:<8} {:>8.1} MB/s  ({:.0} ms)",
        data.len() as f64 / 1e6 / best,
        best * 1000.0
    );
}

pub fn run_all() {
    let size = 64 << 20;
    for (name, data) in [("plain", plain(size)), ("colored", colored(size)), ("tui", tui(size))] {
        run(name, &data);
    }
}
