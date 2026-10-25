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

#[test]
fn html_entities_are_decoded() {
    use mist_core::ansi::AnsiParser;
    use mist_core::markup::Parser;

    let mut ansi = AnsiParser::new();
    let spans = ansi.feed_spans("&nbsp;* A &amp; B &lt;tag&gt; &#65; &#x42; &unknown;");
    let mut markup = Parser::new();
    let doc = markup.parse_spans(&spans).expect("parse");

    // Collect the text.
    let mut text = String::new();
    fn walk(n: &mist_core::markup::Node, out: &mut String) {
        match n {
            mist_core::markup::Node::Text(t) => out.push_str(&t.text),
            mist_core::markup::Node::Document { children }
            | mist_core::markup::Node::Bold(children)
            | mist_core::markup::Node::Italic(children)
            | mist_core::markup::Node::Underline(children)
            | mist_core::markup::Node::MudText(children)
            | mist_core::markup::Node::Unknown { children, .. }
            | mist_core::markup::Node::Font { children, .. } => {
                for c in children { walk(c, out); }
            }
            _ => {}
        }
    }
    walk(&doc, &mut text);

    assert_eq!(
        text,
        "\u{a0}* A & B <tag> A B &unknown;",
        "entity decoding wrong: {text:?}"
    );
}

// ---------------------------------------------------------------------------
// MCCP
// ---------------------------------------------------------------------------

/// A MUSH that negotiates MCCP2 sends `IAC SB COMPRESS2 IAC SE` and then
/// everything is a zlib stream. The client must answer `DO COMPRESS2`,
/// switch to decompressing, and hand the decompressed bytes to the rest of
/// the pipeline.
#[test]
fn mccp2_decompresses_the_stream_after_the_start_sequence() {
    use mist_core::telnet::{cmd, opt, TelnetEvent, TelnetStream};

    // What the MUSH would have sent.
    let payload = b"\x1b[32mHello, compressed world!\x1b[0m\r\n";
    let mut encoder =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write;
    encoder.write_all(payload).unwrap();
    let compressed = encoder.finish().unwrap();

    let mut telnet = TelnetStream::new();

    // The negotiation: the server offers WILL COMPRESS2, we answer DO.
    telnet.feed(&[cmd::IAC, cmd::WILL, opt::COMPRESS2]);
    assert!(
        telnet.drain_outgoing().contains(&vec![cmd::IAC, cmd::DO, opt::COMPRESS2]),
        "must accept MCCP2"
    );

    // The start sequence, then the compressed bytes.
    telnet.feed(&[cmd::IAC, cmd::SB, opt::COMPRESS2, cmd::IAC, cmd::SE]);
    assert!(
        telnet.drain_events().iter().any(|e| matches!(e, TelnetEvent::CompressionBegin)),
        "compression should be announced"
    );

    // Now the compressed payload.
    telnet.feed(&compressed);

    let mut text = String::new();
    for ev in telnet.drain_events() {
        if let TelnetEvent::Data(d) = ev {
            text.push_str(&String::from_utf8_lossy(&d));
        }
    }
    // The payload carries its own ANSI codes; the telnet layer passes
    // bytes through verbatim. (Stripping them is the UI layer's job.)
    assert_eq!(
        text, "\x1b[32mHello, compressed world!\x1b[0m\r\n",
        "decompressed payload wrong"
    );
}

#[test]
fn mccp2_handles_a_stream_split_across_chunks() {
    use mist_core::telnet::{cmd, opt, TelnetEvent, TelnetStream};

    let payload = b"line one\r\nline two\r\nline three\r\n";
    let mut encoder =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write;
    encoder.write_all(payload).unwrap();
    let compressed = encoder.finish().unwrap();

    let mut telnet = TelnetStream::new();
    telnet.feed(&[cmd::IAC, cmd::SB, opt::COMPRESS2, cmd::IAC, cmd::SE]);
    telnet.drain_events();

    // Feed the compressed stream one byte at a time — the worst case a
    // real network produces.
    for byte in compressed.iter() {
        telnet.feed(&[*byte]);
    }

    let mut text = String::new();
    for ev in telnet.drain_events() {
        if let TelnetEvent::Data(d) = ev {
            text.push_str(&String::from_utf8_lossy(&d));
        }
    }
    assert_eq!(text, "line one\r\nline two\r\nline three\r\n");
}

#[test]
fn mccp2_keeps_bytes_the_decoder_could_not_use_yet() {
    use mist_core::telnet::{cmd, opt, TelnetEvent, TelnetStream};

    let payload = b"a payload long enough to need several chunks to decode";
    let mut encoder =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write;
    encoder.write_all(payload).unwrap();
    let compressed = encoder.finish().unwrap();

    let mut telnet = TelnetStream::new();
    telnet.feed(&[cmd::IAC, cmd::SB, opt::COMPRESS2, cmd::IAC, cmd::SE]);
    telnet.drain_events();

    // Feed the compressed stream in small chunks. The decoder will
    // sometimes need more bytes than a chunk provides; those bytes must
    // be kept for the next chunk rather than dropped.
    for chunk in compressed.chunks(7) {
        telnet.feed(chunk);
    }

    let mut text = String::new();
    for ev in telnet.drain_events() {
        if let TelnetEvent::Data(d) = ev {
            text.push_str(&String::from_utf8_lossy(&d));
        }
    }
    assert_eq!(
        text, "a payload long enough to need several chunks to decode",
        "bytes were lost between chunks"
    );
}
