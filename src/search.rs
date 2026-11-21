use std::time::{Duration, Instant};

use crate::term::{SearchMatch, Term};

pub const MAX_MATCHES: usize = 100_000;
pub const REFRESH_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Default)]
pub struct Search {
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub current: Option<usize>,
    stale: bool,
    last_refresh: Option<Instant>,
}

impl Search {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn capped(&self) -> bool {
        self.matches.len() >= MAX_MATCHES
    }
    pub fn current_match(&self) -> Option<SearchMatch> {
        self.matches.get(self.current?).copied()
    }
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }
    pub fn refresh_due(&self) -> Option<Instant> {
        if !self.stale {
            return None;
        }
        Some(self.last_refresh.map_or_else(Instant::now, |t| t + REFRESH_INTERVAL))
    }
    pub fn set_query(&mut self, term: &mut Term, query: String) {
        self.query = query;
        self.run(term);
        self.current = self.nearest_above(term.abs_line(term.rows().saturating_sub(1)));
        self.reveal(term);
    }
    pub fn push_str(&mut self, term: &mut Term, s: &str) {
        let q = format!("{}{s}", self.query);
        self.set_query(term, q);
    }
    pub fn pop_char(&mut self, term: &mut Term) {
        let mut q = self.query.clone();
        if q.pop().is_some() {
            self.set_query(term, q);
        }
    }
    pub fn clear_query(&mut self, term: &mut Term) {
        if !self.query.is_empty() {
            self.set_query(term, String::new());
        }
    }
    pub fn refresh(&mut self, term: &Term, now: Instant) {
        let previous = self.current_match();
        self.run(term);
        self.last_refresh = Some(now);
        self.current = match previous {
            Some(p) => match self.matches.binary_search_by_key(&key(&p), key) {
                Ok(i) => Some(i),
                Err(_) => self.nearest_above(p.start.line),
            },
            None => self.nearest_above(term.abs_line(term.rows().saturating_sub(1))),
        };
    }
    pub fn step(&mut self, term: &mut Term, up: bool) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.current = Some(match self.current {
            Some(i) if up => (i + n - 1) % n,
            Some(i) => (i + 1) % n,
            None if up => n - 1,
            None => 0,
        });
        self.reveal(term);
    }
    fn run(&mut self, term: &Term) {
        self.stale = false;
        self.matches = term.search(&self.query, MAX_MATCHES);
    }
    fn reveal(&self, term: &mut Term) {
        if let Some(m) = self.current_match() {
            term.scroll_to_line(m.start.line);
        }
    }
    fn nearest_above(&self, line: usize) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let after = self.matches.partition_point(|m| m.start.line <= line);
        Some(after.saturating_sub(1))
    }
}

fn key(m: &SearchMatch) -> (usize, usize) {
    (m.start.line, m.start.col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn term_with(lines: usize, rows: usize) -> Term {
        let mut t = Term::new(20, rows);
        let mut p = Parser::new();
        for i in 0..lines {
            p.advance(&mut t, format!("item {i}\r\n").as_bytes());
        }
        t
    }

    #[test]
    fn new_query_selects_match_nearest_the_bottom() {
        let mut t = term_with(30, 5);
        let mut s = Search::new();
        s.set_query(&mut t, "item".into());
        assert_eq!(s.matches.len(), 30);
        assert_eq!(s.current, Some(29));
        assert_eq!(t.display_offset, 0);
    }

    #[test]
    fn stepping_wraps_and_scrolls() {
        let mut t = term_with(30, 5);
        let mut s = Search::new();
        s.set_query(&mut t, "item 1".into());
        let n = s.matches.len();
        assert_eq!(n, 11);
        assert_eq!(s.current, Some(n - 1));
        s.step(&mut t, false);
        assert_eq!(s.current, Some(0));
        let line = s.current_match().unwrap().start.line;
        assert!(t.abs_line(0) <= line && line < t.abs_line(t.rows()));
        s.step(&mut t, true);
        assert_eq!(s.current, Some(n - 1));
    }

    #[test]
    fn editing_the_query() {
        let mut t = term_with(3, 5);
        let mut s = Search::new();
        s.push_str(&mut t, "item 2x");
        assert!(s.matches.is_empty());
        assert_eq!(s.current, None);
        s.pop_char(&mut t);
        assert_eq!(s.query, "item 2");
        assert_eq!(s.matches.len(), 1);
        s.clear_query(&mut t);
        assert!(s.matches.is_empty());
    }

    #[test]
    fn refresh_keeps_the_selected_match() {
        let mut t = term_with(10, 5);
        let mut s = Search::new();
        s.set_query(&mut t, "item".into());
        s.step(&mut t, true);
        s.step(&mut t, true);
        let before = s.current_match().unwrap();
        Parser::new().advance(&mut t, b"item new\r\n");
        s.mark_stale();
        assert!(s.refresh_due().is_some());
        s.refresh(&t, Instant::now());
        assert_eq!(s.current_match(), Some(before));
        assert_eq!(s.matches.len(), 11);
        assert!(s.refresh_due().is_none());
    }
}
