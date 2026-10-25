//! Pueblo/MWorld markup parser.
//!
//! Source of truth: `api/ChHtmSym.cpp:110-180` (the tag token table) and the
//! pre-processor in `api/ChHtmWnd.cpp` and `api/ChHTPriv.cpp`. Pueblo's own
//! extensions live in the `xch_*` namespace.
//!
//! v1 scope (per SPEC.md §3):
//!   `<b>` `<u>` `<i>` `<font color size>` `<a href>` `<a xch_cmd>`
//!   `<img src>` `<hr>` `<br>` `<xch_page clear>` `<xch_mudtext>`
//! plus the full set of MXP tags Pueblo negotiates once MXP is enabled
//! (`<SEND>`, `<A HINT>`, `<A HREF>`, `<COLOR>`, etc.).
//!
//! This file is scaffolding: a hand-written recursive-descent parser for the
//! tag subset. The tree it returns is consumed by `pueblo-render`.

use crate::error::{PuebloError, Result};

use crate::ansi::StyledText;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Document { children: Vec<Node> },
    /// A run of text with the SGR style that was in force when it
    /// arrived. The style comes from the ANSI layer; markup tags like
    /// `<font color=...>` layer their own colour on top at paint time.
    Text(StyledText),
    Bold(Vec<Node>),
    Italic(Vec<Node>),
    Underline(Vec<Node>),
    Font { color: Option<String>, size: Option<u8>, children: Vec<Node> },
    LineBreak,
    HorizontalRule,
    /// `<img src="..." alt="..." [width=".." height=".."]>`. Width/height
    /// are pixel hints the renderer may use to size the image before the
    /// texture loads; both are optional.
    Image {
        src: String,
        alt: String,
        width: Option<u32>,
        height: Option<u32>,
    },
    Anchor { href: Option<String>, xch_cmd: Option<String>, children: Vec<Node> },
    PageBreak { clear: Option<String> },
    MudText(Vec<Node>),
    /// Unknown tag — preserved verbatim for round-tripping.
    Unknown { tag: String, attrs: Vec<(String, String)>, children: Vec<Node> },
}

