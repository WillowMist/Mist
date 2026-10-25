//! End-to-end smoke for the pueblo-mode mock.
//!
//! Spawns `mist-mock` with `MOCK_MUD_MODE=pueblo`, opens a TCP
//! connection through the same `mist-ui` runtime path the client
//! uses, drains the intro banner, sends a couple of commands, and
//! asserts the markup arrives intact.

use mist_core::ansi::{AnsiEvent, AnsiParser};
use mist_core::markup::{Node, Parser as MarkupParser};
use mist_core::telnet::TelnetStream;
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pueblo_intro_render_smoke() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mock_path = env!("CARGO_MANIFEST_DIR").to_string() + "/../target/debug/mist-mock";
    let mut mock = Command::new(&mock_path)
        .env("MOCK_MUD_PORT", port.to_string())
        .env("MOCK_MUD_MODE", "pueblo")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn mock");
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Connect directly (no mist-ui runtime yet — simpler for verifying
    // the raw markup bytes from the mock).
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut telnet = TelnetStream::new();
    let mut ansi = AnsiParser::new();

    // Drain the first chunk the mock sends. Read until we see `<xch_page clear=text>`
    // which signals the start of the Pueblox-mode intro.
    let mut combined = String::new();
    let mut buf = [0u8; 4096];
    let deadline = std::time::Instant::now() + Duration::from_millis(800);
    while combined.len() < 4096 {
        let read_fut = reader.read(&mut buf);
        tokio::select! {
            r = read_fut => {
                let n = r.unwrap();
                if n == 0 { break; }
                telnet.feed(&buf[..n]);
                for reply in telnet.drain_outgoing() {
                    write.write_all(&reply).await.unwrap();
                }
                for ev in telnet.drain_events() {
                    if let mist_core::telnet::TelnetEvent::Data(d) = ev {
                        for a in ansi.feed(&String::from_utf8_lossy(&d)).unwrap_or_default() {
                            if let AnsiEvent::Text(t) = a {
                                combined.push_str(&t);
                            }
                        }
                    }
                }
            }
            _ = tokio::time::sleep_until(deadline.into()) => break,
        }
        if combined.contains("<xch_page clear=text>") {
            break;
        }
    }

    assert!(
        combined.contains("<xch_page clear=text>"),
        "expected <xch_page clear=text> in mock intro, got: {combined:?}"
    );
    assert!(
        combined.contains("<b>") && combined.contains("</b>"),
        "expected <b> markup in intro"
    );
    assert!(
        combined.contains("xch_cmd=\"") || combined.contains("xch_cmd='"),
        "expected at least one xch_cmd anchor; got: {combined:?}"
    );

    // Try parsing the markup end-to-end via mist_core + mist_ui's
    // first_xch_cmd helper through sending a "who" command and parsing
    // the response.
    // The handshake exchange takes a moment: the client reads the
    // server's offers, then sends its WILL/DO + TTYPE reply. The mock
    // waits up to 600ms for that reply, and the connection has to
    // settle before the "who" command is recognised as user input
    // rather than echoed as a TTYPE fragment.
    write.write_all(b"who\r\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(900)).await;
    // Flush any pending outgoing TTYPE bytes the client generated
    // from the initial handshake — they sit in pending_out until the
    // next feed, otherwise the mock reads them as user input.
    for reply in telnet.drain_outgoing() {
        let _ = write.write_all(&reply).await;
    }
    // Diagnostic: tell us what the test's client has actually buffered.
    // Diagnostic
eprintln!("[test] pending_out after 900ms: {} bytes", {
        let total: usize = telnet.drain_outgoing().iter().map(|v| v.len()).sum();
        total
    });
    let mut second_chunk = String::new();
    // The raw telnet payload (decompressed by the telnet layer, but
    // still carrying its ANSI SGR sequences). We check the SGR bytes
    // here rather than on the socket bytes: once the session negotiates
    // MCCP2, the wire bytes are zlib data.
    let mut telnet_payload = Vec::<u8>::new();
    let mut tmp = vec![0u8; 4096];
    let deadline2 = std::time::Instant::now() + Duration::from_millis(2000);
    while second_chunk.len() < 32_000 {
        let read_fut = reader.read(&mut tmp);
        tokio::select! {
            r = read_fut => {
                let n = r.unwrap();
                if n == 0 { break; }
                telnet.feed(&tmp[..n]);
                // Drain any telnet-generated replies (the server's
                // repeated SB TERMINAL_TYPE/TN3270E prompts draw
                // xterm-color / xterm-256color from the client; if
                // they aren't flushed here they sit in pending_out
                // and the mock's reader sees the TTYPE bytes as user
                // input on the next turn).
                for reply in telnet.drain_outgoing() {
                    let _ = write.write_all(&reply).await;
                }
                for ev in telnet.drain_events() {
                    if let mist_core::telnet::TelnetEvent::Data(d) = ev {
                        telnet_payload.extend_from_slice(&d);
                        for a in ansi.feed(&String::from_utf8_lossy(&d)).unwrap_or_default() {
                            if let AnsiEvent::Text(t) = a {
                                second_chunk.push_str(&t);
                            }
                        }
                    }
                }
            }
            _ = tokio::time::sleep_until(deadline2.into()) => break,
        }
        if second_chunk.contains("return to the village square") {
            break;
        }
    }

    assert!(
        second_chunk.contains("Players online"),
        "expected mock to respond to `who` with markup; got {second_chunk:?}"
    );
    // The mock always colour-paints its banner. ANSI SGR sequences
    // must reach the client, even when the client never negotiates
    // beyond ECHO/NAWS. A real MUSH decides whether to send colour
    // based on its own termcap, not on the client's TTYPE reply.
    assert!(
        telnet_payload.windows(2).any(|w| w[0] == 0x1b && w[1] == b'['),
        "expected ANSI SGR sequences in the mock who-page; first 200 bytes: {:02x?}",
        &telnet_payload[..telnet_payload.len().min(200)]
    );

    // Parse the who-response markup and confirm we find an xch_cmd anchor.
    let mut parser = MarkupParser::new();
    println!("[smoke] who-page contents: {:?}", second_chunk);
    let parsed = parser
        .parse(&second_chunk)
        .expect("markup parse failed");
    let cmd = first_xch_cmd(&parsed);
    assert!(
        cmd.is_some(),
        "expected an xch_cmd anchor in the who-page"
    );

    let _ = mock.kill();
}

fn first_xch_cmd(node: &Node) -> Option<String> {
    match node {
        Node::Anchor { xch_cmd, .. } if xch_cmd.is_some() => xch_cmd.clone(),
        Node::Document { children }
        | Node::Bold(children)
        | Node::Italic(children)
        | Node::Underline(children)
        | Node::MudText(children)
        | Node::Unknown { children, .. } => {
            for c in children {
                if let Some(c) = first_xch_cmd(c) {
                    return Some(c);
                }
            }
            None
        }
        Node::Font { children, .. } => {
            for c in children {
                if let Some(c) = first_xch_cmd(c) {
                    return Some(c);
                }
            }
            None
        }
        _ => None,
    }
}
