//! Verifies that a Connect request actually reaches the runtime and
//! produces a connected tab. Drives the same `DriverRequest::Connect`
//! path the egui Connect button uses, without needing a window.

use mist_ui::session::{DriverRequest, RenderedLine, RuntimeHandle};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_request_reaches_the_runtime() {
    // Start the mock on a free port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mock_path =
        env!("CARGO_MANIFEST_DIR").to_string() + "/../target/debug/mist-mock";
    let mut mock = Command::new(&mock_path)
        .env("MOCK_MUD_PORT", port.to_string())
        // These tests assert on the plain ANSI banner.
        .env("MOCK_MUD_MODE", "plain")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mist-mock");
    tokio::time::sleep(Duration::from_millis(250)).await;

    let runtime = RuntimeHandle::spawn();
    let incoming: Arc<Mutex<HashMap<usize, Vec<RenderedLine>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let tab_id = 7usize;
    let (tx, rx) = oneshot::channel();
    runtime.request(DriverRequest::Connect {
        tab_id,
        host: "127.0.0.1".into(),
        port,
        incoming: incoming.clone(),
        result: tx,
    });

    let result = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("connect request timed out — the runtime never answered")
        .expect("result channel dropped");

    assert!(
        result.is_ok(),
        "connect reported failure: {:?}",
        result.err()
    );

    // Give the driver a moment to deliver the banner.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let map = incoming.lock().await;
    let lines = map.get(&tab_id).cloned().unwrap_or_default();
    let joined: String = lines.iter().map(|l| l.text.clone()).collect();

    assert!(
        joined.contains("connected to"),
        "expected the connected notice in scrollback, got: {joined:?}"
    );
    // The banner is ANSI-styled, so compare on the words rather than the
    // whole line: newlines get folded out during ANSI decoding.
    assert!(
        joined.contains("Pueblo") && joined.contains("Mock") && joined.contains("MUD"),
        "expected the mock banner in scrollback, got: {joined:?}"
    );

    let _ = mock.kill();
}