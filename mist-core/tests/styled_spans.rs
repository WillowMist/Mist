//! Tests for the styled-span colour pipeline.
//!
//! ANSI SGR state used to be parsed and thrown away, leaving only
//! `<font color=...>` markup able to colour text. `AnsiParser::feed_spans`
//! now attaches the SGR state in force to each text run, and
//! `Parser::parse_spans` carries that state through the markup tree. These
//! tests pin both halves and the join between them.

use mist_core::ansi::{AnsiParser, Color, SgrState, StyledText};
use mist_core::markup::{Node, Parser};

// ---------------------------------------------------------------------------
// ANSI → styled spans
// ---------------------------------------------------------------------------

#[test]
fn plain_text_is_one_default_styled_span() {
    let mut p = AnsiParser::new();
    let spans = p.feed_spans("hello");

    assert_eq!(spans.len(), 1, "plain text should be a single span");
    assert_eq!(spans[0].text, "hello");
    assert_eq!(
        spans[0].sgr,
        SgrState::default(),
        "plain text must carry the default style"
    );
}

#[test]
fn indexed_fg_survives_and_reset_restores_default() {
    let mut p = AnsiParser::new();
    let spans = p.feed_spans("\x1b[31mred\x1b[0mnormal");

    assert_eq!(spans.len(), 2, "expected one span per style run, got {spans:?}");

    assert_eq!(spans[0].text, "red");
    assert_eq!(spans[0].sgr.fg, Color::Indexed(1));
    // A foreground colour must not disturb the other attributes.
    assert!(!spans[0].sgr.bold, "31m should not turn on bold");

    assert_eq!(spans[1].text, "normal");
    assert_eq!(
        spans[1].sgr.fg,
        Color::Default,
        "SGR 0 should reset the foreground to Default"
    );
}

#[test]
fn bold_sgr_sets_bold() {
    let mut p = AnsiParser::new();
    let spans = p.feed_spans("\x1b[1mbold");

    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "bold");
    assert!(spans[0].sgr.bold, "1m should enable bold");
}

#[test]
fn truecolor_fg_is_rgb() {
    let mut p = AnsiParser::new();
    let spans = p.feed_spans("\x1b[38;2;255;0;0mtruecolor");

    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "truecolor");
    assert_eq!(spans[0].sgr.fg, Color::Rgb(255, 0, 0));
}

#[test]
fn indexed_256_fg_is_indexed_202() {
    let mut p = AnsiParser::new();

    // The escape on its own has no text to attach to, so `feed_spans`
    // emits no run. What matters is that the parser latches 38;5;202 and
    // applies it to the next text it sees.
    let empty = p.feed_spans("\x1b[38;5;202m");
    assert!(
        empty.is_empty(),
        "a colour escape with no trailing text should emit no span, got {empty:?}"
    );

    let spans = p.feed_spans("x");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "x");
    assert_eq!(spans[0].sgr.fg, Color::Indexed(202));
}

#[test]
fn non_sgr_csi_is_dropped() {
    let mut p = AnsiParser::new();
    let spans = p.feed_spans("\x1b[2Jcleared");

    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "cleared", "2J must not leak into the text");
    assert_eq!(
        spans[0].sgr,
        SgrState::default(),
        "a non-SGR CSI must not change the style"
    );
}

#[test]
fn mid_text_style_changes_split_spans() {
    let mut p = AnsiParser::new();
    let spans = p.feed_spans("\x1b[31mA\x1b[32mB");

    assert_eq!(spans.len(), 2, "a style change should split the run, got {spans:?}");
    assert_eq!(spans[0].text, "A");
    assert_eq!(spans[0].sgr.fg, Color::Indexed(1));
    assert_eq!(spans[1].text, "B");
    assert_eq!(spans[1].sgr.fg, Color::Indexed(2));
    assert_ne!(
        spans[0].sgr.fg, spans[1].sgr.fg,
        "the two runs must carry different foreground colours"
    );
}

// ---------------------------------------------------------------------------
// Styled spans → markup
// ---------------------------------------------------------------------------

