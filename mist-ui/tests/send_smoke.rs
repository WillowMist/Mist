//! Smoke test for the bidirectional flow: open a connection through the same
//! mist-ui runtime machinery the egui shell uses, then send a line via
//! DriverRequest::Send, then assert it appears in the mock's echo response.

use mist_core::ansi::{AnsiEvent, AnsiParser};
use mist_core::telnet::TelnetStream;
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

use mist_ui::session::{DriverRequest, RenderedLine, RuntimeHandle};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_line_round_trip() {
    // Bind an ephemeral port and start the mock on it.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mock_path = env!("CARGO_MANIFEST_DIR").to_string() + "/../target/debug/mist-mock";
    let mut mock = Command::new(&mock_path)
        .env("MOCK_MUD_PORT", port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mist-mock");
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Spin up a mist-ui runtime (just like the egui shell does).
    let runtime = RuntimeHandle::spawn();
    let incoming: std::sync::Arc<Mutex<HashMap<usize, Vec<RenderedLine>>>> =
        std::sync::Arc::new(Mutex::new(HashMap::new()));
    let tab_id = 42;
    let (result_tx, result_rx) = oneshot::channel();
    runtime.request(DriverRequest::Connect {
        tab_id,
        host: "127.0.0.1".into(),
        port,
        incoming: incoming.clone(),
        result: result_tx,
    });
    let res = result_rx.await.unwrap();
    assert!(res.is_ok(), "connect failed: {res:?}");
    // Give the driver a beat to insert the writer into the registry.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Drain the banner lines so the registry is populated.
    {
        let mut m = incoming.lock().await;
        for lines in m.values_mut() {
            lines.clear();
        }
    }

    // Send a line — this is exactly what the egui Enter handler does.
    runtime.request(DriverRequest::Send {
        tab_id,
        line: "hello\r\n".into(),
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Read whatever the mock echoed back.
    let mut m = incoming.lock().await;
    let got = m.entry(tab_id).or_default();
    let combined: String = got.iter().map(|l| l.text.clone()).collect();
    println!("incoming: {combined:?}");
    assert!(
        combined.contains("hello"),
        "expected echoed text in scrollback, got: {combined:?}"
    );

    let _ = mock.kill();
}