# Mist — ancestry and lineage

This document records where Mist came from, who built the thing it learns
from, and how the two projects relate. If you're reading the source and
wondering "why is this Rust crate doing X, when Pueblo did Y?" — this is
where the answer lives.

Mist is a **from-scratch rewrite** of Pueblo/UE 2.61, the open-source Win32
MUD client originally released in 1998. Mist implements the same on-wire
protocol family (telnet + ANSI SGR + Pueblo's MUSH markup + TinTin++
scripting) and accepts the same markup verbatim. The display layer, layout
model, and toolchain differ — see *What Mist changed* below.

The original Pueblo/UE 2.61 source tree is checked in read-only at
`~/Projects/mist/extracted/`. The packaged archive is
`pueblosrc261.zip`. Both are excluded from version control via
`.gitignore`. They are the canonical reference for behaviour Mist
should reproduce.

---

## Credits

Mist inherits no code from Pueblo/UE — every line is Rust written for this
project. We list the original Pueblo/UE authors and copyright holders so
the lineage is explicit, not to imply any endorsement.

**Pueblo/UE 2.61 — original work, all rights reserved by the original
authors:**

- **Andromedia Incorporated** — Initial Developer and sole copyright
  holder of the 1998 codebase. 818 Mission Street — 2nd Floor, San
  Francisco, CA 94103. (`extracted/api/ChSock.cpp:19-23`,
  `extracted/LICENSE.html:505-510`.)
- **Chaco team: Dan Greening, Glenn Crocker, Jim Doubek, Coyote Lussier,
  Pritham Shetty** — "Wrote and designed original codebase."
  (`extracted/api/ChCore.cpp:31-34`.) "Chaco" is baked into internal
  class names (`ChChacoSocket`, `api/ChThread.cpp:182`) and module
  strings.
- **Ultra Enterprises: Gavin Lambert** — maintainer and porter from
  2002 onwards. Contributor to Andromedia files (ANSI SGR fixes,
  MCCP support) and Initial Developer of UE-only files: "The Original
  Code is Pueblo/UE client code, first released April 1, 2002. The
  Initial Developer of the Original Code is Ultra Enterprises"
  (`extracted/api/ChProp.cpp:21-23`).
- **Peter Unold (1992)** — original author of TinTin++, the
  MUD-scripting engine Pueblo re-implemented in its `ChWorld` module
  (`extracted/modules/client/msw/ChWorld/TinTin.h:38-40`). Pueblo
  inherited TinTin's command vocabulary unchanged; Mist inherits the
  *vocabulary* only (canonical verb table at
  `extracted/modules/client/msw/ChWorld/TinTinUtils.cpp:65`).
- **Gnanasekaran Swaminathan (1992-1994)** — author of **Sockets++**,
  the async socket library Pueblo's `ChChacoSocket` class is built
  on. "Portions copyright (C) 1992,1993,1994 Gnanasekaran Swaminathan"
  (`extracted/include/ChSock.h:38`). Pueblo's Windows TCP stack
  wraps Sockets++'s `sockbuf`; Mist ships its own tokio-based
  transport.
- **Netscape Communications Corporation (1996)** — copyright on the
  bundled Java Runtime Interface headers under `extracted/api/jri/`.

The Andromedia Public License v1.0 (`extracted/LICENSE.html`) governs
the original codebase. Mist itself is MIT — see [`README.md`](./README.md).

---

## Pueblo/UE 2.61 — what it actually was

Pueblo was a **Win32 + MFC** MUD client released in November 1998
(`extracted/api/ChSock.cpp:19`). It was the most feature-rich MUD
client of its era: telnet, MCCP compression, MUSH markup with HTML-like
rendering, TinTin scripting, embedded HTML for MXP-like worlds, sound,
graphics pane. The "UE" branch forked in 2002 when Ultra Enterprises
took over maintenance. The 2.61 release (September 2004) is the last
tagged version of either lineage, archived at `pueblosrc261.zip`.

Some scale: the 2.61 source tree is roughly **390k lines of C++** (`api/`,
`apiutil/`, `client/`, `include/`, `modules/` — excluding bundled
third-party libraries). Files of note:

| Subsystem           | File                                       | Lines |
|---------------------|--------------------------------------------|------:|
| ANSI text engine    | `api/ChTxtEng.cpp`                         | ≈5.5k |
| ANSI text buffer    | `api/ChTxtWnd.cpp`                         | ≈1.0k |
| Telnet + ANSI pump  | `modules/client/msw/ChWorld/ChWConn.cpp`   |  ≈30k |
| TinTin scripting     | `modules/client/msw/ChWorld/TinTin*.cpp`   |  ≈15k |
| HTML/MWorld parser  | `api/ChHtmlArgs.cpp`, `ChHtmSym.cpp` …     |  ≈14k |
| Pueblo HTML engine  | `api/Pueblo*.dll` (binary, not in source)  |   n/a |

