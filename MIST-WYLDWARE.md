# wyldware.com:2020 Pueblo-mode demo content

The user is hosting a MUSH at wyldware.com:2020 that doesn't currently
emit Pueblo markup. v0.2 of the client will render anything the MUSH
sends as long as it's valid MWorld/Pueblo-mode HTML — but the *server*
has to produce it.

This file is a set of **copy-pasteable MUSH softcode objects** —
rooms, objects, descriptions, softcode attributes — that turn part of
your MUSH into a v0.2 test bench you can `@pemit` yourself into or
visit. None of this requires learning Pueblo; the markup is static
text the MUSH prints verbatim when you enter the room.

The simpler alternative, also covered below, is to make your character
a "Pueblo chatter" that bounces markup-rich strings to whoever is in
the same room.

---

## Why a mock-only thing falls short

`pueblo-mock` (built from `pueblo-mock/`, run with `MOCK_MUD_MODE=pueblo`
on port 4001) emits the right markup stream. Connect the client to
`127.0.0.1:4001` and you'll see everything end-to-end. But that's only
useful while you're *off* wyldware; the moment you point the client at
`wyldware.com:2020`, what arrives is whatever wyldware naturally emits,
which is plain ANSI today.

These softcode fragments are intended to make wyldware look like
`pueblo-mock -mode=pueblo` in selected rooms or for specific commands.

---

## Penn / Tiny MUSH: a Pueblo-decorated room

The cleanest pattern: create a room whose `DESC` is one big Pueblo-mode
markup string. Whenever someone enters, the MUSH prints `DESC` — and
that's the markup event the client renders.

```
@dig Pueblo Demo Room = ${place here}

@set Pueblo Demo Room = FLOATING  ; (so description is shown on entry)
@set Pueblo Demo Room = OPAQUE    ; (so it stays a normal room)

&DESCRIBE Pueblo Demo Room = {[ansi(note)]}\
<xch_page clear=text><hr>\
<b>Pueblo Demo Room</b> -- <i>welcome</i>\
<hr>\r\n\
\r\n\
You stand in a small stone circle. Around you:\
\r\n\
&nbsp;* <a xch_cmd="look well">an old stone well</a>\
&nbsp;* <a xch_cmd="look sign">a weathered wooden sign</a>\
\r\n\
<font color="#80c0ff">Exits:</font> <a xch_cmd="north">north</a>, \
<a xch_cmd="south">south</a>, <a xch_cmd="east">east</a>, \
<a xch_cmd="west">west</a>.\
\r\n\
Type <b>who</b>, <b>score</b>, or any command to interact.
```

Notes for PennMUSH/TinyMUSH editors:

- Brace-marker strings like `[ansi(note)]` are conventional noise that
  most MUSHes strip when displaying ANSI; replace with the actual
  ANSI byte sequence (`\x1B[36m` etc.) if your parser doesn't grok
  `[ansi(...)]`.
- `&nbsp;` is the standard HTML entity; the pueblo-ui parser is lenient
  about it (treats it as a literal space when no entity handler matches).
- Room descriptions in Penn/Tiny echo automatically when the player
  enters the room and when they look at it. No trigger needed.

If you want the same string pushed when someone `@pemit`s it, just
override `@pemit` on the room to call `@pemit %#=<desc>`.

---

## A "Pueblo helper" NPC

The other pattern is a server-side NPC that recognises `#who` and
`#score` Pueblo commands and answers with markup. Useful when you want
the markup to come from a *command* rather than a *room*.

```
@create Pueblo Helper = Living Thing
@set Pueblo Helper = PUPPET_OK

&DESC Pueblo Helper = Pueblo Markup Helper -- type <b>who</b> or \
<b>score</b> for a MWorld page.

@UHALF Pueblo Helper = $who *:@pemit %# = \
  {[ansi(h)]}<xch_page clear=text><b>Players online</b><hr>\
   [iter(lwho(),%r&nbsp;* <font color="#80ff80">%b0</font>)]\
  <i>end of page</i>}

@UHALF Pueblo Helper = $score *:@pemit %# = \
  {[ansi(h)]}<xch_page clear=text><b>Character sheet</b><hr>\
  <b>HP</b> <font color="#ff6060">{eval(health(#%))}</font>\
  <b>Mana</b> <font color="#6080ff">{eval(mana(#%))}</font>}
```

PennMUSH uses `{[function()]}`. TinyMUSH uses `[function()]` without the
brace and stores attributes in `&UATTR`. Adapt for your flavour.

---

## Why the markup is what you pasted, not the client's job

The client side parses `&lt;b&gt;`, `&lt;xch_page&gt;`, etc. as they
arrive in the byte stream. The server, not the client, decides when
they appear. So:

- A description is rendered once on entry.
- A `@pemit` is rendered once on receipt.
- A `#who` reaction is rendered once per invocation.

If the MUSH is *also* emitting **ANSI escapes** for color (the legacy
`[ansi(red)]` form), the client strips those first, then parses the
MWorld markup on the remaining text. Both layers work in the same line
— that's the whole point of Pueblo's two-stage stream.

---

## Caveats specific to wyldware

A few things I don't know about your MUSH and that you'll need to
verify before I can say this would Just Work:

1. **Does your flavour of MUSH allow `<a xch_cmd>` in player-facing
   text?** Some softcode setups escape angle-brackets or strip `&`
   chars (HTML entity territory). The client parser is lenient but the
   *server output path* has to keep the markup intact.

2. **Are player `@pemits` to other players filtered?** If yes, the
   NPC `'@UHALF'` approach is the answer; if no, the description
   approach works and is even simpler.

3. **What is wyldware's telnet handshake — does it negotiate MCCP
   v1/v2, ECHO, EOR, NAWS?** This affects which features light up.
   Pasting a 5-second `tcpdump`-equivalent capture from wyldware:2020
   would let me wire any option-specific behaviour in v0.3.

If any of those answers are obvious to you, drop me a one-line note
and I'll bake the right path into v0.2's mock (or v0.3) so wyldware
becomes the canonical smoke target.
