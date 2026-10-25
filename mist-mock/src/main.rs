//! Minimal localhost mock MUD.
//!
//! Two modes:
//!
//! * `MOCK_MUD_MODE=plain`: ANSI banner + echo any line, no markup.
//!   This is the original behaviour.
//!
//! * `MOCK_MUD_MODE=pueblo` (default): emits a full Pueblo-mode intro with `<xch_page>`,
//!   `<b>`, `<i>`, `<u>`, `<font color>`, `<a xch_cmd>`, `<xch_mudtext>` and
//!   `<hr>`. The connect handshake reports the `PUEBLO` terminal type (see
//!   Pueblo/UE 2.61 `ChWConn.cpp:158` — sending `<xch_page clear=text>` once
//!   the client says `PUEBLO`).
//!
//! The mock recognises a small command set so clickable `xch_cmd` links have
//! real targets:
//!
//!   * `who`, `WHO`        → a who's-online page with action links
//!   * `score`             → a coloured score block
//!   * `look`, `l`         → redraw the starting room
//!   * `say <text>`        → reply `<You say, "text">` and a chat echo
//!   * `north`/`south`/…   → "you cannot go that way" page
//!   * `quit`              → close the connection cleanly
//!   * anything else       → `I didn't understand that.` styled response
//!
//! Listens on 127.0.0.1:4000 by default (`MOCK_MUD_PORT`).
//!
//! Run with `./mist-mock` for Pueblo markup, or
//! `MOCK_MUD_MODE=plain ./mist-mock` for the ANSI banner only.

use std::net::SocketAddr as StdSocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const IAC: u8 = 255;
const WILL: u8 = 251;
const DO: u8 = 253;
const WONT: u8 = 252;
const ECHO: u8 = 1;
const TERMINAL_TYPE: u8 = 24;
const EOR: u8 = 25;
const SB: u8 = 250;
const SE: u8 = 240;
const IS: u8 = 0;
const SEND: u8 = 1;
const NAWS: u8 = 31;
const CHARSET: u8 = 44;
const COMPRESS: u8 = 85;
const COMPRESS2: u8 = 86;
const TN3270E: u8 = 34;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let port: u16 = std::env::var("MOCK_MUD_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4000);
    // Pueblo markup is the default: it exercises far more of the client
    // (xch_page, xch_cmd anchors, <img>, font colours) than the plain
    // banner does. Set MOCK_MUD_MODE=plain for the legacy behaviour.
    let mode = std::env::var("MOCK_MUD_MODE").unwrap_or_else(|_| "pueblo".into());
    let addr: StdSocketAddr = ([127, 0, 0, 1], port).into();
    let listener = TcpListener::bind(addr).await?;
    eprintln!("mock MUD listening on {addr} (mode={mode})");
    loop {
        let (stream, peer) = listener.accept().await?;
        eprintln!("connect from {peer}");
        let mode = mode.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, mode).await {
                eprintln!("[{peer}] error: {e}");
            }
        });
    }
}

/// Write bytes to the client, compressing them when the session
/// negotiated MCCP2.
///
/// The compressed stream has to stay open for the life of the session —
/// a MUSH sends many messages and the client keeps one inflate context
/// alive — so we use a streaming `Compress` with `Z_SYNC_FLUSH` after
/// each message. `ZlibEncoder::finish()` would end the stream, which is
/// only correct for a one-shot capture.
async fn send_bytes(
    stream: &mut tokio::net::TcpStream,
    data: &[u8],
    compressor: &mut Option<flate2::Compress>,
) -> std::io::Result<()> {
    match compressor {
        Some(z) => {
            // `Compress::compress` writes into `out` but does not grow
            // it, so the buffer has to be big enough for the whole
            // message plus the zlib framing. A sync-flush adds a few
            // bytes of block overhead, hence the slack.
            let mut out = vec![0u8; data.len() + 64 * 1024];
            let before = z.total_out() as usize;
            z.compress(data, &mut out, flate2::FlushCompress::Sync)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            let produced = z.total_out() as usize - before;
            stream.write_all(&out[..produced]).await
        }
        None => stream.write_all(data).await,
    }
}

