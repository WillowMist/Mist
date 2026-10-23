//! Telnet protocol state machine.
//!
//! Implements the options Pueblo/UE negotiates, drawn from
//! `modules/client/msw/ChWorld/ChWConn.cpp:97-185`:
//!
//! | Option | Constant | Pueblo behaviour | Rust default |
//! |--------|----------|-------------------|--------------|
//! | ECHO | 1 | DO on WILL | DO on WILL |
//! | SGA | 3 | WILL | WILL |
//! | TTYPE | 24 | sends `PUEBLO` / `PUEBLO-MXP` | same |
//! | EOR | 25 | DO | DO |
//! | NAWS | 31 | DONT (refused) | DONT (matches Pueblo) |
//! | MCCP v1 | 85 | DO | DO on WILL, v1 still honoured |
//! | MCCP v2 | 86 | DO | DO on WILL |
//! | MSP | 90 | not negotiated (server-driven) | not negotiated |
//! | MXP | 91 | not offered | not offered (matches Pueblo) |
//!
//! See `SPEC.md` §1 for the on-wire byte layout.

use flate2::Decompress;
use std::collections::VecDeque;

/// Telnet command bytes (RFC 854).
pub mod cmd {
    pub const SE: u8 = 240;
    pub const NOP: u8 = 241;
    pub const DATA_MARK: u8 = 242;
    pub const BRK: u8 = 243;
    pub const IP: u8 = 244;
    pub const AO: u8 = 245;
    pub const AYT: u8 = 246;
    pub const EC: u8 = 247;
    pub const EL: u8 = 248;
    pub const GA: u8 = 249;
    pub const SB: u8 = 250;
    pub const WILL: u8 = 251;
    pub const WONT: u8 = 252;
    pub const DO: u8 = 253;
    pub const DONT: u8 = 254;
    pub const IAC: u8 = 255;
}

/// Subnegotiation discriminators. TTYPE is the only option where these
/// are load-bearing: `SEND` means the server is asking for our terminal
/// type, `IS` means the server is declaring its own.
pub const IS: u8 = 0;
pub const SEND: u8 = 1;

/// Telnet option numbers.
pub mod opt {
    pub const ECHO: u8 = 1;
    pub const SGA: u8 = 3;
    pub const TERMINAL_TYPE: u8 = 24;
    pub const EOR: u8 = 25;
    pub const NAWS: u8 = 31;
    pub const COMPRESS: u8 = 85;
    pub const COMPRESS2: u8 = 86;
    pub const MSP: u8 = 90;
    pub const MXP: u8 = 91;
}

/// Events emitted by the telnet state machine back to the connection driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelnetEvent {
    /// Plain bytes that belong to the application's data stream.
    Data(Vec<u8>),
    /// Server requested echo mode change.
    Echo(bool),
    /// End of Record — treat the buffered line as a complete prompt.
    EndOfRecord,
    /// Sub-option payload for an option the application cares about.
    Subnegotiate { option: u8, params: Vec<u8> },
    /// MCCP v1/v2 has started; subsequent reads are zlib-compressed.
    CompressionBegin,
    /// Compression stream ended.
    CompressionEnd,
}

/// Negotiator that holds server-side and client-side option state.
#[derive(Debug, Clone)]
pub struct Negotiator {
    pub server_echo: bool,
    pub client_will: u128, // bitset of options we WILL
    pub server_do: u128,   // bitset of options server said DO to
    pub mccp_active: bool,
    /// What we report in reply to `IAC SB TTYPE SEND`.
    ///
    /// RFC 1091 caps this at 24 printable US-ASCII characters. The value is
    /// configurable because servers gate on it: PennMUSH and TinyMUSH read
    /// `terminfo()` to decide whether to grant ANSI to the session, so a
    /// server may expect a specific string. See the `TERMINAL_TYPE`
    /// constant for the default, and [`TelnetStream::set_terminal_type`] to
    /// override it per-world.
    pub terminal_type: String,
}

/// Default terminal type we report: a real VT100/xterm name so servers
/// running a terminfo lookup recognise us, plus the ANSI marker MUSH
/// servers look for. Fits inside RFC 1091's 24-character limit.
pub const DEFAULT_TERMINAL_TYPE: &str = "xterm-256color";

