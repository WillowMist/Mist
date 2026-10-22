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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Document { children: Vec<Node> },
    Text(String),
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
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(&mut self, input: &str) -> Result<Node> {
        self.bytes = input.as_bytes().to_vec();
        self.pos = 0;
        let children = self.parse_children(None)?;
        Ok(Node::Document { children })
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
        let mut text = String::new();
        while let Some(b) = self.peek() {
            if let Some(end_tag) = end {
                if self.starts_with(end_tag) {
                    break;
                }
            }
            if b == b'<' {
                if self.starts_with(b"</") {
                    // Orphan closing tag (e.g. `</xch_mudtext>` with no
                    // matching open) — silently consume the entire token
                    // and keep parsing. Real-world MUSHes emit orphan
                    // closers all the time (ANSI stripping, server-side
                    // text substitution, MUX color codes prepended,
                    // etc.). We don't want these to leak into the user's
                    // scrollback as visible markup junk.
                    if !text.is_empty() {
                        out.push(Node::Text(std::mem::take(&mut text)));
                    }
                    let _ = self.consume_close_tag_token();
                } else {
                    if !text.is_empty() {
                        out.push(Node::Text(std::mem::take(&mut text)));
                    }
                    let node = self.parse_element()?;
                    out.push(node);
                }
            } else {
                text.push(b as char);
                self.bump();
            }
        }
        if !text.is_empty() {
            out.push(Node::Text(text));
        }
        Ok(out)
    }

    fn starts_with(&self, needle: &[u8]) -> bool {
        self.bytes[self.pos..].starts_with(needle)
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