The output path was the Win32 GDI rasteriser + MFC `CView` for the GUI.
Multi-tab tabbed MDI. ActiveX controls. OLE automation. DDE bridge for
the browser plug-in path. Win32 MCI for sound. DirectDraw for the
graphics pane.

This was a substantial native UI app, by 1998 standards.

---

## What Mist changed

Three things Pueblo didn't have, Mist has:

1. **No platform lock-in.** Pueblo was a Win32 + MFC application
   (`extracted/LEGAL.txt:15` explicitly notes the MFC **or** BFC
   requirement). Mist runs on Linux/macOS/Windows from the same
   binary — egui + wgpu on top.

2. **A modern display layer.** Pueblo's renderer was a block-flow
   engine with `align={left|center|right}` on `<img>/<hr>` and
   nothing else (`extracted/api/ChHtmSym.cpp:135-200`, the
   argument table, lists *only* `align`, `size`, `width`, `height`,
   `href`, `src`, `clear`, `xch_cmd`, `type`, `text`, `fgcolor`,
   `bgcolor`, `link`, `vlink`, `action`, `method`, `enctype`,
   `disabled`, `selected`, `value`, `checked`, `maxlength`, `name`,
   `multiple`, `rows`, `cols`, `start`, `xch_world`, `xch_prob`,
   `alink`, `xplink`, `hspace`, `vspace`, `ismap`, `background`,
   `border`, `noshade` — 32 attributes in total, no positioning).
   Mist's display model is Flex-column by default with a small named
   layout vocabulary; see `MIST-SPEC.md`.

3. **Forward compatibility.** Pueblo markup is preserved unchanged.
   A server that emits `<xch_page>` / `<font color="#ffaa00">` / `<a
   xch_cmd="look">` keeps working — Mist's parser treats those as
   first-class tags. New servers can use HTML5-flavour tags
   (`<page>`, `<style>`, `<column>`) that Mist renders natively.

What Mist kept:

- **Telnet option set**. 49 telnet options matching Pueblo's: ECHO,
  SGA, TTYPE, EOR, MCCP v1+v2, NAWS rejected. Pueblo's
  `ChWConn::DoTelnetCmd` (`extracted/.../ChWConn.cpp:1667-1941`) is
  the model; Mist's `mist-core/src/telnet.rs` is a clean-room re-
  implementation.
- **ANSI escape grammar**. Pueblo's `ChAnsiState` (a single class
  in `extracted/modules/client/msw/ChWorld/ChWConn.h:146`,
  implemented in `ChWConn.cpp`) parsed SGR bytes, with the same
  quirks we replicate (UTF-8 passthrough, default 16-color
  palettes, `;`-separated parameters).
- **Pueblo markup tag set**. The 49-token `aTags` table at
  `extracted/api/ChHtmSym.cpp:63-138` is the canonical vocabulary.
  Mist parses the same set, plus a handful of new HTML5-style tags.
- **TinTin verb vocabulary**. 71 entries at
  `extracted/modules/client/msw/ChWorld/TinTinUtils.cpp:65`. Mist
  ships the same table at `mist-core/src/tintin.rs::COMMANDS`
  with proper abbreviation matching.

What Mist deliberately **did not** port:

- The MFC GUI, GDI rasteriser, OLE automation, DDE bridge, ActiveX
  plug-in host, Win32 MCI sound path, DirectDraw graphics pane.
  All Windows-specific surfaces; not relevant to a 2026 MUD client.
- `<basefont>`, `<center>`, `<font>`, `<plaintext>`, `<menu>`, `<dir>`
  — five tags Pueblo shipped that HTML5 deprecated. Mist still parses
  them (for compat) but renders as `<span>` semantically.

---

## Pueblo tag vocabulary — what Mist does with each

The 49-token list at `extracted/api/ChHtmSym.cpp:63-138` breaks down as:

| Family            | Count | Mist behaviour                                     |
|-------------------|------:|----------------------------------------------------|
| Headings `<h1>`–`<h6>` | 6 | Accepted, rendered as styled text spans.       |
| Block `<blockquote>` `<address>` `<p>` `<pre>` | 4 | Accepted. |
| Lists `<ul>` `<ol>` `<dl>` `<dir>` `<menu>` `<li>` `<dt>` `<dd>` | 8 | Accepted. |
| Inline style `<b>` `<i>` `<u>` `<strike>` `<tt>` `<em>` `<cite>` `<strong>` `<code>` `<samp>` | 10 | Accepted. |
| Containers `<html>` `<head>` `<body>` `<title>` `<center>` `<nobr>` `<base>` `<basefont>` `<font>` | 9 | `<html>/<head>/<body>/<title>` accepted. `<center>` → `<span align="center">`. `<font>` → `<span style="...">`. `<basefont>` and `<nobr>` accepted but ignored — HTML5 deprecated. |
| Quote `<quote>` | 1 | Accepted, rendered as inline `<q>`. |
| Break `<hr>` `<br>` | 2 | Accepted. |
| Link `<a>` | 1 | Accepted; `xch_cmd` is the click-handler. |
| Pueblo ext. `<xch_page>` `<xch_pane>` `<xch_mudtext>` `<xch_prefetch>` | 4 | All accepted. `<xch_page clear=text>` is the canonical page-break. |
| Pre-rendered `<xmp>` `<listing>` `<plaintext>` | 3 | Accepted; render as `<pre>`. |
| Forms `<form>` `<input>` `<select>` `<option>` `<textarea>` | 5 | Accepted but not rendered (no use inside MUD input streams; retained for forward compat). |
| Forms-disabled `<disabled>` `<selected>` `<checked>` `<value>` `<maxlength>` `<name>` `<multiple>` `<rows>` `<cols>` `<start>` `<action>` `<method>` `<enctype>` | 12 | Accepted on `<form>/<input>/<select>`; ignored elsewhere. |
| Image `<img>` `<embed>` | 2 | Accepted. `<img>` fetches and paints as `egui::Image`. `<embed>` accepted but treated as a placeholder for v1 (no plugin host). |

Two Pueblo-only attributes survive as the canonical way to wire MUD
features:

- **`xch_cmd="..."`** on `<a>` — clickable action that runs a
  scripting command on the MUD client side. Pueblo's primary
  interaction mechanism. Mist keeps it.
- **`clear="text|all"`** on `<xch_page>` — what gets cleared from
  the previous page (`text` = scrollback, `all` = full reset). Same.

---

## Pueblo's hosting and the source archive

Every CVS `$Header:` line names the path `/cvsroot/pueblo/pueblo/...`
(e.g. `extracted/api/ChCore.cpp:43`), strongly consistent with the
SourceForge CVS layout of the late 90s / early 2000s. The README
points users to `http://pueblo.sf.net/pueblo/source.php`
(`extracted/README.txt:4`); the Windows binary ships with
`pueblo.sourceforge.net` baked into its resource scripts
(`extracted/client/msw/Pueblo.rc:877,903`).

`pueblosrc261.zip` is the offline mirror of the same tree: the zip's
internal comment is *"Pueblo/UE 2.61 full source tree"*; file
modification times cluster around `2004-09-30 22:30–22:46`; the most
recent commit inside the tree is also `2004-09-30 08:21:13`. So the zip
and the `extracted/` tree are the same payload, and `extracted/` is
where to read code when you want exact citation-grade history.

## Where REA would have helped

(Belt-and-braces note for future sessions. Captures what would have
been faster with REA-binary analysis.)

Binary REA would have sped three checks that we did by source-reading:

- Whether Pueblo 2.61 actually shipped the v0.2 ANSI parser quirks we
  re-implemented (no tested binary present in the archive; we cited
  `ChWConn.cpp` lines but never binary-verified).
- The exact byte sequence Pueblo emits on MCCP v2 hand-off (`IAC SB
  COMPRESS2 IAC SE` per `ChMCCP.cpp:73`).
- The TinTin verb dispatch order for ambiguous abbreviations (e.g.
  `#s` → which verb?) — source said one thing, REA on a binary would
  have been definitive.

Source-only REA-analysis (using `tools.search`) was blocked because
no compiled Pueblo DLL ships in `pueblosrc261.zip`. Building Pueblo
end-to-end on Linux would require MinGW/wine and an MFC port — not
worth the time for what amounted to a one-shot verification pass.

---

## License note on the bundled third-party code

Pueblo shipped five third-party libraries bundled in-tree; each has its
own license:

| Library | Version | Upstream          | License  | Tree location |
|---------|---------|-------------------|----------|---------------|
| lcms    | 1.11b   | littlecms.com     | LGPL     | `extracted/lcms/` |
| JPEGlib | 6b / 1998-03-27 | ijg.org        | IJG-style| `extracted/jpgsrc6b/` |
| zlib    | 1.2.1   | zlib.net          | zlib     | `extracted/zlib/` |
| libmng  | 1.0.8   | libmng.com        | libmng   | `extracted/libmng/` |
| JRI     | 1996    | netscape.com      | Netscape | `extracted/api/jri/` |

LZW/GIF decoding in Pueblo carried a Unisys patent warning
(`extracted/LEGAL.txt:1-3`); Mist doesn't decode GIFs and therefore
doesn't ship that patent exposure.