async fn handle(mut stream: tokio::net::TcpStream, mode: String) -> std::io::Result<()> {
    // Telnet handshake. This is the set wyldware.com actually sends
    // when a real client connects: WILL ECHO, DO NAWS, WILL CHARSET,
    // WILL MCCP v1+v2, plus the Pueblo-style TTYPE-equivalent (TN3270E
    // is the IBM terminal-emulation option; the MUSH uses it as a
    // "real client" signal, similar to TTYPE). The EOR (option 25) ask
    // is on the real MUSH too; we keep it for compatibility.
    stream.write_all(&[IAC, WILL, ECHO]).await?;
    stream.write_all(&[IAC, DO, NAWS]).await?;
    // Offer MCCP v2. The client answers DO COMPRESS2; if it does, the
    // rest of the session is zlib-compressed. This is the same dance a
    // real MUSH performs, and it is the only way to exercise the
    // client's decompressor.
    stream.write_all(&[IAC, WILL, COMPRESS2]).await?;

    // Wait briefly for the client to answer DO COMPRESS2 before we
    // start sending data, so the negotiation completes cleanly.
    //
    // The client's reply can arrive split across reads, so we keep a
    // small buffer of everything we've seen and scan it for the
    // `IAC DO COMPRESS2` sequence.
    let mut compress = false;
    {
        use tokio::time::{timeout, Duration};
        // `seen` is a rolling window of the client's bytes; `chunk` is
        // the scratch buffer we read into. A `Vec` with zero length
        // would make `read` return 0 bytes every time, so the scratch
        // buffer has to be a real slice.
        let mut seen: Vec<u8> = Vec::with_capacity(64);
        let mut chunk = [0u8; 64];
        let deadline = Duration::from_millis(800);
        let t0 = std::time::Instant::now();
        while t0.elapsed() < deadline {
            let remaining = deadline.saturating_sub(t0.elapsed());
            match timeout(remaining, stream.read(&mut chunk)).await {
                Ok(Ok(n)) if n > 0 => {
                    seen.extend_from_slice(&chunk[..n]);
                    // Look for IAC DO COMPRESS2 (255 253 86).
                    if seen.windows(3).any(|w| w == [IAC, DO, COMPRESS2]) {
                        compress = true;
                        break;
                    }
                    // Keep the tail so a sequence split across two
                    // reads is still detected.
                    if seen.len() > 3 {
                        let start = seen.len() - 3;
                        seen.drain(..start);
                    }
                }
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    }

    if compress {
        // MCCP v2 start sequence, then everything is compressed.
        stream.write_all(&[IAC, SB, COMPRESS2, IAC, SE]).await?;
        eprintln!("client accepted MCCP2; compressing the session");
    }

    // A streaming compressor for the session. `Compress::new(true)`
    // selects a zlib stream, which is what MCCP uses.
    let mut compressor = if compress {
        Some(flate2::Compress::new(flate2::Compression::default(), true))
    } else {
        None
    };

    if mode == "pueblo" {
        // Pueblo-mode intro: announce a banner in markup, signal the
        // Pueblox protocol, and drop a fresh <xch_page>.
        let intro = PUEBLO_INTRO.as_bytes();
        send_bytes(&mut stream, intro, &mut compressor).await?;
    } else {
        let banner = b"\x1B[1;36m== Pueblo Mock MUD ==\x1B[0m\r\n\
                       \x1B[33mType anything; we'll echo it back.\x1B[0m\r\n\
                       \x1B[32m> \x1B[0m";
        send_bytes(&mut stream, banner, &mut compressor).await?;
    }

    let mut buf = [0u8; 1024];
    let mut line = String::new();
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        if n >= 3 && buf[0] == IAC && buf[1] == DO && buf[2] == EOR {
            stream.write_all(&[IAC, WILL, EOR]).await?;
        }
        let mut i = 0;
        while i < n {
            let b = buf[i];
            if b == b'\n' || b == b'\r' {
                if !line.is_empty() {
                    let cmd = line.trim().to_string();
                    line.clear();
                    if cmd == "quit" {
                        send_bytes(
                            &mut stream,
                            b"\x1B[36mGoodbye.\x1B[0m\r\n",
                            &mut compressor,
                        )
                        .await?;
                        stream.shutdown().await?;
                        return Ok(());
                    }
                    if mode == "pueblo" {
                        let response = pueblo_response(&cmd);
                        send_bytes(&mut stream, response.as_bytes(), &mut compressor).await?;
                    } else {
                        let reply = format!(
                            "\x1B[36m< \x1B[0m\x1B[1m{}\x1B[0m\r\n\x1B[32m> \x1B[0m",
                            cmd
                        );
                        // The reply goes through the same compressor as
                        // the banner — once the session is compressed,
                        // every byte after the MCCP start sequence is
                        // zlib data.
                        send_bytes(&mut stream, reply.as_bytes(), &mut compressor).await?;
                    }
                }
                i += 1;
            } else if b == IAC && i + 2 < n {
                // Skip telnet IAC + cmd + option. Don't drop into line buffer.
                i += 3;
            } else if b >= 0x20 {
                line.push(b as char);
                i += 1;
            } else {
                i += 1;
            }
        }
    }
}

// ---------- Pueblo-mode content ----------

const PUEBLO_INTRO: &str = "\
<xch_mudtext>\
\x1B[1;36mPueblo Mock MUSH\x1B[0m\r\n\
\x1B[33mA small test bench for the Pueblo/UE rewrite.\x1B[0m\r\n\
\x1B[32m> \x1B[0m\
</xch_mudtext>\
<xch_page clear=text>\
<hr>\
<b>Pueblo Mock MUSH</b> — <i>village square</i>\
<hr>\r\n\
\r\n\
A small stone square sits at the centre of a quiet village. Cobblestones \
radiate outward under your <font color=\"#ffaa40\">\x1b[38;5;215mboots\x1b[0m</font>. \
Lanterns flicker against the early-evening air.\r\n\
\r\n\
<img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAUAAAABQCAIAAADnUzvSAAAE6klEQVR42u3TdVfVBxyAcf+fMkd3X7r70t1c8gKX7hZERVEpsVFUFBFEQQzEQBFBFMVtrjtZu81trl/Evr8/fQmcPec8r+A557NOk6gmojXaOhYQrXnAqxoiWku9ANjU8CUzyWi9udF6C2Npg6XJBivJVM/aVM/GTM/GXM/W/GU7CyV7y40OktVGR6tXVNZKTjb6zpKtvoudvqudgau9gZu9gbuDoeThaOjpaOSpMvJSGXk7GUs+zsa+LpKJn6uJv+RmGuBmGuhuGuRuFuRhFuxhFuJpLqm9zEO9JYswH4twydcywtcy0k+yivK3ipYCrGMCrGMDrWODrOOCbOKDlRJCbBMltW2S2i45VCklzD5VCrdPi7BPj3BIj3TIiHTQRDlKmdGOWdGqrBhVdowqJ9ZJyo1zyot30sY7axOc8xOcCxJdpMIkF12Sqy7ZtSjZtTjFTSpJdStNk9zL0t3LpQyPigyPSo3kWZXpWS1ledVke9VKOV51Od71uUoNeT6NktanSevbnK/UUuC3qVCpVefXpvNvK/LfXOTfXhwgbSkJ2FoSuK1UqaMsaHu50o6KoM6K4M7K4J2VwbuqQqTd1SFd1equGnV3jbqnNrSnLrS3LrSvXgrb0xDWLzWG720M39ckRexvjjggtUQc3BR5SGqNPNwaNdCmdGRz9FGpPXqwPebYFqXjW2NObIuVhjpiT3bEndquNLwj/rTUGT/SmXBmp9LoroSx3YnS2a7E8a6k8e6kc91J53uSpYne5MnelMm+lAt9KVN7Uqb6Uy/2p17aK6Vd3pd2RdqfPr0//eoBKWPmYMa1Q0rXD2tuSAOamwOZs0eUbh3Nui0NZs0NZt85lj1/XOnuiZwFaShncSj33kmlpVN596XhvAfD2uXT2uUR7cMR7aMz+dLKaP7j0YLHYwWvjhW8drZQen288Mm47sk53RvndW9KE0VvTRS9PSkVv3Oh+F1pquS9qZL3L0qlH1wq/fCy0kdXyj6Wpss+mS7/9Gr5Z9JMxeczFV9ckyq/vF65ekPpq5tVX0uzVd/MVn97S+m72zXfS3M1P8zVPr1T+3S+9sf52p/u1kk/L9Q9W6h/tlj/y2L9r/capN+WGp4vNT6/3/j7g8Y/pOWmP5eb/nooNf/9qPkfaaXl35UWAAMYwAAGMIABDGAAAxjAAAYwgAEMYAADGMAABjCAAQxgAAMYwAAGMIABDGAAAxjAAAYwgAEMYAADGMAABjCAAQxgAAMYwAAGMIABDGAAAxjAAAYwgAEMYAADGMAABjCAAQxgAAMYwAAGMIABDGAAAxjAAAYwgAEMYAADGMAABjCAAQxgAAMYwAAGMIABDGAAAxjAAAYwgAEMYAADGMAABjCAAQxgAAMYwAAGMIABDGAAAxjAAAYwgAEMYAADGMAABjCAAQxgAAMYwAAGMIABDGAAAxjAAAYwgAEMYAADGMAABjCAAQxgAAMYwAAGMIABDGAAAxjAAAYwgAEMYAADGMAABjCAAQxgAAMYwAAGMIABDGAAAxjAAAYwgAEMYAADGMAABjCAAQxgAAMYwAAGMIABDGAAAxjAAAYwgAEMYAADGMAABjCAAQxgAAMYwAAGMIABDGAAAxjAAAYwgAEMYAADGMAABjCAAQxgAAMYwAAGMIAB/H8FvKohorXUC4CJaC0GYKI13H+S/j1AduYWkgAAAABJRU5ErkJggg==\" alt=\"a generated gradient banner\" width=\"320\">\r\n\
\r\n\
<b>You see:</b>\r\n\
&nbsp;* A <a xch_cmd=\"look sign\">weathered wooden sign</a>\r\n\
&nbsp;* A <a xch_cmd=\"look well\">stone well</a> in the southeast corner\r\n\
&nbsp;* <a xch_cmd=\"who\">Other players online</a>\r\n\
&nbsp;* <a xch_cmd=\"score\">Your character sheet</a>\r\n\
\r\n\
<u>Exits:</u> <a xch_cmd=\"north\">north</a>, <a xch_cmd=\"south\">south</a>, \
<a xch_cmd=\"east\">east</a>, <a xch_cmd=\"west\">west</a>.\r\n\
";

/// Dispatch a single command to a markup-rich response.
fn pueblo_response(cmd: &str) -> String {
    let lower = cmd.trim().to_ascii_lowercase();
    match lower.as_str() {
        "who" => who_page(),
        "score" => score_block(),
        "look" | "l" => PUEBLO_INTRO.to_string(),
        c if c.starts_with("look ") => look_target(&c[5..].trim()),
        "north" | "south" | "east" | "west" => no_exit(&lower),
        c if c.starts_with("say ") => say_page(&c[4..]),
        _ => i_didnt_understand(&cmd),
    }
}

fn page_break_open() -> &'static str {
    "<xch_page clear=text>"
}

