//! Minimal localhost mock MUD.
//!
//! Two modes:
//!
//! * `MOCK_MUD_MODE=plain` (default): ANSI banner + echo any line, no markup.
//!   This is the original behaviour.
//!
//! * `MOCK_MUD_MODE=pueblo`: emits a full Pueblo-mode intro with `<xch_page>`,
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
//! Run with `mist-mock` for plain ANSI banner, or
//! `MOCK_MUD_MODE=pueblo ./mist-mock` for markup streams.

use std::net::SocketAddr as StdSocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const IAC: u8 = 255;
const WILL: u8 = 251;
const DO: u8 = 253;
const ECHO: u8 = 1;
const TERMINAL_TYPE: u8 = 24;
const EOR: u8 = 25;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let port: u16 = std::env::var("MOCK_MUD_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4000);
    let mode = std::env::var("MOCK_MUD_MODE").unwrap_or_else(|_| "plain".into());
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

async fn handle(mut stream: tokio::net::TcpStream, mode: String) -> std::io::Result<()> {
    // Telnet handshake so the client state machine has something to react to.
    stream.write_all(&[IAC, WILL, ECHO]).await?;
    stream.write_all(&[IAC, DO, TERMINAL_TYPE]).await?;
    stream.write_all(&[IAC, DO, EOR]).await?;

    if mode == "pueblo" {
        // Pueblo-mode intro: announce a banner in markup, signal the
        // Pueblox protocol, and drop a fresh <xch_page>.
        let intro = PUEBLO_INTRO.as_bytes();
        stream.write_all(intro).await?;
    } else {
        let banner = b"\x1B[1;36m== Pueblo Mock MUD ==\x1B[0m\r\n\
                       \x1B[33mType anything; we'll echo it back.\x1B[0m\r\n\
                       \x1B[32m> \x1B[0m";
        stream.write_all(banner).await?;
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
                        stream.write_all(b"\x1B[36mGoodbye.\x1B[0m\r\n").await?;
                        stream.shutdown().await?;
                        return Ok(());
                    }
                    if mode == "pueblo" {
                        let response = pueblo_response(&cmd);
                        stream.write_all(response.as_bytes()).await?;
                    } else {
                        let reply = format!(
                            "\x1B[36m< \x1B[0m\x1B[1m{}\x1B[0m\r\n\x1B[32m> \x1B[0m",
                            cmd
                        );
                        stream.write_all(reply.as_bytes()).await?;
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
radiate outward under your <font color=\"#ffaa40\">boots</font>. \
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

fn who_page() -> String {
    format!(
        "{open}<b>Players online</b><hr>\r\n\
         \r\n\
         &nbsp;* <font color=\"#80ff80\">Willow</font> — <i>idle</i> (<a xch_cmd=\"page willow\">page</a>)\r\n\
         &nbsp;* <font color=\"#80c0ff\">Eve</font> — <i>editing</i> (<a xch_cmd=\"page eve\">page</a>)\r\n\
         &nbsp;* <font color=\"#ff8080\">Cerin</font> — <i>typing</i> (<a xch_cmd=\"page cerin\">page</a>)\r\n\
         \r\n\
         <a xch_cmd=\"look\">return to the village square</a>",
        open = page_break_open()
    )
}

fn score_block() -> String {
    format!(
        "{open}<b>Character sheet</b><hr>\r\n\
         \r\n\
         <b>Name:</b>       <font color=\"#ffaa40\">Willow</font>\r\n\
         <b>Race:</b>       <i>human</i>\r\n\
         <b>Class:</b>      <font color=\"#80c0ff\">scout</font>\r\n\
         \r\n\
         <b>HP</b>   <font color=\"#ff6060\">42 / 60</font>\r\n\
         <b>Mana</b> <font color=\"#6080ff\">31 / 50</font>\r\n\
         <b>Move</b> <font color=\"#80ff80\">120 / 120</font>\r\n\
         \r\n\
         <u>Stats</u>\r\n\
         &nbsp;* STR <font color=\"#ffaa40\">14</font>\r\n\
         &nbsp;* DEX <font color=\"#80ff80\">16</font>\r\n\
         &nbsp;* CON <font color=\"#ffaa40\">13</font>\r\n\
         &nbsp;* INT <font color=\"#80c0ff\">12</font>\r\n\
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
         <font color=\"#c0c0c0\">\
         Whatever it is, it has been here longer than you have. The wind \
         hums through it; nothing moves. You could probably <a xch_cmd=\"examine {what}\">\
         examine it more carefully</a>, but it doesn't seem to lead anywhere.\
         </font>\r\n\
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
         <i><font color=\"#ffd060\">You say, \"{text}\"</font></i>\r\n\
         <a xch_cmd=\"look\">return to the village square</a>",
        open = page_break_open()
    )
}

fn i_didnt_understand(raw: &str) -> String {
    format!(
        "{open}<font color=\"#ff8080\">I didn't understand that.</font><hr>\r\n\
         You typed: <tt><b>{raw}</b></tt>\r\n\
         Try <a xch_cmd=\"look\">look</a>, <a xch_cmd=\"who\">who</a>, \
         <a xch_cmd=\"score\">score</a>, or <a xch_cmd=\"say hello\">say hello</a>.\r\n\
         \r\n\
         <a xch_cmd=\"look\">return to the village square</a>",
        open = page_break_open()
    )
}
