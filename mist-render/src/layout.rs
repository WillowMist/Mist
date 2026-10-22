//! Layout primitives for the custom HTML/MWorld renderer.
//!
//! The layout model is intentionally simple: one block-flow stream per pane,
//! broken into lines by word boundaries, with each glyph run carrying a
//! [`Style`] snapshot.

use mist_core::markup::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Color,
}

impl Default for Style {
    fn default() -> Self {
        Self { bold: false, italic: false, underline: false, color: Color::Default }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Named(NamedColor),
    Hex([u8; 3]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedColor {
    Black, Yellow, Cyan, Red, Green, Blue, Magenta, White,
}

impl Color {
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim_start_matches('#');
        if s.len() != 6 { return None; }
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(Color::Hex([r, g, b]))
    }

    pub fn from_named(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "black" => Color::Named(NamedColor::Black),
            "red" => Color::Named(NamedColor::Red),
            "green" => Color::Named(NamedColor::Green),
            "yellow" => Color::Named(NamedColor::Yellow),
            "blue" => Color::Named(NamedColor::Blue),
            "magenta" => Color::Named(NamedColor::Magenta),
            "cyan" => Color::Named(NamedColor::Cyan),
            "white" => Color::Named(NamedColor::White),
            _ => return None,
        })
    }

    pub fn parse(s: &str) -> Self {
        if let Some(c) = Self::from_hex(s) { return c; }
        if let Some(c) = Self::from_named(s) { return c; }
        Color::Default
    }
}

#[derive(Debug, Clone)]
pub struct GlyphRun {
    pub text: String,
    pub style: Style,
    /// Optional link target.
    pub href: Option<String>,
    /// Optional Pueblo xch_cmd action.
    pub xch_cmd: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Line {
    pub runs: Vec<GlyphRun>,
    pub break_kind: BreakKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BreakKind {
    #[default]
    SoftWrap,
    HardBreak,
    PageBreak,
}

#[derive(Debug, Default, Clone)]
pub struct PaneLayout {
    pub lines: Vec<Line>,
    pub current_line: Line,
    pub current_x: f32,
    pub current_style: Style,
}

impl PaneLayout {
    pub fn new() -> Self {
        Self {
            lines: vec![Line { runs: Vec::new(), break_kind: BreakKind::SoftWrap }],
            current_line: Line { runs: Vec::new(), break_kind: BreakKind::SoftWrap },
            current_x: 0.0,
            current_style: Style::default(),
        }
    }

    pub fn push_text(&mut self, text: &str, _wrap_width: f32) {
        let _ = _wrap_width;
        if text.is_empty() { return; }
        let run = GlyphRun {
            text: text.to_string(),
            style: self.current_style,
            href: None,
            xch_cmd: None,
        };
        self.current_line.runs.push(run);
    }

    pub fn hard_break(&mut self) {
        self.current_line.break_kind = BreakKind::HardBreak;
        let finished = std::mem::replace(&mut self.current_line, Line { runs: Vec::new(), break_kind: BreakKind::SoftWrap });
        self.lines.push(finished);
    }

    pub fn page_break(&mut self) {
        self.current_line.break_kind = BreakKind::PageBreak;
        let finished = std::mem::replace(&mut self.current_line, Line { runs: Vec::new(), break_kind: BreakKind::SoftWrap });
        self.lines.push(finished);
    }
}

pub(crate) fn layout_node(node: &Node, pane: &mut PaneLayout, wrap_width: f32) {
    match node {
        Node::Document { children } => {
            for c in children { layout_node(c, pane, wrap_width); }
        }
        Node::Text(t) => pane.push_text(t, wrap_width),
        Node::Bold(children) => {
            let prev = pane.current_style;
            pane.current_style.bold = true;
            for c in children { layout_node(c, pane, wrap_width); }
            pane.current_style = prev;
        }
        Node::Italic(children) => {
            let prev = pane.current_style;
            pane.current_style.italic = true;
            for c in children { layout_node(c, pane, wrap_width); }
            pane.current_style = prev;
        }
        Node::Underline(children) => {
            let prev = pane.current_style;
            pane.current_style.underline = true;
            for c in children { layout_node(c, pane, wrap_width); }
            pane.current_style = prev;
        }
        Node::Font { color, children, .. } => {
            let prev = pane.current_style;
            if let Some(c) = color { pane.current_style.color = Color::parse(c); }
            for c in children { layout_node(c, pane, wrap_width); }
            pane.current_style = prev;
        }
        Node::LineBreak => pane.hard_break(),
        Node::HorizontalRule => {
            pane.push_text(&"─".repeat(60), wrap_width);
            pane.hard_break();
        }
        Node::Image { src, alt, .. } => {
            pane.push_text(&format!("[img:{src}|{alt}]"), wrap_width);
        }
        Node::Anchor { href, xch_cmd, children } => {
            let text = children.iter().map(|c| match c {
                Node::Text(t) => t.clone(),
                _ => String::new(),
            }).collect::<String>();
            pane.current_line.runs.push(GlyphRun {
                text,
                style: pane.current_style,
                href: href.clone(),
                xch_cmd: xch_cmd.clone(),
            });
        }
        Node::PageBreak { .. } => pane.page_break(),
        Node::MudText(children) => {
            for c in children { layout_node(c, pane, wrap_width); }
        }
        Node::Unknown { children, .. } => {
            for c in children { layout_node(c, pane, wrap_width); }
        }
    }
}