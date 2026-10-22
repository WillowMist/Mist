//! Background tokio runtime + session drivers.
//!
//! `mist-ui` uses `eframe`, whose `update` callback runs on the main thread
//! outside any tokio runtime. We can't `runtime.spawn(...)` from there and
//! expect anything to happen. So:
//!
//!  - `Runtime::new()` here, plus a dedicated OS thread that calls
//!    `runtime.block_on(futures::future::pending())` so the runtime stays
//!    alive and polls every spawned task.
//!  - The egui side only sends requests to the runtime via an mpsc channel.
//!
//! This keeps the runtime hot and avoids the `tokio::spawn from non-tokio
//! thread does nothing` trap.

use mist_core::ansi::{AnsiEvent, AnsiParser};
use mist_core::telnet::TelnetStream;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;

use mist_core::markup::{Node, Parser as MarkupParser};
use crate::images::CachedImage;

/// Rendered line in a tab's scrollback.
///
/// `text` is the plain ANSI-stripped text. `markup` is the parsed MWorld
/// HTML/Pueblo-x node tree (only populated when the line carried Pueblo
/// markup). `xch_cmd` is the first anchor action, extracted eagerly when
/// the line arrives so click-handlers don't have to re-walk the AST.
#[derive(Debug, Clone)]
pub struct RenderedLine {
    pub text: String,
    pub markup: Option<Node>,
    /// First `<a xch_cmd="...">` value found in `markup`, if any.
    pub xch_cmd: Option<String>,
}

impl RenderedLine {
    pub fn from_text(s: impl Into<String>) -> Self {
        Self { text: s.into(), markup: None, xch_cmd: None }
    }

    pub fn with_markup(s: impl Into<String>, markup: Node) -> Self {
        let xch_cmd = first_xch_cmd(&markup);
        Self { text: s.into(), markup: Some(markup), xch_cmd }
    }
}

/// Walk the parsed markup tree and return the first `<a xch_cmd="...">` value
/// found, in document order.
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
                if let Some(cmd) = first_xch_cmd(c) {
                    return Some(cmd);
                }
            }
            None
        }
        Node::Font { children, .. } => {
            for c in children {
                if let Some(cmd) = first_xch_cmd(c) {
                    return Some(cmd);
                }
            }
            None
        }
        _ => None,
    }
}

/// Per-tab state held in the egui thread.
pub struct Tab {
    pub id: usize,
    pub title: String,
    pub host: String,
    pub port: u16,
    pub scrollback: Vec<RenderedLine>,
    pub input: String,
    pub status: String,
    /// Whether the runtime has parked a writer half for this tab.
    /// Set to true when the connect handler reports success.
    pub connected: bool,
}

impl Tab {
    pub fn new(id: usize, host: String, port: u16) -> Self {
        let title = format!("{host}:{port}");
        Self {
            id,
            title,
            host,
            port,
            scrollback: vec![RenderedLine::from_text("[not connected]")],
            input: String::new(),
            status: "disconnected".into(),
            connected: false,
        }
    }

    pub fn push_line(&mut self, line: RenderedLine) {
        self.scrollback.push(line);
        if self.scrollback.len() > 5_000 {
            let drop = self.scrollback.len() - 5_000;
            self.scrollback.drain(0..drop);
        }
    }
}

/// Owned writer + driver handle for one tab's connection.
/// (Stub kept for API compat — actual writer halves live in the
/// runtime's WRITERS registry.)
#[allow(dead_code)]
pub struct Connection {
    pub writer: Arc<Mutex<Option<OwnedWriteHalf>>>,
    pub driver: JoinHandle<()>,
}

/// Background runtime handle. Drop = runtime stops.
#[derive(Clone)]
pub struct RuntimeHandle {
    pub runtime: Arc<Runtime>,
    /// Channel used by the egui thread to enqueue connect requests.
    pub requests_tx: mpsc::UnboundedSender<DriverRequest>,
}