impl Default for Negotiator {
    fn default() -> Self {
        // We agree to: ECHO, SGA, TERMINAL_TYPE, EOR, COMPRESS2.
        // We explicitly do NOT offer NAWS or MXP by default; the original
        // Pueblo client refused both with DONT, and we mirror that.
        // Override by changing `client_will` if you need them.
        // Telnet options can be up to 255, so we use a u128 bitfield.
        let mut will = 0u128;
        for b in [opt::ECHO, opt::SGA, opt::TERMINAL_TYPE, opt::EOR, opt::COMPRESS2] {
            will |= 1u128 << b;
        }
        Self {
            server_echo: false,
            client_will: will,
            server_do: 0,
            mccp_active: false,
            terminal_type: DEFAULT_TERMINAL_TYPE.to_string(),
        }
    }
}

/// Owned handle around a stream of bytes. The connection driver feeds raw socket
/// bytes in via [`TelnetStream::feed`] and pulls completed events out via
/// [`TelnetStream::drain`].
pub struct TelnetStream {
    state: ParserState,
    negotiated: Negotiator,
    pending_out: VecDeque<Vec<u8>>, // bytes to send back to the server
    events: VecDeque<TelnetEvent>,
    mccp: Option<MccpDecompressor>,
    sb_buf: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
enum ParserState {
    Normal,
    Iac,
    Cmd(u8),
    /// `None` = next byte is the option; `Some(opt)` = we're inside the payload.
    Sb(Option<u8>),
    /// Inside an SB payload and we just saw an IAC; expect SE next.
    SbIac(u8),
}

struct MccpDecompressor {
    z: Decompress,
}

impl MccpDecompressor {
    fn new_v2() -> Self {
        Self { z: Decompress::new(true) }
    }
    fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), crate::error::PuebloError> {
        // flate2's low-level `Decompress` doesn't implement `Read`. We use
        // `Decompress::decompress` with a streaming input/output pair, draining
        // the decoder until it would block on more input. The scaffold is
        // intentionally minimal — a real MCCP implementation also needs to
        // detect Z_STREAM_END.
        let mut total_in = self.z.total_in();
        let mut total_out = self.z.total_out();
        loop {
            let mut scratch_in = input.get(total_in as usize..).unwrap_or(&[]);
            let mut scratch_out = vec![0u8; 32 * 1024];
            let before_in = self.z.total_in();
            let before_out = self.z.total_out();
            let res = self.z.decompress(
                scratch_in,
                &mut scratch_out,
                flate2::FlushDecompress::None,
            );
            match res {
                Ok(flate2::Status::Ok) | Ok(flate2::Status::StreamEnd) => {
                    let produced = (self.z.total_out() - before_out) as usize;
                    out.extend_from_slice(&scratch_out[..produced]);
                    total_in = self.z.total_in();
                    total_out = self.z.total_out();
                    if matches!(res, Ok(flate2::Status::StreamEnd)) || produced == 0 {
                        break;
                    }
                }
                Ok(flate2::Status::BufError) => break, // need more bytes
                Err(e) => return Err(crate::error::PuebloError::Mccp(e.to_string())),
            }
            // Update the local input cursor so we don't double-feed bytes.
            scratch_in = &[];
        }
        Ok(())
    }
}

impl TelnetStream {
    pub fn new() -> Self {
        Self {
            state: ParserState::Normal,
            negotiated: Negotiator::default(),
            pending_out: VecDeque::new(),
            events: VecDeque::new(),
            mccp: None,
            sb_buf: Vec::new(),
        }
    }

    pub fn negotiated(&self) -> &Negotiator {
        &self.negotiated
    }

    /// Override the terminal type we report in reply to
    /// `IAC SB TTYPE SEND`. Values are truncated to 24 printable
    /// US-ASCII characters per RFC 1091.
    pub fn set_terminal_type(&mut self, tt: &str) {
        let cleaned: String = tt
            .chars()
            .filter(|c| (' '..='~').contains(c))
            .take(24)
            .collect();
        self.negotiated.terminal_type = cleaned;
    }