/// Decode the HTML entities a MUSH actually emits.
///
/// Pueblo's markup is HTML-flavoured, and `&nbsp;` shows up in
/// list markup constantly (see the mock's "You see:" list). We decode
/// the handful that matter and leave anything else alone, so a stray
/// `&` in prose is not mangled.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find('&') {
        out.push_str(&rest[..idx]);
        let tail = &rest[idx..];
        // An entity is &name; where name is alphanumerics.
        let end = tail
            .find(';')
            .filter(|e| *e <= 12)
            .unwrap_or(usize::MAX);
        if end == usize::MAX {
            out.push('&');
            rest = &tail[1..];
            continue;
        }
        let name = &tail[1..end];
        let replacement = match name {
            "nbsp" => Some('\u{a0}'),
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            // Numeric: &#123; or &#x1F;
            _ if name.starts_with('#') => {
                let code = if let Some(hex) = name.strip_prefix("#x").or(name.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(dec) = name.strip_prefix('#') {
                    dec.parse::<u32>().ok()
                } else {
                    None
                };
                code.and_then(char::from_u32)
            }
            _ => None,
        };
        match replacement {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                // Not an entity we know; leave it verbatim.
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Tags that the Pueblo render treats as void — they never carry children
/// even if the markup omits the trailing `/`. Matches HTML's `<br>`/`<hr>`
/// model: `xch_page`, `xch_mudtext`, `hr`, `br`, `img`, `embed`, `xch_pane`.
pub fn is_void_tag(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "xch_page" | "xch_pane" | "xch_mudtext" | "hr" | "br" | "img" | "embed"
    )
}

#[derive(Default)]
pub struct Parser {
    /// Bytes consumed so far; useful for error messages.
    pos: usize,
    bytes: Vec<u8>,
    /// The SGR style in force at each byte offset in `bytes`. Always the
    /// same length as `bytes`; `styles[i]` is the style of `bytes[i]`.
    /// Empty for the plain-`parse` path, where every byte is default.
    styles: Vec<crate::ansi::SgrState>,
    /// Byte offsets where the style changes, as (offset, style). Used to
    /// slice a text run into styled pieces.
    style_runs: Vec<(usize, crate::ansi::SgrState)>,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(&mut self, input: &str) -> Result<Node> {
        self.bytes = input.as_bytes().to_vec();
        self.pos = 0;
        self.styles = Vec::new();
        self.style_runs = Vec::new();
        let children = self.parse_children(None)?;
        Ok(Node::Document { children })
    }

    /// Parse styled spans.
    ///
    /// The markup structure is identical to [`Parser::parse`], but the
    /// text between tags keeps the SGR style that was in force when it
    /// arrived from the wire. A run of text that straddles a style
    /// change is split so each piece carries its own style.
    pub fn parse_spans(&mut self, spans: &[StyledText]) -> Result<Node> {
        // Flatten the spans into a byte buffer plus a style-per-byte map.
        let mut bytes: Vec<u8> = Vec::new();
        let mut styles: Vec<crate::ansi::SgrState> = Vec::new();
        let mut style_runs: Vec<(usize, crate::ansi::SgrState)> = Vec::new();
        for span in spans {
            if span.text.is_empty() {
                continue;
            }
            let start = bytes.len();
            bytes.extend_from_slice(span.text.as_bytes());
            styles.extend(std::iter::repeat(span.sgr).take(span.text.len()));
            style_runs.push((start, span.sgr));
        }
        self.bytes = bytes;
        self.pos = 0;
        self.styles = styles;
        self.style_runs = style_runs;
        let children = self.parse_children(None)?;
        Ok(Node::Document { children })
    }

    /// The style in force at byte offset `at`.
    fn style_at(&self, at: usize) -> crate::ansi::SgrState {
        if self.styles.is_empty() {
            return crate::ansi::SgrState::default();
        }
        self.styles
            .get(at)
            .copied()
            .unwrap_or_else(|| self.styles[self.styles.len() - 1])
    }

    /// Split the byte range `[from, to)` into styled pieces, one per
    /// style change.
    fn styled_slice(&self, from: usize, to: usize) -> Vec<StyledText> {
        if self.styles.is_empty() {
            let text = decode_entities(&String::from_utf8_lossy(&self.bytes[from..to]));
            if text.is_empty() {
                return Vec::new();
            }
            return vec![StyledText::plain(text)];
        }
        let mut out: Vec<StyledText> = Vec::new();
        let mut i = from;
        while i < to {
            let style = self.style_at(i);
            let mut j = i + 1;
            while j < to && self.style_at(j) == style {
                j += 1;
            }
            let text = decode_entities(&String::from_utf8_lossy(&self.bytes[i..j]));
            if !text.is_empty() {
                out.push(StyledText::new(text, style));
            }
            i = j;
        }
        out
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    fn parse_children(&mut self, end: Option<&[u8]>) -> Result<Vec<Node>> {
        let mut out = Vec::new();
        // Start offset of the text run we're accumulating. The bytes
        // between here and `self.pos` are plain text, which we slice out
        // styled once we know where the run ends.
        let mut text_start = self.pos;
        while let Some(b) = self.peek() {
            if let Some(end_tag) = end {
                if self.starts_with(end_tag) {
                    break;
                }
            }
            if b == b'<' {
                if !self.looks_like_tag() {
                    // Not a tag — literal '<' text.
                    self.bump();
                    continue;
                }
                if self.starts_with(b"</") {
                    // Orphan closing tag (e.g. `</xch_mudtext>` with no
                    // matching open) — silently consume the entire token
                    // and keep parsing. Real-world MUSHes emit orphan
                    // closers all the time (ANSI stripping, server-side
                    // text substitution, MUX color codes prepended,
                    // etc.). We don't want these to leak into the user's
                    // scrollback as visible markup junk.
                    self.flush_text(&mut out, text_start, self.pos);
                    text_start = self.pos;
                    let _ = self.consume_close_tag_token();
                    text_start = self.pos;
                } else {
                    self.flush_text(&mut out, text_start, self.pos);
                    text_start = self.pos;
                    let node = self.parse_element()?;
                    out.push(node);
                    text_start = self.pos;
                }
            } else {
                self.bump();
            }
        }
        self.flush_text(&mut out, text_start, self.pos);
        Ok(out)
    }

    /// Emit the bytes `[from, to)` as one or more styled text nodes.
    fn flush_text(&self, out: &mut Vec<Node>, from: usize, to: usize) {
        if to <= from {
            return;
        }
        for piece in self.styled_slice(from, to) {
            out.push(Node::Text(piece));
        }
    }

    fn starts_with(&self, needle: &[u8]) -> bool {
        self.bytes[self.pos..].starts_with(needle)
    }


    /// Does a tag start at `self.pos`?
    ///
    /// A `<` only opens a tag when it is followed by a letter (or `/`
    /// for a closing tag) and a matching `>` appears later. Anything
    /// else is literal text.
    ///
    /// The letter requirement is what keeps MUD banner art intact:
    /// ASCII art is full of `<`, `>` and `/`, but almost never `<`
    /// followed by a letter *and* a closing `>`.
    ///
    /// The scan distance has to be generous because a legitimate tag can
    /// be long — an `<img src="data:image/png;base64,…">` carries
    /// kilobytes before its `>`.
    fn looks_like_tag(&self) -> bool {
        let Some(&first) = self.bytes.get(self.pos + 1) else {
            return false;
        };
        if !(first.is_ascii_alphabetic() || first == b'/') {
            return false;
        }
        // Find the closing '>'. A tag can't contain a raw '<' — if we
        // hit one first, this isn't a tag.
        let mut i = self.pos + 2;
        while i < self.bytes.len() {
            match self.bytes[i] {
                b'>' => return true,
                b'<' => return false,
                _ => i += 1,
            }
        }
        false
    }

    fn parse_element(&mut self) -> Result<Node> {
        // Consume '<'
        self.expect(b'<')?;
        let (tag, attrs) = self.parse_tag_and_attrs()?;
        // Self-closing tags
        let self_closing = self.peek() == Some(b'/');
        if self_closing {
            self.expect(b'/')?;
        }
        self.expect(b'>')?;
        if self_closing {
            return Ok(self.make_leaf(&tag, attrs));
        }
        if is_void_tag(&tag) {
            // Void elements with structure (e.g. <xch_page clear=text>) still
            // go through `make_node` with empty children so their attributes
            // survive. Pure self-closing leaves (br, hr, img) use make_leaf.
            return Ok(self.make_node(&tag, attrs, Vec::new()));
        }
        // Block-level children termination
        let end_tag = format!("</{}>", tag.to_ascii_lowercase());
        let end_bytes = end_tag.as_bytes();
        let children = self.parse_children(Some(end_bytes))?;
        // Consume </tag>
        if self.starts_with(end_bytes) {
            for _ in 0..end_bytes.len() {
                self.bump();
            }
        }
        Ok(self.make_node(&tag, attrs, children))
    }

    fn make_leaf(&self, tag: &str, attrs: Vec<(String, String)>) -> Node {
        match tag.to_ascii_lowercase().as_str() {
            "br" => Node::LineBreak,
            "hr" => Node::HorizontalRule,
            "img" => {
                let src = attr(&attrs, "src").unwrap_or_default();
                let alt = attr(&attrs, "alt").unwrap_or_default();
                let width = attr(&attrs, "width").and_then(|s| s.parse().ok());
                let height = attr(&attrs, "height").and_then(|s| s.parse().ok());
                Node::Image { src, alt, width, height }
            }
            _ => Node::Unknown { tag: tag.to_string(), attrs, children: vec![] },
        }
    }

    fn make_node(&self, tag: &str, attrs: Vec<(String, String)>, children: Vec<Node>) -> Node {
        match tag.to_ascii_lowercase().as_str() {
            "b" | "strong" => Node::Bold(children),
            "i" | "em" => Node::Italic(children),
            "u" => Node::Underline(children),
            "font" => Node::Font {
                color: attr(&attrs, "color").or_else(|| attr(&attrs, "fgcolor")),
                size: attr(&attrs, "size").and_then(|s| s.parse().ok()),
                children,
            },
            "a" => Node::Anchor {
                href: attr(&attrs, "href"),
                xch_cmd: attr(&attrs, "xch_cmd"),
                children,
            },
            "xch_page" => Node::PageBreak { clear: attr(&attrs, "clear") },
            "xch_mudtext" => Node::MudText(children),
            "img" => Node::Image {
                src: attr(&attrs, "src").unwrap_or_default(),
                alt: attr(&attrs, "alt").unwrap_or_default(),
                width: attr(&attrs, "width").and_then(|s| s.parse().ok()),
                height: attr(&attrs, "height").and_then(|s| s.parse().ok()),
            },
            _ => Node::Unknown { tag: tag.to_string(), attrs, children },
        }
    }

    fn parse_tag_and_attrs(&mut self) -> Result<(String, Vec<(String, String)>)> {
        let mut tag = String::new();
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>' => break,
                _ => {
                    tag.push(b as char);
                    self.bump();
                }
            }
        }
        if tag.is_empty() {
            return Err(PuebloError::Markup("empty tag".into()));
        }
        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'>') | Some(b'/') => break,
                None => break,
                _ => {
                    let (k, v) = self.parse_attr()?;
                    attrs.push((k, v));
                }
            }
        }
        Ok((tag, attrs))
    }

    /// Consume a `<…>` token (open or close tag) from the input as
    /// plain text. Used to recover from orphan closing tags, malformed
    /// attributes, and the like. Returns the literal token as a
    /// `String` so the renderer can pass it through verbatim.
    fn consume_close_tag_token(&mut self) -> String {
        let mut out = String::new();
        // We are positioned at `<`.
        out.push('<');
        self.bump();
        // Optional `/` for a closing tag.
        if self.peek() == Some(b'/') {
            out.push('/');
            self.bump();
        }
        // Tag name.
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'>' || b == b'/' {
                break;
            }
            out.push(b as char);
            self.bump();
        }
        // Drain any attribute text up to the closing `>`.
        let mut depth: usize = 0;
        while let Some(b) = self.peek() {
            if b == b'<' {
                // Stray nested tag in the same token — bail.
                break;
            }
            if b == b'>' && depth == 0 {
                out.push('>');
                self.bump();
                break;
            }
            if b == b'"' {
                depth += 1;
                out.push('"');
                self.bump();
            } else if depth > 0 && b == b'"' {
                depth -= 1;
                out.push('"');
                self.bump();
            } else {
                out.push(b as char);
                self.bump();
            }
        }
        out
    }

    fn parse_attr(&mut self) -> Result<(String, String)> {
        let mut name = String::new();
        while let Some(b) = self.peek() {
            match b {
                b'=' | b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' => break,
                _ => {
                    name.push(b as char);
                    self.bump();
                }
            }
        }
        let mut value = String::new();
        if self.peek() == Some(b'=') {
            self.bump();
            match self.peek() {
                Some(b'"') => {
                    self.bump();
                    while let Some(b) = self.peek() {
                        if b == b'"' {
                            self.bump();
                            break;
                        }
                        value.push(b as char);
                        self.bump();
                    }
                }
                Some(b'\'') => {
                    self.bump();
                    while let Some(b) = self.peek() {
                        if b == b'\'' {
                            self.bump();
                            break;
                        }
                        value.push(b as char);
                        self.bump();
                    }
                }
                _ => {
                    while let Some(b) = self.peek() {
                        if b == b' ' || b == b'>' || b == b'/' || b == b'\t' {
                            break;
                        }
                        value.push(b as char);
                        self.bump();
                    }
                }
            }
        }
        Ok((name.to_ascii_lowercase(), value))
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, b: u8) -> Result<()> {
        if self.peek() == Some(b) {
            self.bump();
            Ok(())
        } else {
            Err(PuebloError::Markup(format!(
                "expected '{}', got {:?}",
                b as char,
                self.peek().map(|c| c as char)
            )))
        }
    }
}