/// What the egui side can ask the driver to do.
pub enum DriverRequest {
    Connect {
        tab_id: usize,
        host: String,
        port: u16,
        /// Scrollback callback — shared map of per-tab pending lines.
        incoming: Arc<Mutex<HashMap<usize, Vec<RenderedLine>>>>,
        /// Channel back to egui to tell it the connect finished.
        result: oneshot::Sender<Result<(), String>>,
    },
    Send {
        tab_id: usize,
        line: String,
    },
    /// Drop the writer half for this tab; the driver task will exit on its
    /// next read once the socket closes.
    Disconnect {
        tab_id: usize,
    },
    /// Fetch an external image URL (from `<img src>`), decode it, and
    /// insert the resulting `TextureHandle` into the shared image cache.
    /// Errors leave a "failed" marker so the UI can stop showing the
    /// placeholder.
    FetchImage {
        url: String,
        ctx: eframe::egui::Context,
        cache: Arc<std::sync::Mutex<crate::images::CacheInner>>,
    },
}

impl RuntimeHandle {
    /// Spawn the background runtime thread and return a handle to it.
    pub fn spawn() -> Self {
        let runtime = Arc::new(Runtime::new().expect("tokio runtime"));
        let (tx, mut rx) = mpsc::unbounded_channel::<DriverRequest>();
        let rt = runtime.clone();
        std::thread::Builder::new()
            .name("pueblo-runtime".into())
            .spawn(move || {
                rt.block_on(async move {
                    while let Some(req) = rx.recv().await {
                        handle_request(req).await;
                    }
                });
            })
            .expect("spawn runtime thread");
        Self { runtime, requests_tx: tx }
    }

    pub fn request(&self, req: DriverRequest) {
        let _ = self.requests_tx.send(req);
    }
}

/// Per-tab writer halves, owned exclusively by the runtime thread. Send
/// requests look the writer up here.
static WRITERS: tokio::sync::OnceCell<Mutex<HashMap<usize, OwnedWriteHalf>>> =
    tokio::sync::OnceCell::const_new();

async fn writers_registry() -> &'static Mutex<HashMap<usize, OwnedWriteHalf>> {
    WRITERS
        .get_or_init(|| async { Mutex::new(HashMap::new()) })
        .await
}

/// Returns true if the parsed document contains anything more interesting
/// than a flat list of `Text` nodes — i.e. style/structure worth rendering.
fn has_markup(parsed: &Node) -> bool {
    match parsed {
        Node::Document { children } => children.iter().any(has_markup),
        Node::Text(_) => false,
        _ => true,
    }
}

async fn handle_request(req: DriverRequest) {
    match req {
        DriverRequest::Connect { tab_id, host, port, incoming, result } => {
            let incoming_for_line = incoming.clone();
            let on_line = move |l: RenderedLine| {
                if let Ok(mut map) = incoming_for_line.try_lock() {
                    map.entry(tab_id).or_default().push(l);
                }
            };
            let connect_result = do_connect(tab_id, host.clone(), port, on_line).await;
            match connect_result {
                Ok((writer_half, _driver)) => {
                    if let Ok(mut map) = incoming.try_lock() {
                        map.entry(tab_id).or_default().push(
                            RenderedLine::from_text(format!("[connected to {host}:{port}]")),
                        );
                    }
                    let registry = writers_registry().await;
                    let mut reg = registry.lock().await;
                    reg.insert(tab_id, writer_half);
                    let _ = result.send(Ok(()));
                }
                Err(e) => {
                    if let Ok(mut map) = incoming.try_lock() {
                        map.entry(tab_id).or_default().push(
                            RenderedLine::from_text(format!("[connect failed: {e}]")),
                        );
                    }
                    let _ = result.send(Err(format!("{e}")));
                }
            }
        }
        DriverRequest::Send { tab_id, line } => {
            let registry = writers_registry().await;
            let mut reg = registry.lock().await;
            if let Some(w) = reg.get_mut(&tab_id) {
                if let Err(e) = w.write_all(line.as_bytes()).await {
                    eprintln!("[tab {tab_id}] write error: {e}");
                }
            } else {
                eprintln!("[tab {tab_id}] send: no writer for this tab");
            }
        }
        DriverRequest::Disconnect { tab_id } => {
            let registry = writers_registry().await;
            let mut reg = registry.lock().await;
            if reg.remove(&tab_id).is_some() {
                eprintln!("[tab {tab_id}] disconnected");
            }
        }
        DriverRequest::FetchImage { url, ctx, cache } => {
            // Best-effort fetch. We deliberately do not propagate errors —
            // they're stored in the cache as a "failed" marker so the UI
            // can stop drawing the placeholder.
            let result = crate::images::fetch_and_cache(&url, &ctx).await;
            if let Ok(mut g) = cache.lock() {
                g.pending.remove(&url);
                match result {
                    Ok((size, image)) => {
                        let tex = ctx.load_texture(url.clone(), image, Default::default());
                        g.ready.insert(url.clone(), CachedImage { texture: tex, natural_size: size });
                    }
                    Err(e) => {
                        eprintln!("[img] fetch failed for {url}: {e}");
                        g.failed.insert(url.clone());
                    }
                }
            }
        }
    }
}

