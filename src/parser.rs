pub trait Perform {
    fn print(&mut self, c: char);
    fn execute(&mut self, byte: u8);
    fn csi(&mut self, params: &[u16], subs: &[bool], inter: &[u8], private: Option<u8>, action: u8);
    fn esc(&mut self, inter: &[u8], byte: u8);
    fn osc(&mut self, data: &[u8]);
    fn dcs(&mut self, _params: &[u16], _inter: &[u8], _private: Option<u8>, _action: u8, _data: &[u8]) {}
    fn apc(&mut self, _data: &[u8]) {}
    fn print_ascii(&mut self, s: &[u8]) {
        for &b in s {
            self.print(b as char);
        }
    }
}
const OSC_MAX: usize = 4096;
const OSC52_MAX: usize = 4 * ((1 << 20) + 2) / 3 + 64;
pub const DCS_MAX: usize = 4096;
pub const APC_MAX: usize = 8192;
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    Csi,
    Osc,
    DcsEntry,
    DcsPass,
    Apc,
    StrIgnore,
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
    dcs_action: u8,
    data: Vec<u8>,
    data_overflow: bool,
    utf8: [u8; 4],
    utf8_len: usize,
    utf8_need: usize,
}
impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
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
            dcs_action: 0,
            data: Vec::new(),
            data_overflow: false,
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
                State::DcsEntry => self.dcs_entry(b),
                State::DcsPass | State::Apc => self.string_data(p, b),
                State::StrIgnore => self.str_ignore(b),
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
                self.reset_params();
                self.state = State::Csi;
            }
            b'P' if self.inter.is_empty() => {
                self.reset_params();
                self.state = State::DcsEntry;
            }
            b'_' if self.inter.is_empty() => {
                self.data.clear();
                self.data_overflow = false;
                self.state = State::Apc;
            }
            b']' if self.inter.is_empty() => {
                self.osc.clear();
                self.osc_overflow = false;
                self.state = State::Osc;
            }
            b'X' | b'^' if self.inter.is_empty() => self.state = State::Ignore,
            0x30..=0x7E => {
                p.esc(&self.inter, b);
                self.state = State::Ground;
            }
            _ => self.state = State::Ground,
        }
    }
    fn reset_params(&mut self) {
        self.params.clear();
        self.subs.clear();
        self.pending_colon = false;
        self.cur = 0;
        self.started = false;
        self.inter.clear();
        self.private = None;
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
    fn dcs_entry(&mut self, b: u8) {
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
                } else {
                    self.state = State::StrIgnore;
                }
            }
            0x20..=0x2F => {
                if self.inter.len() < 4 {
                    self.inter.push(b);
                } else {
                    self.state = State::StrIgnore;
                }
            }
            0x40..=0x7E => {
                if self.started {
                    self.push_param();
                }
                self.dcs_action = b;
                self.data.clear();
                self.data_overflow = false;
                self.state = State::DcsPass;
            }
            0x1B => self.enter_escape(),
            0x18 | 0x1A => self.state = State::Ground,
            _ => {}
        }
    }
    fn string_data<P: Perform>(&mut self, p: &mut P, b: u8) {
        match b {
            0x1B => {
                if !self.data_overflow {
                    if self.state == State::Apc {
                        p.apc(&self.data);
                    } else {
                        p.dcs(&self.params, &self.inter, self.private, self.dcs_action, &self.data);
                    }
                }
                self.enter_escape();
            }
            0x18 | 0x1A => self.state = State::Ground,
            _ => {
                let max = if self.state == State::Apc { APC_MAX } else { DCS_MAX };
                if self.data.len() < max {
                    self.data.push(b);
                } else {
                    self.data_overflow = true;
                }
            }
        }
    }
    fn str_ignore(&mut self, b: u8) {
        match b {
            0x18 | 0x1A => self.state = State::Ground,
            0x1B => self.enter_escape(),
            _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    type Dcs = (Vec<u16>, Vec<u8>, Option<u8>, u8, Vec<u8>);
    #[derive(Default)]
    struct Rec {
        text: String,
        dcs: Vec<Dcs>,
        apc: Vec<Vec<u8>>,
        esc: Vec<u8>,
    }
    impl Perform for Rec {
        fn print(&mut self, c: char) {
            self.text.push(c);
        }
        fn execute(&mut self, _byte: u8) {}
        fn csi(&mut self, _params: &[u16], _subs: &[bool], _inter: &[u8], _private: Option<u8>, _action: u8) {
        }
        fn esc(&mut self, _inter: &[u8], byte: u8) {
            self.esc.push(byte);
        }
        fn osc(&mut self, _data: &[u8]) {}
        fn dcs(&mut self, params: &[u16], inter: &[u8], private: Option<u8>, action: u8, data: &[u8]) {
            self.dcs.push((params.to_vec(), inter.to_vec(), private, action, data.to_vec()));
        }
        fn apc(&mut self, data: &[u8]) {
            self.apc.push(data.to_vec());
        }
    }
    fn run(input: &[u8]) -> Rec {
        let mut r = Rec::default();
        Parser::new().advance(&mut r, input);
        r
    }

    #[test]
    fn dcs_with_7bit_st() {
        let r = run(b"\x1bP+q544e\x1b\\ok");
        assert_eq!(r.dcs, vec![(vec![], b"+".to_vec(), None, b'q', b"544e".to_vec())]);
        assert_eq!(r.text, "ok");
    }
    #[test]
    fn bare_9c_is_payload_not_st() {
        let r = run(b"\x1bP$qm\x9cok\x1b\\");
        assert_eq!(r.dcs, vec![(vec![], b"$".to_vec(), None, b'q', b"m\x9cok".to_vec())]);
        assert!(r.text.is_empty());
        assert_eq!(r.esc, b"\\");
    }
    #[test]
    fn dcs_collects_params_private_and_final() {
        let r = run(b"\x1bP0;1;8q#0;2;0;0;0\x1b\\");
        assert_eq!(r.dcs, vec![(vec![0, 1, 8], vec![], None, b'q', b"#0;2;0;0;0".to_vec())]);
        let r = run(b"\x1bP>|x\x1b\\");
        assert_eq!(r.dcs[0].2, Some(b'>'));
        assert_eq!(r.dcs[0].3, b'|');
    }
    #[test]
    fn dcs_split_across_reads() {
        let mut r = Rec::default();
        let mut p = Parser::new();
        for chunk in [&b"\x1bP"[..], b"+", b"q54", b"4e\x1b", b"\\ok"] {
            p.advance(&mut r, chunk);
        }
        assert_eq!(r.dcs.len(), 1);
        assert_eq!(r.dcs[0].4, b"544e");
        assert_eq!(r.text, "ok");
    }
    #[test]
    fn oversized_dcs_is_dropped() {
        let mut input = b"\x1bP+q".to_vec();
        input.extend(std::iter::repeat_n(b'a', DCS_MAX + 10));
        input.extend(b"\x1b\\ok");
        let r = run(&input);
        assert!(r.dcs.is_empty());
        assert_eq!(r.text, "ok");
        let mut input = b"\x1bP+q".to_vec();
        input.extend(std::iter::repeat_n(b'a', DCS_MAX));
        input.extend(b"\x1b\\");
        assert_eq!(run(&input).dcs[0].4.len(), DCS_MAX);
    }
    #[test]
    fn cancelled_dcs_is_not_dispatched() {
        let r = run(b"\x1bP+q544e\x18ok");
        assert!(r.dcs.is_empty());
        assert_eq!(r.text, "ok");
        let r = run(b"\x1bP+q54\x1b[mok");
        assert_eq!(r.dcs.len(), 1);
        assert_eq!(r.text, "ok");
    }
    #[test]
    fn malformed_dcs_header_is_ignored() {
        let r = run(b"\x1bP1>q\x07data\x1b\\ok");
        assert!(r.dcs.is_empty());
        assert_eq!(r.text, "ok");
    }
    #[test]
    fn apc_is_collected_and_bounded() {
        let r = run(b"\x1b_Gf=24,a=T;AAAA\x1b\\ok");
        assert_eq!(r.apc, vec![b"Gf=24,a=T;AAAA".to_vec()]);
        assert_eq!(r.text, "ok");
        let mut input = b"\x1b_G".to_vec();
        input.extend(std::iter::repeat_n(b'A', APC_MAX + 1));
        input.extend(b"\x1b\\ok");
        let r = run(&input);
        assert!(r.apc.is_empty());
        assert_eq!(r.text, "ok");
    }
    #[test]
    fn utf8_in_dcs_and_apc_arrives_whole() {
        let r = run("\x1bP+qaⓜb\x1b\\\x1b_Gaⓜb\x1b\\ok".as_bytes());
        assert_eq!(r.dcs.len(), 1);
        assert_eq!(r.dcs[0].4, "aⓜb".as_bytes());
        assert_eq!(r.apc, vec!["Gaⓜb".as_bytes().to_vec()]);
        assert_eq!(r.text, "ok");
    }
    #[test]
    fn sos_and_pm_are_ignored() {
        let r = run(b"\x1bXhid\x9cden\x1b\\\x1b^hidden\x1b\\ok");
        assert!(r.dcs.is_empty() && r.apc.is_empty());
        assert_eq!(r.text, "ok");
    }
}