    /// Build the `IAC SB TTYPE IS <name> IAC SE` reply. Split out so the
    /// test suite can assert the exact bytes without a socket.
    fn terminal_type_reply(&self) -> Vec<u8> {
        let mut out = vec![cmd::IAC, cmd::SB, opt::TERMINAL_TYPE, IS];
        out.extend(self.negotiated.terminal_type.bytes());
        out.extend_from_slice(&[cmd::IAC, cmd::SE]);
        out
    }

    /// Bytes the caller should push back onto the socket.
    pub fn drain_outgoing(&mut self) -> Vec<Vec<u8>> {
        self.pending_out.drain(..).collect()
    }

    /// Events ready for the application layer.
    pub fn drain_events(&mut self) -> Vec<TelnetEvent> {
        self.events.drain(..).collect()
    }

    /// Feed raw socket bytes into the parser.
    pub fn feed(&mut self, input: &[u8]) {
        let mut buf: Vec<u8> = Vec::new();
        if let Some(z) = self.mccp.as_mut() {
            // Compressed path: decompress into buf first, then parse the
            // expanded bytes as if they were the raw stream.
            if let Err(e) = z.feed(input, &mut buf) {
                self.events.push_back(TelnetEvent::CompressionEnd);
                self.events.push_back(TelnetEvent::Data(format!("[MCCP error: {e}]").into_bytes()));
                self.mccp = None;
                return;
            }
        } else {
            buf.extend_from_slice(input);
        }

        for &b in buf.iter() {
            self.consume_byte(b);
        }
    }

    fn consume_byte(&mut self, b: u8) {
        match self.state {
            ParserState::Normal => {
                if b == cmd::IAC {
                    self.state = ParserState::Iac;
                } else {
                    self.push_data_byte(b);
                }
            }
            ParserState::Iac => {
                match b {
                    cmd::IAC => {
                        self.push_data_byte(cmd::IAC);
                        self.state = ParserState::Normal;
                    }
                    c if c == cmd::DO
                        || c == cmd::DONT
                        || c == cmd::WILL
                        || c == cmd::WONT =>
                    {
                        self.state = ParserState::Cmd(c);
                    }
                    cmd::SB => {
                        self.state = ParserState::Sb(None); // option byte comes next
                    }
                    cmd::SE => {
                        // Stray SE without SB — ignore.
                        self.state = ParserState::Normal;
                    }
                    cmd::GA => {
                        // Go-ahead treated as a soft prompt.
                        self.events.push_back(TelnetEvent::EndOfRecord);
                        self.state = ParserState::Normal;
                    }
                    _ => {
                        // Other commands (NOP, BRK, IP, AO, AYT, EC, EL): skip.
                        self.state = ParserState::Normal;
                    }
                }
            }
            ParserState::Cmd(cmd_byte) => {
                self.handle_command(cmd_byte, b);
                self.state = ParserState::Normal;
            }
            ParserState::Sb(opt) => {
                match opt {
                    None => {
                        // First byte after IAC SB is the option code.
                        self.state = ParserState::Sb(Some(b));
                    }
                    Some(opt_byte) => {
                        if b == cmd::IAC {
                            self.state = ParserState::SbIac(opt_byte);
                        } else {
                            self.sb_buf.push(b);
                        }
                    }
                }
            }
            ParserState::SbIac(opt_byte) => {
                if b == cmd::SE {
                    self.handle_subnegotiation(opt_byte);
                    self.sb_buf.clear();
                    self.state = ParserState::Normal;
                } else {
                    // Malformed; bail out.
                    self.sb_buf.clear();
                    self.state = ParserState::Normal;
                }
            }
        }
    }

    // Internal buffer for SB payloads (declared on the struct).

    fn push_data_byte(&mut self, b: u8) {
        // Coalesce data into the last event if it was Data.
        if let Some(TelnetEvent::Data(buf)) = self.events.back_mut() {
            buf.push(b);
        } else {
            self.events.push_back(TelnetEvent::Data(vec![b]));
        }
    }

