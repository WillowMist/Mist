# Mist

*A **M**UD **I**nterchange **S**tyle **T**emplate.*

A modern MUD client. Cross-platform, no Win32/MFC dependency, with a
custom markup layer that improves on Pueblo/UE 2.61's display vocabulary
while honouring its wire format. Telnet protocol, ANSI parser, Pueblo's
MUSH markup, and the TinTin++ scripting language are all implemented
from scratch in Rust.

**Status:** v0.2.0 — round-trips a real telnet connection, parses Pueblo
`<xch_*>` markup, fetches `<img>` resources, offers a tabbed egui shell,
and themes itself from a Material 3 palette. See
[MIST-SPEC.md](./MIST-SPEC.md) for the design spec,
[MIST-ANCESTRY.md](./MIST-ANCESTRY.md) for the Pueblo lineage, and
[UI-REQS.md](./UI-REQS.md) for the requirements the next milestones
answer to.

## Design philosophy

**One template, many worlds.** Mist is a *style template* for MUD
output, not a client that hardcodes what a MUD looks like. A theme
defines the whole visual vocabulary — surfaces, roles, the ANSI 16 — and
every world renders through it. The world supplies content; Mist
supplies the frame.

**Honour the wire, improve the surface.** Pueblo's markup is a real
format with real users, so Mist parses all 49 of its tags unchanged.
What Mist replaces is the *renderer*: block-flow-plus-align gives way to
a flex-derived layout, and a fixed 16-colour table gains truecolor
pass-through and a theming layer nobody had in 2004.

**The creator's choices are sacred.** A world that sends `#8a2be2` does
so because that purple means something to its players, and Mist paints
it exactly. Only *indexed* colour references get remapped through a
theme, because an index is a reference into a palette rather than a
colour itself. See [UI-REQS.md](./UI-REQS.md) §1.6.

**Neutral formats, not one app's dialect.** The theme format is
Material 3's semantic vocabulary rather than Alacritty's or Ghostty's or
GTK's, because a vocabulary tied to one program's output breaks when
that program changes. Alacritty, Ghostty, Kitty and KColorscheme are
all read as importers so existing files work — but none of them is
canonical.

**A tool you can theme.** Colours live in a file you can edit while the
client is running. No restart, no menu trip. `~/.config/mist/themes/`,
drop a file in, it appears.

## Quick start

```
cargo run -p mist                  # the binary (default = plain ANSI)
MOCK_MUD_PORT=4001 MOCK_MUD_MODE=pueblo cargo run -p mist-mock &
MOCK_MUD_PORT=4001 cargo run -p mist
cargo test                        # 50+ tests across mist-core, mist-render, mist-ui
```

The connect dialog accepts DNS names and IP addresses.

## Crates

| Crate         | Role                                                |
|---------------|------------------------------------------------------|
| `mist-core`   | Telnet state machine, ANSI parser, Pueblo markup, TinTin scripting, profile storage. No GUI. |
| `mist-render` | Custom HTML/MWorld layout (text runs, images, line breaks, flex containers). |
| `mist-ui`     | egui application shell — windows, tabs, scrollback, input line, connect dialog. |
| `mist-bin`    | The binary.                                            |
| `mist-mock`   | Test bench. Spawned by `cargo run -p mist-mock`; emits Pueblo-style markup on a chosen port. |

## Ancestry

Mist is a from-scratch rewrite of the open-source Pueblo/UE 2.61 MUD
client, with three goals Pueblo didn't have:

1. **No platform lock-in.** Pueblo was a Win32 + MFC application. Mist
   runs on Linux/macOS/Windows from the same binary.
2. **A modern display layer.** Pueblo's 49-tag vocabulary was useful
   in 1996 but didn't survive contact with HTML5. Mist defines a small
   Flex-derived layout grammar on top of a Pueblo-compatible parser.
3. **Forward compatibility.** Mist accepts old Pueblo markup unchanged
   so existing MUDs work; new servers can use HTML5-flavour tags.

See [MIST-ANCESTRY.md](./MIST-ANCESTRY.md) for the lineage, credits,
and a tag-by-tag map of which Pueblo tags Mist accepts, renames, or
ignores.

## Licence

Mist is MIT licensed — see [`LICENSE`](./LICENSE). Copyright (c) 2026 Wyldware.

Mist contains no code from Pueblo/UE. The original Pueblo/UE 2.61 source
(Andromedia Public License v1.0) is kept locally as a read-only reference
and is excluded from version control; if you need to inspect it, re-extract
`pueblosrc261.zip` from the upstream SourceForge archive. Its licence text
lives at `extracted/LICENSE.html` once extracted.

## Publisher and config namespace

Mist is a **Wyldware** product. `wyldware.com` has been hosting MUDs
since the mid-90s.

Persistent state lives in the platform-idiomatic location for the
`com.wyldware` / `Wyldware` / `mist` application triple:

| Platform | Path |
|----------|------|
| Linux    | `~/.config/mist/config.toml` |
| macOS    | `~/Library/Application Support/com.wyldware/mist/config.toml` |
| Windows  | `%APPDATA%\Wyldware\mist\config\config.toml` |

Linux gets a plain `~/.config/mist/` because XDG mandates a single flat
directory per application; that's why `directories` discards the
qualifier and organization on that platform. A Flatpak build will carry
the application ID `com.wyldware.mist`. See
[`mist-core/src/profile.rs`](./mist-core/src/profile.rs) for the
resolution code and
[`mist-core/tests/namespace.rs`](./mist-core/tests/namespace.rs) for the
invariant test.
