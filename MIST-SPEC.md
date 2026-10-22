# Mist — Design Specification

This is the working design spec for Mist. Sections 1–4 below define
the **sharp** half of the spec — the parts we cut code against every
day. Sections 5–12 are forward-looking (composability, security,
migration story). When in doubt, prefer the line that says **"sharp"**
over the line that says **"future"** — sharp wins.

The document is per-area authoritative: if code disagrees with this
spec, **the spec is right** and the code is wrong. Update this file
in the same commit that changes the behaviour.

---

## 1. Document model

A Mist stream is **UTF-8** text with embedded markup. There is exactly
one document stream per MUD connection. The stream alternates plain
text (anything not in a tag), Pueblo-style markup, and well-known
byte-level escapes:

1. **ANSI SGR** sequences (`ESC[...m`) are decoded *before* markup
   parsing. They produce **inline style** on the following text, never
   *markup nodes*. Pueblo's `ChAnsiState` (`extracted/modules/client/
   msw/ChWorld/ChWConn.h:146`) is the reference: same SGR byte
   grammar, same quirks.
2. **Other ANSI** sequences (`ESC[2J` clear, `ESC[s`/`ESC[u` save/restore,
   etc.) become *markup events* in the document stream — not direct
   paint operations. A `CSI n J` is rewritten into `<page clear={n==0?"all":...}>`.
3. **Pueblo markup** is parsed next, paragraph by paragraph (paragraph
   = text bracketed by `<xch_page>` or two newlines).
4. **Markup tags** are the only structural element. There are no script
   tags, no style tags-as-strings. Layout attributes are parsed from
   the tag string.

### Default layout algorithm

Block-flow on the Y axis (top-to-bottom). Each block:

1. If it contains a `layout=flex-column` attribute (or is `<row>` /
   `<column>` / `<grid>`): layout its children with the matching
   flex-style algorithm.
2. Otherwise: lay children out top-to-bottom like CSS `display: block`.

Width is determined by container. Height is content height plus
declared `pad` (per-side optional).

### Reserved `<mist-*>` element namespace

Server authors **must not** use elements whose names start with `mist:`.
That's Mist's extension surface. MUD servers writing markup today don't
need to; this is reserved for forward compatibility.

---

## 2. Tag vocabulary

Mist recognises the **Pueblo 49** plus a small HTML5-flavour set. Tags
fall into three categories:

| Mode   | What happens when present                                |
|--------|---------------------------------------------------------|
| Render | Tag is parsed and produces paint in the document.       |
| Accept | Tag is parsed and produces *structural* effect (insert empty placeholder, consume child slot). |
| Ignore | Tag is consumed and dropped; no paint, no children.    |

Pueblo's tag table (`extracted/api/ChHtmSym.cpp:63-138`) is the canonical
reference. Mapping:

| Pueblo tag                  | Mist mode  | Notes                                             |
|-----------------------------|------------|---------------------------------------------------|
| `<h1>`–`<h6>`               | Render     | Stylised text span by font weight + size.        |
| `<p>`                       | Render     | Paragraph break + spacing.                       |
| `<blockquote>` `<address>`  | Render     | Indented text block.                              |
| `<pre>` `<xmp>` `<listing>` `<plaintext>` | Render | Monospace block; markup inside is escaped. |
| `<ul>` `<ol>` `<dl>` `<dir>` `<menu>` | Render | `<dir>` and `<menu>` are deprecated aliases of `<ul>`. |
| `<li>` `<dt>` `<dd>`        | Render     | Inside their list containers.                    |
| `<b>` `<i>` `<u>` `<strike>` `<em>` `<strong>` `<cite>` | Render | Inline style; `<em>`/`<strong>`/`<cite>` may style differently by font fallback. |
| `<tt>` `<code>` `<samp>`    | Render     | Monospace inline style.                          |
| `<a href=… xch_cmd=…>`       | Render     | `<a>` is the only clickable element. `href` opens in browser when clicked; `xch_cmd` fires into the TinTin scripting runtime. |
| `<xch_page clear=…>`        | Render     | Page break. `clear={text,all}`.                  |
| `<xch_pane>` `<xch_mudtext>` `<xch_prefetch>` | Render | `<xch_pane>` reserved; `<xch_prefetch>` fires async URL fetches; `<xch_mudtext>` switches back to monospace. |
| `<img src=… alt=… [width=…] [height=…]>` | Render | Fetches the URL on first paint; cached. `width`/`height` are pixel hints. |
| `<br>` `<hr>`               | Render     | Line break; horizontal rule.                     |
| `<center>` `<basefont>` `<font>` `<nobr>` `<base>` | Render | `<font color=… size=…>` re-formatted as inline style; `<basefont>` parsed but ignored. |
| `<head>` `<body>` `<html>` `<title>` | Accept | Carried for HTML compat; renderer produces nothing. |
| `<form>` `<input>` `<select>` `<option>` `<textarea>` | Ignore | Form elements don't carry MUD-client state. |
| `<embed>`                   | Accept     | Parsed as a placeholder rect ("embed: $alt").    |
| `<mist-*>`                  | Render     | Reserved Mist extension.                          |
| Anything else              | Ignore     | `<unknown>` placeholder in dev builds; silent in release. |

`<a xch_cmd=>` and `<a href=>` are **both supported** because different
MUDs use different conventions. Render emits an underlined link in
both cases; the click handler picks the right action.

### MXP tags (forward-looking)

MXP `<SEND>`, `<A HINT>`, `<A HREF>` — accepted in secure-mode-marked
streams (`<mist:mxp-secure flag>`). v0.x scope: parse them, dispatch
into TinTin like `xch_cmd`.

### HTML5 flavours Mist accepts

These are server-emittable on Mist-aware MUDs:

`<row>`, `<column>` (`<column>` is `<column layout=flex-column gap=8>`).
`<card>` (a `<column>` with a 1-px border and 8-px padding).
`<pad top=N right=N bottom=N left=N>` (shorthand for inline spacing).
`<line height=N>` and `<line align={left|center|right}>`.
`<stack>` (vertical flex-column with no gap).
`<grid columns=N gap=N>` (CSS-grid-like).

These are 100% HTML-flavour names but they don't require CSS. A future
spec revision can extend the set without breaking older markup.

---

## 3. Attribute vocabulary

**Atomic-valued attributes** (default if absent):

| Attribute   | Type                    | Effect                                                   |
|-------------|-------------------------|---------------------------------------------------------|
| `id`        | `Identifier` (string)  | Internal handle, used by `<mist:dialog open=…>` etc.    |
| `class`     | Space-separated tokens  | Reserved for future CSS classes.                       |
| `style`     | String                  | **Not parsed.** Carried verbatim; renderer ships style strings out of band. |
| `data-*`    | String                  | **Not parsed.** Server-emitted attachments.            |

**Layout attributes**:

| Attribute   | Type                                | Effect                                     |
|-------------|-------------------------------------|--------------------------------------------|
| `width`     | `Number` (pixels)                   | Element width.                             |
| `height`    | `Number` (pixels)                   | Element height.                            |
| `min-/max-` | Number                              | Constraint on width or height.             |
| `pad`       | `Number` (CSS-like: `<one\|two\|four>` values, all-pixel) | Inner padding, single value or per-side. |
| `gap`       | `Number`                            | Flex container inter-item gap.             |
| `align`     | `Key from {left, center, right, justify}` | Horizontal alignment of children.    |
| `valign`    | `Key from {top, middle, bottom, baseline}` | Vertical alignment.                 |
| `wrap`      | `Key from {none, yes}`              | Disable word-wrap (default yes).           |
| `hidden`    | Boolean                             | Element is laid out but not painted.       |

**Style attributes** (each accepts a small set of named values):

| Attribute           | Values                                     | Default    |
|---------------------|--------------------------------------------|------------|
| `bg`                | `<#hex> \| transparent`                  | transparent |
| `fg`                | `<#hex>`                                  | theme fg  |
| `bold`              | `true \| false`                           | inherit   |
| `italic`            | `true \| false`                           | inherit   |
| `underline`         | `true \| false`                           | inherit   |
| `strike`            | `true \| false`                           | inherit   |
| `family`            | `mono \| sans \| serif`                   | inherit   |
| `size`              | `<px>` or `<em>`                          | inherit   |
| `radius`            | `<px>`                                    | 0          |
| `border`            | `<px> \| <px> <style> \| <px> <style> <hex>` | none      |
| `shadow`            | `<x> <y> <blur> <hex>`                  | none       |

The renderer **never** parses CSS. A `style="…"` attribute is stored
verbatim and used only by name; layouts use the **element attributes**
(`bg`, `width`, etc.) above.

**Interaction attributes**:

| Attribute           | Effect                                                                  |
|---------------------|-------------------------------------------------------------------------|
| `href`              | URL clicked → open in OS-default browser.                              |
| `xch_cmd`           | String clicked → dispatched as a user-typed command.                  |
| `action`            | v0.4: bind to a `<mist:dialog>` or `<mist:panel>` open action.        |
| `data-mist-command` | v0.4: arbitrary identifier resolvable to a TinTin command (for client-defined macros). |

**Pueblo-specific attributes** (kept for backward compatibility):

| Attribute     | Effect on tags                         |
|---------------|----------------------------------------|
| `clear=text\|all` | on `<xch_page>`                  |
| `href=…` and `xch_cmd=…` | both recognised on `<a>` |
| `src=…` `alt=…` | on `<img>`                       |
| `align`        | on `<img>` and `<hr>` (legacy)        |

---

## 4. Style attribute DSL (the small one)

The attribute grammar is:

```
style-attr  ::= ident '=' value
             | ident '(' value ')'       # reserved for future functions
value       ::= word                     # keyword values
             | '#' hex{6,8}             # color
             | number ('px' | 'em')?    # length
             | '"' inner '"'           # quoted string
             | '(' value-list ')'       # tuple (e.g. shadow)
word        ::= [a-zA-Z_-][a-zA-Z0-9_-]*
```

There is exactly one parser: `mist_core::style::parse_attr("bg=#ffaa00 pad=4")`
returns `[(Attr::Bg, "#ffaa00"), (Attr::Pad(N), 4)]`. No expressions, no
calculations, no `calc()`. The renderer reads the parsed map.

The grammar is a **strict subset** of CSS syntax; future versions may
adopt more CSS keywords but never variable substitution. Anyone wanting
Turing-completeness in styling should use a real CSS parser, not this.

---

## 5. Composability

Server-side `<mist:include src="…">` (v0.4) and `<mist:tile name="…">` (v0.5)
introduce shared content across pages. Both require a security model
(see §7). For now: no includes, the document is exactly one stream.

## 6. MIME / encoding

UTF-8 only on the wire. ANSI SGR is decoded first. MXP secure-mode
streams are marked by `<mist:mxp-secure>` at the document root or by
a telnet WILL TTYPE "PUEBLO-MXP" handshake (whichever comes first).

Locale: text rendering uses the user-selected monospace font loaded at
startup. Layout direction defaults to LTR; RTL via `dir="rtl"` (v0.5).

## 7. Security

`<a href>` opens a URL in the OS-default browser. **We must** warn the
user before following third-party URLs out of a MUD; MUDs are casual-
privacy territory and a hostile server could embed `href="…"` to phishing
URLs. Default: prompt the user before opening (v0.4).

`<xch_cmd>` is text typed at the user; never executed without a confirm
in v0.4. Today's behaviour sends immediately, which is fine for trusted
local mocks but unsafe on the open internet.

## 8. Migration story

Mist's parser accepts the Pueblo 49 verbatim. Tags in the
`pueblo-deprecated` family (see table in §2) still parse and render, just
through different element-attribute paths.

`REYNARDS-NEEDED`: a translator option (off by default) that, given a
`<font color="#ffaa00">` block, emits `<span fg="#ffaa00">` and warns the
emitter on stderr. v0.4 work.

## 9. Wire format

Same telnet stack Pueblo used; the markup is carried on the byte
stream after telnet and ANSI decode. The document opens with
`<meta name="version" content="mist-X.Y">` so the client knows which
profile to apply. Server emulators in the binary should include this
meta tag; older Pueblo servers that don't still render fine — the
client assumes default profile in that case.

## 10. Backward compatibility matrix

(Tracked in [`MIST-ANCESTRY.md`](./MIST-ANCESTRY.md) Tag-lineage table.
This is the broad summary; see the doc for fine detail.)

| Server emits    | Mist renders correctly | Notes                                  |
|-----------------|-------------------------|----------------------------------------|
| Pueblo 1.x markup | ✅                       | All 49 Pueblo tags parse.             |
| ANSI-only         | ✅                       | SGR colors, no markup tags.           |
| MXP (secure)      | partial                  | `<SEND>`, `<A>` work; secure-mode = v0.4. |
| HTML5-flavour Mist | ✅                     | This spec.                              |

## 11. Profile levels

Three, picked automatically by the client when it sees a `<meta>` tag
or via the telnet TTYPE handshake:

- **`mist-1`** strict superset. Server authored against this spec.
  All HTML5-flavour tags recognised.
- **`mist-2`** permissive. Accepts Pueblo 49 verbatim, including
  deprecated 5. `style="…"` is parsed leniently but ignored.
- **`mist-3`** legacy. Maps deprecated Pueblo tags to their modern
  equivalents in the renderer. `<basefont>` becomes `<span>` with
  the same `color`/`size`; `<center>` becomes `<line align="center">`.

If `<meta name="version">` is missing or empty, the client selects
`mist-2` (the most permissive).

## 12. Reserved extensions

Namespaced `mist-*` (or `<mist:*>`) tags are reserved for future work.
Today no tag in that namespace is *defined* — the renderer treats them
as opaque placeholders — but they're the future extension surface:

```
mist:dialog       — popup dialogs (modal / non-modal)
mist:panel        — drawer / slide-out side panel
mist:menu         — dropdown or context menu
mist:command-palette  — quick-command launcher overlay
mist:tab          — nested tabs inside a scrollback
mist:file-tree    — embedded file explorer for code-sharing MUDs
mist:image-grid   — grid layout for image galleries
mist:audio        — audio playback with playback controls
mist:gl           — WebGL canvas, used by 3D-MUD worlds
```

Until each lands, renderers should *accept* and *ignore* these tags so
early-adopter servers don't break.

---

## 13. Persistence and world-list collections (roadmap)

Saved sessions and a world-list collection are deferred to **v0.5**.
The contract is:

- **`mist-core::profile::Config`** stores a list of `SavedSession {
  name, host, port, last_connect }` and a list of `WorldListEntry`
  on disk under the platform config dir (`directories::ProjectDirs::
  from("rs", "Mist", "Mist")`). Default path becomes
  `~/.config/mist/config.toml` (Linux) /
  `~/Library/Application Support/rs.Mist/config.toml` (macOS).
- **Toolbar "Worlds" button** opens a side panel listing entries; clicking
  one opens a new tab pre-filled with the entry's host/port.
- **Toolbar "Save session" action** (right-click on a tab) adds the
  active tab's connection to the saved sessions.

Storage format is TOML. The migration path from the current empty
profile file is a v0.5 task.

### Namespace decision (locked 2026-10-22)

Wyldware is the publisher of record. The application triple passed to
`directories::ProjectDirs::from` is:

```rust
ProjectDirs::from("com.wyldware", "Wyldware", "mist")
```

Resulting paths:

| Platform | Path |
|----------|------|
| Linux    | `~/.config/mist/config.toml` |
| macOS    | `~/Library/Application Support/com.wyldware/mist/config.toml` |
| Windows  | `%APPDATA%\Wyldware\mist\config\config.toml` |
| Flatpak  | application ID `com.wyldware.mist` |

The Linux path carries no publisher name, and that is deliberate rather
than a limitation we failed to work around: `directories` on Linux
honours **only** the `application` argument. The XDG spec mandates a
single flat directory per app, and the crate ignores `qualifier` and
`organization` outright on that platform — they are literally named
`_qualifier` and `_organization` in `directories-5.0.1/src/lin.rs`.

Rejected alternatives, for the record:

- **`com.dragonpro`** — nobody ever understood what the "Pro" was for.
  Not a brand worth putting on a user's disk.
- **`net.legendsgames`** — reserved for the Cortex virtual tabletop,
  which is a separate product with a separate audience.
- **`wyldware-mist` as the application name** — tested and reverted. It
  put `wyldware` on the Linux path at the cost of a nonsense folder name
  on macOS and Windows.