fn attr(attrs: &[(String, String)], key: &str) -> Option<String> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_text() {
        let mut p = Parser::new();
        let n = p.parse("hello world").unwrap();
        assert_eq!(
            n,
            Node::Document { children: vec![Node::Text("hello world".into())] }
        );
    }

    #[test]
    fn parses_xch_page() {
        let mut p = Parser::new();
        let n = p.parse("<xch_page clear=text>foo").unwrap();
        match n {
            Node::Document { children } => {
                assert!(matches!(&children[0], Node::PageBreak { clear: Some(_) }));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_anchor_with_xch_cmd() {
        let mut p = Parser::new();
        let n = p.parse(r#"<a xch_cmd="north">north</a>"#).unwrap();
        match n {
            Node::Document { children } => match &children[0] {
                Node::Anchor { xch_cmd, children, .. } => {
                    assert_eq!(xch_cmd.as_deref(), Some("north"));
                    assert_eq!(children, &vec![Node::Text("north".into())]);
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn parses_font_color() {
        let mut p = Parser::new();
        let n = p.parse("<font color=\"#ff0000\" size=\"3\">x</font>").unwrap();
        match n {
            Node::Document { children } => match &children[0] {
                Node::Font { color, size, .. } => {
                    assert_eq!(color.as_deref(), Some("#ff0000"));
                    assert_eq!(*size, Some(3));
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }
}