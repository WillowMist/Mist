//! Smoke test against a real `mist-mock` binary (started by the test).
//!
//! Verifies that the bytes flowing back from the mock are decoded through
//! mist-core's telnet state machine + ANSI parser the same way the egui shell
//! does.

use mist_core::ansi::{AnsiEvent, AnsiParser};
use mist_core::telnet::TelnetStream;
use std::process::Command;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_to_mock_and_echo() {
    // Find a free port and start the mock there.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mock_path = env!("CARGO_MANIFEST_DIR").to_string()
        + "/../target/debug/mist-mock";
    let mut mock = Command::new(&mock_path)
        .env("MOCK_MUD_PORT", port.to_string())
        // These tests assert on the plain ANSI banner.
        .env("MOCK_MUD_MODE", "plain")
        .spawn()
        .expect("spawn mist-mock");
    // Give the mock a beat to bind.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Now do exactly what mist-ui's connect() does.
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (read, mut write) = stream.into_split();
    let mut telnet = TelnetStream::new();
    let mut ansi = AnsiParser::new();
    let mut reader = tokio::io::BufReader::new(read);

    // Read until we see the banner (or timeout).
    let mut collected = String::new();
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
    loop {
        let read_fut = reader.read(&mut tmp);
        tokio::select! {
            r = read_fut => {
                let n = r.unwrap();
                if n == 0 { break; }
                buf.extend_from_slice(&tmp[..n]);
            }
            _ = tokio::time::sleep_until(deadline.into()) => break,
        }
        telnet.feed(&buf);
        buf.clear();
        for reply in telnet.drain_outgoing() {
            write.write_all(&reply).await.unwrap();
        }
        for ev in telnet.drain_events() {
            if let mist_core::telnet::TelnetEvent::Data(d) = ev {
                for a in ansi.feed(&String::from_utf8_lossy(&d)).unwrap_or_default() {
                    if let AnsiEvent::Text(t) = a {
                        collected.push_str(&t);
                    }
                }
            }
        }
        if collected.contains("Pueblo Mock MUD") {
            break;
        }
    }
    assert!(
        collected.contains("Pueblo Mock MUD"),
        "expected banner text, got: {collected:?}"
    );

    // Echo round-trip
    write.write_all(b"hello\n").await.unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
    let mut echoed = String::new();
    buf.clear();
    loop {
        let read_fut = reader.read(&mut tmp);
        tokio::select! {
            r = read_fut => {
                let n = r.unwrap();
                if n == 0 { break; }
                buf.extend_from_slice(&tmp[..n]);
            }
            _ = tokio::time::sleep_until(deadline.into()) => break,
        }
        telnet.feed(&buf);
        buf.clear();
        for ev in telnet.drain_events() {
            if let mist_core::telnet::TelnetEvent::Data(d) = ev {
                for a in ansi.feed(&String::from_utf8_lossy(&d)).unwrap_or_default() {
                    if let AnsiEvent::Text(t) = a {
                        echoed.push_str(&t);
                    }
                }
            }
        }
        if echoed.contains("hello") {
            break;
        }
    }
    assert!(echoed.contains("hello"), "expected echoed text, got: {echoed:?}");

    let _ = mock.kill();
}