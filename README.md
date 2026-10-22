# Mist

A modern MUD client. Cross-platform, no Win32/MFC dependency, with a
custom markup layer that improves on Pueblo/UE 2.61's display vocabulary
while honouring its wire format. Telnet protocol, ANSI parser, Pueblo's
MUSH markup, and the TinTin++ scripting language are all implemented
from scratch in Rust.

**Status:** v0.2.0 — round-trips a real telnet connection, parses Pueblo
`<xch_*>` markup, fetches `<img>` resources, and offers a tabbed
egui shell. See [MIST-SPEC.md](./MIST-SPEC.md) for the design spec,
[MIST-ANCESTRY.md](./MIST-ANCESTRY.md) for the Pueblo lineage, and
[MIST-WYLDWARE.md](./MIST-WYLDWARE.md) for server-side softcode to
exercise it on a real MUSH.

## Quick start

```
cargo run -p mist                  # the binary (default = plain ANSI)
MOCK_MUD_PORT=4001 MOCK_MUD_MODE=pueblo cargo run -p mist-mock &
MOCK_MUD_PORT=4001 cargo run -p mist
cargo test                        # 30+ tests across mist-core, mist-render, mist-ui
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
since the mid-90s; the Cortex virtual tabletop is a separate Wyldware
lineage under `net.legendsgames`.

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
