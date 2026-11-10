pub trait Perform {
    fn print(&mut self, c: char);
    fn execute(&mut self, byte: u8);
    fn csi(&mut self, params: &[u16], subs: &[bool], inter: &[u8], private: Option<u8>, action: u8);
    fn esc(&mut self, inter: &[u8], byte: u8);
    fn osc(&mut self, data: &[u8]);
    /// Trecho contínuo de ASCII imprimível (0x20..=0x7E). Implementações
    /// podem sobrescrever para escrever em lote.
    fn print_ascii(&mut self, s: &[u8]) {
        for &b in s {
            self.print(b as char);
        }
    }
}

const OSC_MAX: usize = 4096;
const OSC52_MAX: usize = 4 * ((1 << 20) + 2) / 3 + 64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    Csi,
    Osc,
    Ignore,
}

pub struct Parser {
    state: State,
    params: Vec<u16>,
    subs: Vec<bool>,
    pending_colon: bool,
    cur: u32,
    started: bool,
    inter: Vec<u8>,
    private: Option<u8>,
    osc: Vec<u8>,
    osc_overflow: bool,
    utf8: [u8; 4],
    utf8_len: usize,
    utf8_need: usize,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            params: Vec::with_capacity(32),
            subs: Vec::with_capacity(32),
            pending_colon: false,
            cur: 0,
            started: false,
            inter: Vec::with_capacity(4),
            private: None,
            osc: Vec::with_capacity(256),
            osc_overflow: false,
            utf8: [0; 4],
            utf8_len: 0,
            utf8_need: 0,
        }
    }

    pub fn advance<P: Perform>(&mut self, p: &mut P, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            if self.state == State::Ground && self.utf8_need == 0 {
                let start = i;
                while i < bytes.len() && (0x20..=0x7E).contains(&bytes[i]) {
                    i += 1;
                }
                if i > start {
                    p.print_ascii(&bytes[start..i]);
                    continue;
                }
            }
            let b = bytes[i];
            i += 1;
            match self.state {
                State::Ground => self.ground(p, b),
                State::Escape => self.escape(p, b),
                State::Csi => self.csi(p, b),
                State::Osc => self.osc(p, b),
                State::Ignore => self.ignore(b),
            }
        }
    }

    fn enter_escape(&mut self) {
        self.state = State::Escape;
        self.inter.clear();
    }

    fn start_utf8(&mut self, b: u8, need: usize) {
        self.utf8[0] = b;
        self.utf8_len = 1;
        self.utf8_need = need;
    }

    fn ground<P: Perform>(&mut self, p: &mut P, b: u8) {
        if self.utf8_need > 0 {
            if b & 0xC0 == 0x80 {
                self.utf8[self.utf8_len] = b;
                self.utf8_len += 1;
                if self.utf8_len == self.utf8_need {
                    let c = std::str::from_utf8(&self.utf8[..self.utf8_len])
                        .ok()
                        .and_then(|s| s.chars().next())
                        .unwrap_or('\u{FFFD}');
                    p.print(c);
                    self.utf8_need = 0;
                    self.utf8_len = 0;
                }
                return;
            }
            p.print('\u{FFFD}');
            self.utf8_need = 0;
            self.utf8_len = 0;
        }
        match b {
            0x1B => self.enter_escape(),
            0x20..=0x7E => p.print(b as char),
            0x00..=0x1F | 0x7F => p.execute(b),
            0xC2..=0xDF => self.start_utf8(b, 2),
            0xE0..=0xEF => self.start_utf8(b, 3),
            0xF0..=0xF4 => self.start_utf8(b, 4),
            _ => p.print('\u{FFFD}'),
        }
    }

    fn escape<P: Perform>(&mut self, p: &mut P, b: u8) {
        match b {
            0x1B => self.enter_escape(),
            0x18 | 0x1A => self.state = State::Ground,
            0x00..=0x1F => p.execute(b),
            0x20..=0x2F => self.inter.push(b),
            b'[' if self.inter.is_empty() => {
                self.params.clear();
                self.subs.clear();
                self.pending_colon = false;
                self.cur = 0;
                self.started = false;
                self.inter.clear();
                self.private = None;
                self.state = State::Csi;
            }
            b']' if self.inter.is_empty() => {
                self.osc.clear();
                self.osc_overflow = false;
                self.state = State::Osc;
            }
            b'P' | b'X' | b'^' | b'_' if self.inter.is_empty() => self.state = State::Ignore,
            0x30..=0x7E => {
                p.esc(&self.inter, b);
                self.state = State::Ground;
            }
            _ => self.state = State::Ground,
        }
    }

    fn push_param(&mut self) {
        if self.params.len() < 32 {
            self.params.push(self.cur as u16);
            self.subs.push(self.pending_colon);
        }
        self.cur = 0;
    }

    fn csi<P: Perform>(&mut self, p: &mut P, b: u8) {
        match b {
            b'0'..=b'9' => {
                self.started = true;
                self.cur = (self.cur * 10 + (b - b'0') as u32).min(65535);
            }
            b';' | b':' => {
                self.push_param();
                self.pending_colon = b == b':';
                self.started = true;
            }
            0x3C..=0x3F => {
                if !self.started && self.params.is_empty() && self.inter.is_empty() {
                    self.private = Some(b);
                }
            }
            0x20..=0x2F => {
                if self.inter.len() < 4 {
                    self.inter.push(b);
                }
            }
            0x40..=0x7E => {
                if self.started {
                    self.push_param();
                }
                p.csi(&self.params, &self.subs, &self.inter, self.private, b);
                self.state = State::Ground;
            }
            0x1B => self.enter_escape(),
            0x18 | 0x1A => self.state = State::Ground,
            0x00..=0x1F => p.execute(b),
            _ => {}
        }
    }

    fn osc<P: Perform>(&mut self, p: &mut P, b: u8) {
        match b {
            0x07 => {
                if !self.osc_overflow {
                    p.osc(&self.osc);
                }
                self.state = State::Ground;
            }
            0x1B => {
                if !self.osc_overflow {
                    p.osc(&self.osc);
                }
                self.enter_escape();
            }
            0x18 | 0x1A => self.state = State::Ground,
            _ => {
                let is52 = self.osc.starts_with(b"52;");
                let max = if is52 { OSC52_MAX } else { OSC_MAX };
                if self.osc.len() < max {
                    self.osc.push(b);
                } else if is52 {
                    self.osc_overflow = true;
                }
            }
        }
    }

    fn ignore(&mut self, b: u8) {
        match b {
            0x07 | 0x18 | 0x1A => self.state = State::Ground,
            0x1B => self.enter_escape(),
            _ => {}
        }
    }
}