/// Convert a hex colour to an xterm 256-colour index and emit the SGR
/// open/close around the supplied text. Used to interleave ANSI colour
/// codes with the Pueblo markup so terminal clients without their own
/// HTML renderer still paint in colour.
fn colorize(hex: &str, text: &str) -> String {
    let s = hex.trim_start_matches('#');
    if s.len() != 6 {
        return text.to_string();
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok();
    let g = u8::from_str_radix(&s[2..4], 16).ok();
    let b = u8::from_str_radix(&s[4..6], 16).ok();
    let (r, g, b) = match (r, g, b) {
        (Some(r), Some(g), Some(b)) => (r, g, b),
        _ => return text.to_string(),
    };
    let ri = (r as usize * 6) / 256;
    let gi = (g as usize * 6) / 256;
    let bi = (b as usize * 6) / 256;
    let cube = 16 + 36 * ri + 6 * gi + bi;
    format!("\x1b[38;5;{}m{}\x1b[0m", cube, text)
}

fn who_page() -> String {
    format!(
        "{open}<b>Players online</b><hr>\r\n\
         \r\n\
         &nbsp;* <font color=\"#80ff80\">\x1b[38;5;114mWillow\x1b[0m</font> — <i>idle</i> (<a xch_cmd=\"page willow\">page</a>)\r\n\
         &nbsp;* <font color=\"#80c0ff\">\x1b[38;5;111mEve\x1b[0m</font> — <i>editing</i> (<a xch_cmd=\"page eve\">page</a>)\r\n\
         &nbsp;* <font color=\"#ff8080\">\x1b[38;5;210mCerin\x1b[0m</font> — <i>typing</i> (<a xch_cmd=\"page cerin\">page</a>)\r\n\
         \r\n\
         <a xch_cmd=\"look\">return to the village square</a>",
        open = page_break_open()
    )
}

fn score_block() -> String {
    format!(
        "{open}<b>Character sheet</b><hr>\r\n\
         \r\n\
         <b>Name:</b>       <font color=\"#ffaa40\">\x1b[38;5;215mWillow\x1b[0m</font>\r\n\
         <b>Race:</b>       <i>human</i>\r\n\
         <b>Class:</b>      <font color=\"#80c0ff\">\x1b[38;5;111mscout\x1b[0m</font>\r\n\
         \r\n\
         <b>HP</b>   <font color=\"#ff6060\">\x1b[38;5;203m42 / 60\x1b[0m</font>\r\n\
         <b>Mana</b> <font color=\"#6080ff\">\x1b[38;5;69m31 / 50\x1b[0m</font>\r\n\
         <b>Move</b> <font color=\"#80ff80\">\x1b[38;5;114m120 / 120\x1b[0m</font>\r\n\
         \r\n\
         <u>Stats</u>\r\n\
         &nbsp;* STR <font color=\"#ffaa40\">\x1b[38;5;215m14\x1b[0m</font>\r\n\
         &nbsp;* DEX <font color=\"#80ff80\">\x1b[38;5;157m16\x1b[0m</font>\r\n\
         &nbsp;* CON <font color=\"#ffaa40\">\x1b[38;5;215m13\x1b[0m</font>\r\n\
         &nbsp;* INT <font color=\"#80c0ff\">\x1b[38;5;153m12\x1b[0m</font>\r\n\
         \r\n\
         <a xch_cmd=\"look\">return to the village square</a>",
        open = page_break_open()
    )
}

fn look_target(what: &str) -> String {
    let what = what.trim();
    format!(
        "{open}<b>You look closely at the {what}.</b><hr>\r\n\
         \r\n\
         <font color=\"#c0c0c0\">\x1b[38;5;145m\
         Whatever it is, it has been here longer than you have. The wind \
         hums through it; nothing moves. You could probably <a xch_cmd=\"examine {what}\">\
         examine it more carefully</a>, but it doesn't seem to lead anywhere.\
         \x1b[0m</font>\r\n\
         \r\n\
         <a xch_cmd=\"look\">return to the village square</a>",
        open = page_break_open()
    )
}

fn no_exit(direction: &str) -> String {
    format!(
        "{open}<b>You cannot go {direction} from here.</b>\r\n\
         <a xch_cmd=\"look\">back to the village square</a>",
        open = page_break_open()
    )
}

fn say_page(text: &str) -> String {
    format!(
        "{open}\
         <i><font color=\"#ffd060\">\x1b[38;5;222mYou say, \"{text}\"\x1b[0m</font></i>\r\n\
         <a xch_cmd=\"look\">return to the village square</a>",
        open = page_break_open()
    )
}

fn i_didnt_understand(raw: &str) -> String {
    format!(
        "{open}<font color=\"#ff8080\">\x1b[38;5;217mI didn't understand that.\x1b[0m</font><hr>\r\n\
         You typed: <tt><b>{raw}</b></tt>\r\n\
         Try <a xch_cmd=\"look\">look</a>, <a xch_cmd=\"who\">who</a>, \
         <a xch_cmd=\"score\">score</a>, or <a xch_cmd=\"say hello\">say hello</a>.\r\n\
         \r\n\
         <a xch_cmd=\"look\">return to the village square</a>",
        open = page_break_open()
    )
}