async fn do_connect(
    tab_id: usize,
    host: String,
    port: u16,
    on_line: impl Fn(RenderedLine) + Send + 'static,
) -> anyhow::Result<(OwnedWriteHalf, JoinHandle<()>)> {
    let stream = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
    let (read, write) = stream.into_split();
    let mut telnet = TelnetStream::new();
    let mut ansi = AnsiParser::new();
    let mut reader = BufReader::new(read);

    let driver = tokio::spawn(async move {
        let mut pending_line = String::new();
        let mut raw = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => {
                    if !pending_line.is_empty() {
                        on_line(RenderedLine::from_text(std::mem::take(&mut pending_line)));
                    }
                    break;
                }
                Ok(n) => {
                    telnet.feed(&buf[..n]);
                    let _ = telnet.drain_outgoing();

                    for ev in telnet.drain_events() {
                        if let mist_core::telnet::TelnetEvent::Data(d) = ev {
                            raw.extend_from_slice(&d);
                        }
                    }
                    if !raw.is_empty() {
                        let slice = String::from_utf8_lossy(&raw);
                        for ev in ansi.feed(&slice).unwrap_or_default() {
                            match ev {
                                AnsiEvent::Text(t) => pending_line.push_str(&t),
                                AnsiEvent::UnknownEscape(_) => {
                                    pending_line.push('\n');
                                }
                                _ => {}
                            }
                        }
                        raw.clear();
                        // Flush any complete lines (terminated by \n) from
                        // pending_line. Keep the tail (after the last \n) for
                        // the next chunk.
                        while let Some(idx) = pending_line.find('\n') {
                            let raw: String = pending_line.drain(..=idx).collect();
                            // strip trailing \r if present
                            let line = raw.trim_end_matches('\n').trim_end_matches('\r').to_string();
                            // Try parsing as Pueblo markup. Lines without any
                            // tag syntax stay as plain text (much faster path).
                            let mut parser = MarkupParser::new();
                            let rendered = match parser.parse(&line) {
                                Ok(parsed) if has_markup(&parsed) => {
                                    RenderedLine::with_markup(line, parsed)
                                }
                                _ => RenderedLine::from_text(line),
                            };
                            on_line(rendered);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[tab {tab_id}] read error: {e}");
                    break;
                }
            }
        }
    });

    Ok((write, driver))
}

// Keep the original connect() / Connection API so other modules that imported
// it continue to compile. Marked deprecated; the runtime handle above is the
// real path now.
#[allow(dead_code)]
pub async fn connect(
    _tab_id: usize,
    _host: String,
    _port: u16,
    _on_line: impl Fn(RenderedLine) + Send + 'static,
) -> anyhow::Result<Connection> {
    anyhow::bail!("use RuntimeHandle::request with DriverRequest::Connect instead")
}