#[test]
fn parse_spans_preserves_style_inside_bold() {
    let style = SgrState { fg: Color::Indexed(2), ..Default::default() };
    let span = StyledText::new("<b>bold</b>", style);

    let mut p = Parser::new();
    let doc = p.parse_spans(&[span]).expect("parse_spans should succeed");

    let Node::Document { children } = doc else {
        panic!("expected a Document root, got {doc:?}");
    };
    assert_eq!(children.len(), 1, "expected a single Bold node, got {children:?}");

    let Node::Bold(inner) = &children[0] else {
        panic!("expected Bold, got {:?}", children[0]);
    };
    assert_eq!(inner.len(), 1, "Bold should contain one text node, got {inner:?}");

    let Node::Text(text) = &inner[0] else {
        panic!("expected Text inside Bold, got {:?}", inner[0]);
    };
    assert_eq!(text.text, "bold");
    assert_eq!(
        text.sgr.fg,
        Color::Indexed(2),
        "markup must not discard the SGR style of the text it wraps"
    );
}

#[test]
fn parse_spans_without_markup_is_one_styled_text_node() {
    let style = SgrState {
        bold: true,
        fg: Color::Indexed(3),
        ..Default::default()
    };
    let span = StyledText::new("no markup here", style);

    let mut p = Parser::new();
    let doc = p.parse_spans(&[span]).expect("parse_spans should succeed");

    let Node::Document { children } = doc else {
        panic!("expected a Document root, got {doc:?}");
    };
    assert_eq!(children.len(), 1, "expected one Text node, got {children:?}");

    let Node::Text(text) = &children[0] else {
        panic!("expected Text, got {:?}", children[0]);
    };
    assert_eq!(text.text, "no markup here");
    assert_eq!(text.sgr, style, "the input span's style should be preserved verbatim");
}

#[test]
fn ansi_to_markup_round_trip_keeps_colour() {
    let mut ansi = AnsiParser::new();
    let spans = ansi.feed_spans("\x1b[31mred\x1b[0m");
    assert_eq!(spans.len(), 1, "expected a single coloured run, got {spans:?}");

    let mut parser = Parser::new();
    let doc = parser
        .parse_spans(&spans)
        .expect("parse_spans should accept ANSI-derived spans");

    let Node::Document { children } = doc else {
        panic!("expected a Document root, got {doc:?}");
    };
    assert_eq!(children.len(), 1, "expected one Text node, got {children:?}");

    let Node::Text(text) = &children[0] else {
        panic!("expected Text, got {:?}", children[0]);
    };
    assert_eq!(text.text, "red");
    assert_eq!(
        text.sgr.fg,
        Color::Indexed(1),
        "colour must survive the ANSI → spans → markup trip"
    );
}

#[test]
fn end_to_end_ansi_colour_survives_markup_parsing() {
    use mist_core::ansi::{AnsiParser, Color, StyledText};
    use mist_core::markup::{Node, Parser};

    // Raw bytes as a MUD would send them.
    let wire = "\x1b[31mThe sign says \x1b[0m<font color=\"#80ff80\">GO\x1b[0m</font> north.";
    let mut ansi = AnsiParser::new();
    let spans = ansi.feed_spans(wire);
    assert!(!spans.is_empty());

    let mut markup = Parser::new();
    let doc = markup.parse_spans(&spans).expect("parse");

    // Walk the tree and collect (text, colour) pairs.
    let mut pairs: Vec<(String, Color)> = Vec::new();
    fn walk(n: &Node, out: &mut Vec<(String, Color)>) {
        match n {
            Node::Text(t) => out.push((t.text.clone(), t.sgr.fg)),
            Node::Document { children }
            | Node::Bold(children)
            | Node::Italic(children)
            | Node::Underline(children)
            | Node::MudText(children)
            | Node::Unknown { children, .. }
            | Node::Font { children, .. } => {
                for c in children { walk(c, out); }
            }
            _ => {}
        }
    }
    walk(&doc, &mut pairs);

    // The ANSI red must survive.
    assert!(
        pairs.iter().any(|(t, c)| t.contains("The sign says") && *c == Color::Indexed(1)),
        "ANSI red lost: {pairs:?}"
    );
    // The markup green must be present as a Font node (its colour is
    // applied at paint time, not stored in the span).
    let dumped = format!("{doc:?}");
    assert!(dumped.contains("#80ff80"), "markup colour lost: {dumped}");
}