    fn handle_command(&mut self, cmd_byte: u8, opt_byte: u8) {
        use cmd::*;
        match cmd_byte {
            DO => {
                if self.negotiated.client_will & (1u128 << opt_byte) != 0 {
                    self.pending_out.push_back(vec![cmd::IAC, WILL, opt_byte]);
                    self.negotiated.server_do |= 1u128 << opt_byte;
                } else {
                    self.pending_out.push_back(vec![cmd::IAC, WONT, opt_byte]);
                }
            }
            DONT => {
                self.pending_out.push_back(vec![cmd::IAC, WONT, opt_byte]);
                self.negotiated.server_do &= !(1u128 << opt_byte);
            }
            WILL => {
                // Server offering something: we say DO only for what we want.
                match opt_byte {
                    opt::ECHO => {
                        self.negotiated.server_echo = true;
                        self.pending_out.push_back(vec![cmd::IAC, DO, opt::ECHO]);
                        self.events.push_back(TelnetEvent::Echo(true));
                    }
                    opt::SGA | opt::EOR | opt::COMPRESS | opt::COMPRESS2 => {
                        self.pending_out.push_back(vec![cmd::IAC, DO, opt_byte]);
                    }
                    _ => {
                        self.pending_out.push_back(vec![cmd::IAC, DONT, opt_byte]);
                    }
                }
            }
            WONT => match opt_byte {
                opt::ECHO => {
                    self.negotiated.server_echo = false;
                    self.pending_out.push_back(vec![cmd::IAC, DONT, opt::ECHO]);
                    self.events.push_back(TelnetEvent::Echo(false));
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn handle_subnegotiation(&mut self, opt_byte: u8) {
        match opt_byte {
            opt::TERMINAL_TYPE => {
                // RFC 1091. Two directions:
                //
                //   SB TTYPE SEND   — the server is asking what we are.
                //                       We must answer with
                //                       SB TTYPE IS <name> SE. MUSH servers
                //                       (Penn, Tiny) read this to decide whether
                //                       to grant ANSI to the session, so
                //                       staying silent here means no colours.
                //
                //   SB TTYPE IS <x>  — the server is telling us its own
                //                       type. Recorded for the application,
                //                       which is how we notice a Pueblo
                //                       ("PUEBLO" / "PUEBLO-MXP") and switch
                //                       on MWorld markup.
                let params = std::mem::take(&mut self.sb_buf);
                match params.first() {
                    Some(&SEND) => {
                        let reply = self.terminal_type_reply();
                        self.pending_out.push_back(reply);
                    }
                    _ => {
                        self.events.push_back(TelnetEvent::Subnegotiate {
                            option: opt_byte,
                            params,
                        });
                    }
                }
            }
            opt::COMPRESS => {
                // Pueblo receives `IAC SB COMPRESS WILL SE` for v1 and
                // `IAC SB COMPRESS2 IAC SE` for v2.
                if self.sb_buf.first() == Some(&cmd::WILL) {
                    self.mccp = Some(MccpDecompressor::new_v2());
                    self.negotiated.mccp_active = true;
                    self.events.push_back(TelnetEvent::CompressionBegin);
                }
                self.sb_buf.clear();
            }
            opt::COMPRESS2 => {
                self.mccp = Some(MccpDecompressor::new_v2());
                self.negotiated.mccp_active = true;
                self.events.push_back(TelnetEvent::CompressionBegin);
                self.sb_buf.clear();
            }
            opt::NAWS => {
                let params = std::mem::take(&mut self.sb_buf);
                self.events.push_back(TelnetEvent::Subnegotiate {
                    option: opt_byte,
                    params,
                });
            }
            _ => {
                let params = std::mem::take(&mut self.sb_buf);
                self.events.push_back(TelnetEvent::Subnegotiate {
                    option: opt_byte,
                    params,
                });
            }
        }
    }
}

impl Default for TelnetStream {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_bytes() {
        let mut s = TelnetStream::new();
        s.feed(b"hello world");
        let ev = s.drain_events();
        assert_eq!(ev.len(), 1);
        match &ev[0] {
            TelnetEvent::Data(d) => assert_eq!(d, b"hello world"),
            _ => panic!("expected Data"),
        }
    }

    #[test]
    fn parses_iac_will_echo() {
        let mut s = TelnetStream::new();
        s.feed(&[cmd::IAC, cmd::WILL, opt::ECHO]);
        assert_eq!(
            s.drain_outgoing(),
            vec![vec![cmd::IAC, cmd::DO, opt::ECHO]]
        );
        assert!(matches!(s.drain_events()[0], TelnetEvent::Echo(true)));
    }

    #[test]
    fn parses_iac_ga() {
        let mut s = TelnetStream::new();
        s.feed(&[cmd::IAC, cmd::GA]);
        assert!(matches!(s.drain_events()[0], TelnetEvent::EndOfRecord));
    }

    #[test]
    fn parses_iac_sb_ttype_pueblo() {
        let mut s = TelnetStream::new();
        // IAC SB TTYPE IS PUEBLO IAC SE
        s.feed(&[cmd::IAC, cmd::SB, opt::TERMINAL_TYPE, 0, b'P', b'U', b'E', b'B', b'L', b'O', cmd::IAC, cmd::SE]);
        let ev = s.drain_events();
        match &ev[0] {
            TelnetEvent::Subnegotiate { option, params } => {
                assert_eq!(*option, opt::TERMINAL_TYPE);
                // The TTYPE subnegotiation starts with an `IS`/`SEND`
                // discriminator byte (here 0 = IS) which is part of the params
                // payload; the application is responsible for stripping it.
                assert_eq!(params.as_slice(), &[0u8, b'P', b'U', b'E', b'B', b'L', b'O']);
            }
            _ => panic!("expected Subnegotiate"),
        }
    }

    #[test]
    fn answers_ttype_send_with_our_terminal_type() {
        // A MUSH server asks who we are; we must answer or it withholds ANSI.
        let mut s = TelnetStream::new();
        // IAC SB TTYPE SEND IAC SE
        s.feed(&[
            cmd::IAC, cmd::SB, opt::TERMINAL_TYPE, SEND, cmd::IAC, cmd::SE,
        ]);
        let out = s.drain_outgoing();
        let mut expected = vec![cmd::IAC, cmd::SB, opt::TERMINAL_TYPE, IS];
        expected.extend_from_slice(DEFAULT_TERMINAL_TYPE.as_bytes());
        expected.extend_from_slice(&[cmd::IAC, cmd::SE]);
        assert_eq!(
            out,
            vec![expected],
            "must answer TTYPE SEND so MUSH servers grant ANSI"
        );
    }

    #[test]
    fn ttype_reply_is_configurable_and_rfc1091_clamped() {
        let mut s = TelnetStream::new();
        // Overlong and non-printable input is clamped.
        s.set_terminal_type("pueblo-mxp-very-long-name-here");
        assert_eq!(s.negotiated().terminal_type.len(), 24);

        s.set_terminal_type("PUEBLO-MXP");
        s.feed(&[cmd::IAC, cmd::SB, opt::TERMINAL_TYPE, SEND, cmd::IAC, cmd::SE]);
        let out = s.drain_outgoing();
        let joined: Vec<u8> = out.concat();
        assert!(
            joined.windows(9).any(|w| w == b"PUEBLO-MX"),
            "expected the configured terminal type in the reply, got {joined:?}"
        );
    }

    #[test]
    fn ttype_is_does_not_trigger_a_reply() {
        // The server declaring its own type is an event, not a prompt.
        let mut s = TelnetStream::new();
        s.feed(&[
            cmd::IAC, cmd::SB, opt::TERMINAL_TYPE, 0, b'x', b't', b'e', b'r', b'm',
            cmd::IAC, cmd::SE,
        ]);
        assert!(s.drain_outgoing().is_empty(), "IS should not be answered");
        let evs = s.drain_events();
        assert!(
            matches!(evs.first(), Some(TelnetEvent::Subnegotiate { option, .. }) if *option == opt::TERMINAL_TYPE),
            "IS should surface as an event, got {evs:?}"
        );
    }

    #[test]
    fn wont_do_naws_or_mxp() {
        // Pueblo originally refused NAWS and MXP with DONT. We mirror that.
        let mut naws = TelnetStream::new();
        naws.feed(&[cmd::IAC, cmd::WILL, opt::NAWS]);
        let out = naws.drain_outgoing();
        assert_eq!(out, vec![vec![cmd::IAC, cmd::DONT, opt::NAWS]]);

        let mut mxp = TelnetStream::new();
        mxp.feed(&[cmd::IAC, cmd::DO, opt::MXP]);
        // We didn't WILL MXP, so server DO should be answered with WONT.
        let out = mxp.drain_outgoing();
        assert_eq!(out, vec![vec![cmd::IAC, cmd::WONT, opt::MXP]]);
    }
}