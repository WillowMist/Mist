# Agent config (durable, picked up by subagent discovery)

This file is read by any subagent spawned against this project. It is
the durable layer of the model-pinning observation: the tools we have,
the verified model IDs from `tools.opencode.models({})`, and the
phantom to NEVER use.

## Tools available

From the catalog as of this session:
- `tools.search`, `tools.execute` (Code Mode)
- `tools.opencode.{worktree_create,worktree_list,worktree_move,
  worktree_remove,session_move,session_rename,models,
  list_mcp_resources,read_mcp_resource}`
- `tools.rea.*` (REA reverse-engineering tool family — try
  `tools.search({query: "open_binary", namespace: "rea"})` to discover
  the full set on each session start)
- `tools.fixer`, `tools.observer` may exist depending on the catalog

`oh-my-opencode-slim` is listed in the skill catalog but **no MCP
backs it in this catalog**; there is no central `model:` config to
write. Pin model per-dispatch via the explicit `model:` parameter on
each `subagent(...)` call.

## Verified model IDs

From `tools.opencode.models({limit: 60})` probe (provider prefix
required — bare IDs like `claude-haiku-5-5` will fail):

| Purpose             | Model ID                                    |
|---------------------|---------------------------------------------|
| Explorer (read)     | `opencode-go/deepseek-v4.1-flash`            |
| Fixer (code edits)  | `opencode-go/deepseek-v4.1-flash`            |
| Oracle (escalation)  | `opencode-go/claude-haiku-5-5`               |
| Designer (UI/UX)    | `opencode-go/gemini-4-argon`                 |
| Observer / fallback | `opencode-go/minimax-m3.1-flash`             |
| Strong general      | `opencode-go/qwen3.8-max` or `-plus`        |

### Phantom model (do not use)

`opencode-go/deepseek-v4-flash` — this is the opencode auto-route
default for unmodel'd subagent dispatches and **does not exist** in
the catalog. Discovered mid-session; caused repeated
`Model unavailable: opencode-go/deepseek-v4-flash` errors on every
early subagent dispatch in this project. Always set `model:` per-call.

## Project conventions for ~/Projects/mist

- Crates: `mist-core` (no UI), `mist-render` (custom markup layout),
  `mist-ui` (egui shell), `mist-bin` (binary), `mist-mock` (test bench).
- Build/test commands: `cargo build -p mist`; `cargo test -p mist-core`.
- Telnet state machine is in `mist-core/src/telnet.rs`; mirrors Pueblo 2.61.
- TinTin verb table (71 verbs) is in `mist-core/src/tintin.rs`.
- The original Pueblo/UE 2.61 source tree (read-only reference) lives at
  `~/Projects/mist/extracted/`. The packaged archive is
  `~/Projects/mist/pueblosrc261.zip`. Do not modify either.

## Reference docs in this project

- `README.md` — pitch, quick start, crate table, namespace, licence
- `MIST-SPEC.md` — design spec (markup dialect, layout, wire format)
- `MIST-ANCESTRY.md` — Pueblo 2.61 → Mist lineage, credits, tag mapping
- `UI-REQS.md` — collected UI requirements: theming, worlds/accounts, login autodetection
- `MIST-WYLDWARE.md` — server-side softcode snippets for wyldware
- `AGENTS.md` — this file
