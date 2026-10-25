//! Byte-level regression test for MCCP2.
//!
//! The capture is a real exchange with `mist-mock`: the client answers
//! `DO COMPRESS2`, the mock switches to a zlib stream, and the rest of
//! the session is compressed. The test replays those exact bytes through
//! the client's telnet layer and checks the markup comes out intact.

use mist_core::telnet::{cmd, opt, TelnetEvent, TelnetStream};

/// The bytes a client receives from an MCCP2-capable MUSH: the
/// negotiation, the start sequence, then the compressed payload.
const CAPTURE: &[u8] = include_bytes!("data/mccp2_capture.bin");

#[test]
fn mccp2_capture_decompresses_to_the_banner() {
    let mut telnet = TelnetStream::new();
    telnet.feed(CAPTURE);

    let mut text = String::new();
    let mut saw_compression = false;
    for ev in telnet.drain_events() {
        match ev {
            TelnetEvent::CompressionBegin => saw_compression = true,
            TelnetEvent::Data(d) => text.push_str(&String::from_utf8_lossy(&d)),
            _ => {}
        }
    }

    assert!(saw_compression, "the capture should start compression");
    assert!(
        text.contains("<xch_mudtext>"),
        "expected the decompressed banner, got: {text:?}"
    );
    assert!(
        text.contains("Pueblo Mock MUSH"),
        "expected the banner text, got: {text:?}"
    );
    assert!(
        text.contains("<xch_page"),
        "expected the page break, got: {text:?}"
    );
}

#[test]
fn mccp2_capture_answers_do_compress2() {
    let mut telnet = TelnetStream::new();
    telnet.feed(CAPTURE);
    let out = telnet.drain_outgoing();
    assert!(
        out.contains(&vec![cmd::IAC, cmd::DO, opt::COMPRESS2]),
        "must accept MCCP2, sent: {out:?}"
    );
}
