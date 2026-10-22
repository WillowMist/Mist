//! ANSI/VT100 escape sequence parser → styled text spans.
//!
//! Source of truth: `api/ChTxtWnd.cpp` and `api/ChTxtEng.cpp` in the original
//! Pueblo/UE 2.61 source.
//!
//! This module is the v1 scaffolding: it tokenises an input stream into a
//! stream of [`AnsiEvent`]s. Full styling and 256/truecolor are landmines for
//! later milestones.

use crate::error::{PuebloError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnsiEvent {
    Text(String),
    /// `\r`, `\n`, `\r\n` — decoded.
    Newline,
    /// CSI Ps A through Ps Z / `Ps ` etc. Surface to the renderer.
    CursorUp(u16),
    CursorDown(u16),
    CursorForward(u16),
    CursorBack(u16),
    CursorPos { row: u16, col: u16 },
    EraseDisplay(u16),
    EraseLine(u16),
    Sgr(SgrState),
    /// Anything we don't recognise — preserved verbatim for the renderer to
    /// decide whether to swallow or surface.
    UnknownEscape(String),
}

/// Style state, kept between SGR sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SgrState {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub fg: Color,
    pub bg: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Default for Color {
    fn default() -> Self {
        Color::Default
    }
}

pub struct AnsiParser {
    state: ParserState,
    sb: String,
}

#[derive(Debug, Clone, Copy)]
enum ParserState {
    Normal,
    Esc,
    Csi,
    Osc,
}

impl AnsiParser {
    pub fn new() -> Self {
        Self {
            state: ParserState::Normal,
            sb: String::new(),
        }
    }

    pub fn feed(&mut self, input: &str) -> Result<Vec<AnsiEvent>> {
        let mut out = Vec::new();
        let mut text = String::new();
        let mut flush = |out: &mut Vec<AnsiEvent>, text: &mut String| {
            if !text.is_empty() {
                out.push(AnsiEvent::Text(std::mem::take(text)));
            }
        };
        for c in input.chars() {
            match self.state {
                ParserState::Normal => match c as u32 {
                    0x1B => {
                        flush(&mut out, &mut text);
                        self.state = ParserState::Esc;
                    }
                    0x07 => {} // BEL — ignore
                    _ => text.push(c),
                },
                ParserState::Esc => match c {
                    '[' => self.state = ParserState::Csi,
                    _ => {
                        self.state = ParserState::Normal;
                        out.push(AnsiEvent::UnknownEscape(format!("\\x1B{}", c)));
                    }
                },
                ParserState::Csi => match c {
                    '0'..='9' | '?' | ';' => self.sb.push(c),
                    _ => {
                        let payload = std::mem::take(&mut self.sb);
                        let ev = self.parse_csi(&payload, c).ok_or_else(|| {
                            PuebloError::Markup(format!("bad CSI: ESC[{}{}", payload, c))
                        })?;
                        out.push(ev);
                        self.state = ParserState::Normal;
                    }
                },
                ParserState::Osc => {
                    if c as u32 == 0x07 {
                        self.state = ParserState::Normal;
                    }
                }
            }
        }
        flush(&mut out, &mut text);
        Ok(out)
    }

    fn parse_csi(&self, params: &str, final_byte: char) -> Option<AnsiEvent> {
        let p = params.trim_start_matches('?');
        let nums: Vec<u16> = if p.is_empty() {
            vec![]
        } else {
            p.split(';').map(|s| s.parse().ok()).collect::<Option<Vec<_>>>()?
        };
        let n = |i: usize, default: u16| nums.get(i).copied().unwrap_or(default);
        match final_byte {
            'A' => Some(AnsiEvent::CursorUp(n(0, 1))),
            'B' => Some(AnsiEvent::CursorDown(n(0, 1))),
            'C' => Some(AnsiEvent::CursorForward(n(0, 1))),
            'D' => Some(AnsiEvent::CursorBack(n(0, 1))),
            'H' | 'f' => Some(AnsiEvent::CursorPos { row: n(0, 1), col: n(1, 1) }),
            'J' => Some(AnsiEvent::EraseDisplay(n(0, 0))),
            'K' => Some(AnsiEvent::EraseLine(n(0, 0))),
            'm' => Some(AnsiEvent::Sgr(sgr_from_params(&nums))),
            _ => Some(AnsiEvent::UnknownEscape(format!("\\x1B[{}{}", params, final_byte))),
        }
    }
}

fn sgr_from_params(nums: &[u16]) -> SgrState {
    let mut s = SgrState::default();
    if nums.is_empty() {
        return s;
    }
    let mut i = 0;
    while i < nums.len() {
        match nums[i] {
            0 => s = SgrState::default(),
            1 => s.bold = true,
            3 => s.italic = true,
            4 => s.underline = true,
            7 => s.reverse = true,
            22 => s.bold = false,
            23 => s.italic = false,
            24 => s.underline = false,
            27 => s.reverse = false,
            30..=37 => s.fg = Color::Indexed((nums[i] - 30) as u8),
            39 => s.fg = Color::Default,
            40..=47 => s.bg = Color::Indexed((nums[i] - 40) as u8),
            49 => s.bg = Color::Default,
            38 => {
                // 38;5;n or 38;2;r;g;b
                if let Some(&mode) = nums.get(i + 1) {
                    if mode == 5 {
                        if let Some(&n) = nums.get(i + 2) {
                            s.fg = Color::Indexed(n as u8);
                            i += 2;
                        }
                    } else if mode == 2 {
                        if let (Some(&r), Some(&g), Some(&b)) = (nums.get(i + 2), nums.get(i + 3), nums.get(i + 4)) {
                            s.fg = Color::Rgb(r as u8, g as u8, b as u8);
                            i += 4;
                        }
                    }
                }
            }
            48 => {
                if let Some(&mode) = nums.get(i + 1) {
                    if mode == 5 {
                        if let Some(&n) = nums.get(i + 2) {
                            s.bg = Color::Indexed(n as u8);
                            i += 2;
                        }
                    } else if mode == 2 {
                        if let (Some(&r), Some(&g), Some(&b)) = (nums.get(i + 2), nums.get(i + 3), nums.get(i + 4)) {
                            s.bg = Color::Rgb(r as u8, g as u8, b as u8);
                            i += 4;
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_text() {
        let mut p = AnsiParser::new();
        let ev = p.feed("hello").unwrap();
        assert_eq!(ev, vec![AnsiEvent::Text("hello".into())]);
    }

    #[test]
    fn parses_sgr_bold() {
        let mut p = AnsiParser::new();
        let ev = p.feed("\x1B[1mbold\x1B[0m").unwrap();
        assert_eq!(
            ev,
            vec![
                AnsiEvent::Sgr(SgrState { bold: true, ..Default::default() }),
                AnsiEvent::Text("bold".into()),
                AnsiEvent::Sgr(SgrState::default()),
            ]
        );
    }

    #[test]
    fn parses_256color_fg() {
        let mut p = AnsiParser::new();
        let ev = p.feed("\x1B[38;5;202m").unwrap();
        // No leading text + one SGR event
        assert_eq!(ev.len(), 1);
        match &ev[0] {
            AnsiEvent::Sgr(s) => assert_eq!(s.fg, Color::Indexed(202)),
            _ => panic!("expected Sgr"),
        }
    }

    #[test]
    fn parses_cursor_pos() {
        let mut p = AnsiParser::new();
        let ev = p.feed("\x1B[10;20H").unwrap();
        assert_eq!(ev, vec![AnsiEvent::CursorPos { row: 10, col: 20 }]);
    }
}