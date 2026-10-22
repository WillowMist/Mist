//! TinTin scripting engine.
//!
//! The full verb list lives at `modules/client/msw/ChWorld/TinTinUtils.cpp:60`
//! in the original Pueblo/UE 2.61 source — 69 commands plus 2 help-only
//! (`verbatim`, `enhancements`, `expressions`). This module implements a
//! majority of them at the level needed for MUSH use; tokens whose body we
//! do not evaluate are stored in [`Command::Stub`] with the original verb
//! name and arguments preserved so the host can log and eventually execute.
//!
//! Verb recognition mirrors Pueblo's [`IsAbrev`] algorithm: a prefix matches
//! iff it is a unique prefix of exactly one entry in the canonical table.

use crate::error::{PuebloError, Result};
use std::collections::HashMap;

/// One parsed TinTin directive. Created from a single input line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Alias { left: String, right: String },
    Action { pattern: String, body: String },
    Substitute { pattern: String, replacement: String },
    Antisubstitute { pattern: String, replacement: String },
    Highlight { pattern: String, color: String },
    Variable { name: String, value: String },
    Pathdir { dir: String, action: String },
    Tick { interval: u32, body: String },
    Tab { name: String, body: String },
    Gag,
    Read { path: String },
    Write { path: String },
    Echo(String),
    ShowMe(String),
    ShowMeHtml(String),
    Info,
    If { left: String, op: CondOp, right: String },
    End,
    Verbatim { on: bool },
    Session { name: String },
    Help,
    /// Unknown or un-implemented verb — preserved for the host to record.
    Stub { verb: String, args: String },
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CondOp { Eq, Ne, Lt, Gt, Le, Ge }

/// As `parse_line`, but `$variables` and `&N` references are expanded
/// against the given state before dispatch. `$name` looks up the user
/// variables in `state.variables`; unknown `$name` references become
/// the empty string — that's the original Pueblo behaviour. `&1..&9`
/// substitutes from `state.args`.
pub fn parse_line_with_state(input: &str, state: &State) -> Result<Command> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Command::Noop);
    }
    let stripped = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let stripped = stripped.trim_start();
    let substituted = substitute_my_vars(stripped, state);
    let mut tokens = tokenise(&substituted);
    let head = tokens.next().unwrap_or_default();
    let rest_args: Vec<String> = tokens.collect();
    let rest = rest_args.join(" ");
    parse_inner(head, rest_args, rest)
}

/// Canonical verb table — same order as `TinTinUtils.cpp::m_pstrCommands`.
const COMMANDS: &[&str] = &[
    "action",
    "alert",
    "alias",
    "all",
    "antisubstitute",
    "bell",
    "boss",
    "char",
    "cr",
    "echo",
    "end",
    "gag",
    "help",
    "highlight",
    "history",
    "if",
    "ignore",
    "info",
    "killall",
    "log",
    "loop",
    "map",
    "math",
    "mark",
    "mbox",
    "message",
    "name",
    "nop",
    "path",
    "pathdir",
    "playsound",
    "playsoundbg",
    "presub",
    "random",
    "redraw",
    "retab",
    "return",
    "read",
    "savepath",
    "session",
    "showme",
    "showmehtml",
    "speedwalk",
    "split",
    "substitute",
    "textin",
    "tick",
    "tickkey",
    "tickoff",
    "tickon",
    "tickset",
    "ticksize",
    "tolower",
    "togglesub",
    "toupper",
    "unaction",
    "unalias",
    "unantisubstitute",
    "unhighlight",
    "unsubstitute",
    "ungag",
    "unpath",
    "unvariable",
    "variable",
    "version",
    "wizlist",
    "write",
    "writesession",
    "zap",
    // help-only verbs (recognised but produce a help hint)
    "verbatim",
    "enhancements",
    "expressions",
];

/// Mimics Pueblo's `TinTin::IsAbrev`. Returns the matched command if `prefix`
/// is a unique prefix of exactly one entry in [`COMMANDS`].
pub fn unique_abbrev(prefix: &str) -> Option<&'static str> {
    let p = prefix.to_ascii_lowercase();
    let mut hits = COMMANDS.iter().copied().filter(|c| c.starts_with(&p));
    let first = hits.next()?;
    if hits.next().is_some() {
        None
    } else {
        Some(first)
    }
}

/// Recognises the brace-token form `#alias {n} {north}` (Pueblo's canonical
/// syntax) and bare-word forms. Unknown verbs return [`Command::Stub`].
///
/// For the common case where `State` already holds the current
/// `&1..&9` argument slots, prefer [`parse_line_with_state`]; it
/// expands `$vars` and `&args` first, exactly the way Pueblo's
/// `TinTin::ParseInput` does at `TinTinParse.cpp:120-130`.
pub fn parse_line(input: &str) -> Result<Command> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Command::Noop);
    }
    let stripped = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let stripped = stripped.trim_start();
    let mut tokens = tokenise(stripped);
    let head = tokens.next().unwrap_or_default();
    let rest_args: Vec<String> = tokens.collect();
    let rest = rest_args.join(" ");
    parse_inner(head, rest_args, rest)
}

fn parse_inner(head: String, rest_args: Vec<String>, rest: String) -> Result<Command> {
    let head_lower = head.to_ascii_lowercase();
    let verb = unique_abbrev(&head).unwrap_or(head_lower.as_str());

    Ok(match verb {
        "alias" => expect2(rest_args, "alias").map(|(l, r)| Command::Alias { left: l, right: r })?,
        "action" => expect2(rest_args, "action").map(|(l, r)| Command::Action { pattern: l, body: r })?,
        "substitute" => expect2(rest_args, "substitute").map(|(l, r)| Command::Substitute { pattern: l, replacement: r })?,
        "antisubstitute" => expect2(rest_args, "antisubstitute").map(|(l, r)| Command::Antisubstitute { pattern: l, replacement: r })?,
        "highlight" => expect2(rest_args, "highlight").map(|(l, r)| Command::Highlight { pattern: l, color: r })?,
        "variable" => expect2(rest_args, "variable").map(|(l, r)| Command::Variable { name: l, value: r })?,
        "pathdir" => expect2(rest_args, "pathdir").map(|(l, r)| Command::Pathdir { dir: l, action: r })?,
        "tick" => {
            if rest_args.is_empty() {
                return Err(PuebloError::TinTin("tick needs interval + body".into()));
            }
            let interval: u32 = rest_args[0].parse().map_err(|_| PuebloError::TinTin("bad tick interval".into()))?;
            let body = rest_args[1..].join(" ");
            Command::Tick { interval, body }
        }
        "tab" => expect2(rest_args, "tab").map(|(l, r)| Command::Tab { name: l, body: r })?,
        "gag" => {
            // Pueblo auto-wraps bare text in braces and appends " .".
            let wrapped = if rest_args.is_empty() || !rest.starts_with('{') {
                format!("{{{}}}", rest)
            } else {
                rest.clone()
            };
            Command::Substitute { pattern: wrapped.clone() + " .", replacement: String::new() }
        }
        "read" => Command::Read { path: rest.clone() },
        "write" => Command::Write { path: rest.clone() },
        "echo" => Command::Echo(rest.clone()),
        "showme" => Command::ShowMe(rest.clone()),
        "showmehtml" => Command::ShowMeHtml(rest.clone()),
        "info" => Command::Info,
        "if" => {
            if rest_args.len() >= 4 {
                let op = parse_cond(&rest_args[1])
                    .ok_or_else(|| PuebloError::TinTin(format!("bad if comparator: {}", rest_args[1])))?;
                Command::If { left: rest_args[0].clone(), op, right: rest_args[3].clone() }
            } else {
                return Err(PuebloError::TinTin("if needs: <left> <op> <right> <body>".into()));
            }
        }
        "end" => Command::End,
        "verbatim" => Command::Verbatim { on: matches!(rest_args.first().map(String::as_str), Some("on") | Some("1")) },
        "session" => Command::Session { name: rest.clone() },
        "help" => Command::Help,
        // Un-implemented verbs; surface them for the host.
        v => Command::Stub { verb: v.to_string(), args: rest.clone() },
    })
}

/// Tokenise a stripped TinTin command line. Recognises `{...}` braced tokens
/// as single tokens; otherwise splits on ASCII whitespace.
fn tokenise(s: &str) -> impl Iterator<Item = String> + '_ {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut current = String::new();
    let mut pending: Vec<String> = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'{' {
            let mut depth = 1usize;
            let mut tok = String::new();
            i += 1;
            while i < bytes.len() && depth > 0 {
                let c = bytes[i];
                if c == b'{' { depth += 1; tok.push('{'); }
                else if c == b'}' {
                    depth -= 1;
                    if depth == 0 { break; }
                    tok.push('}');
                } else { tok.push(c as char); }
                i += 1;
            }
            if depth == 0 { i += 1; }
            if !tok.is_empty() || depth == 0 { pending.push(tok); }
        } else if b.is_ascii_whitespace() {
            if !current.is_empty() {
                pending.push(std::mem::take(&mut current));
            }
            i += 1;
        } else {
            current.push(b as char);
            i += 1;
        }
    }
    if !current.is_empty() {
        pending.push(current);
    }
    pending.into_iter()
}

fn expect2(mut args: Vec<String>, name: &str) -> Result<(String, String)> {
    if args.len() < 2 {
        return Err(PuebloError::TinTin(format!("{name} needs 2 args")));
    }
    let left = args.remove(0);
    let right = args.join(" ");
    Ok((left, right))
}

fn parse_cond(s: &str) -> Option<CondOp> {
    match s {
        "==" | "=" => Some(CondOp::Eq),
        "!=" => Some(CondOp::Ne),
        "<" => Some(CondOp::Lt),
        ">" => Some(CondOp::Gt),
        "<=" => Some(CondOp::Le),
        ">=" => Some(CondOp::Ge),
        _ => None,
    }
}

/// Expand `$var` and `&N` references in a single input line. Mirrors
/// `TinTin::SubstituteMyVars` in `TinTinParse.cpp`:
///
/// * `$name`        → value of `state.variables["name"]`, or `""` if unset.
/// * `${name}`      → same as above; braces delimit the name.
/// * `&N` (1..=9)    → `state.args[N-1]`, or `""` if unset.
///
/// The function walks the string byte-by-byte on `$` / `&`; everything
/// else passes through verbatim. No regex, no allocation unless the
/// substituted value differs.
pub fn substitute_my_vars(input: &str, state: &State) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'$' {
            // Either `$name` or `${name}`.
            i += 1;
            let braced = i < bytes.len() && bytes[i] == b'{';
            if braced {
                i += 1;
            }
            let start = i;
            while i < bytes.len() {
                let c = bytes[i];
                if c.is_ascii_whitespace() || c == b'}' || c == b';' {
                    break;
                }
                i += 1;
            }
            let name = &input[start..i];
            if braced && i < bytes.len() && bytes[i] == b'}' {
                i += 1; // consume closing brace
            }
            if let Some(val) = state.variables.get(name) {
                out.push_str(val);
            }
            // else: leave `$name` as empty string (Pueblo's behaviour).
        } else if b == b'&' {
            // `&N` only when N is `1..=9` *and is bounded* — the next
            // byte is an ASCII digit AND the byte after that is NOT
            // another ASCII digit. This keeps `&10`, `&0`, `&foo`,
            // `& ;` etc. as literal text instead of swallowing digits.
            let next = bytes.get(i + 1).copied();
            let after = bytes.get(i + 2).copied();
            let is_valid_positional = matches!(next, Some(b'1'..=b'9'))
                && !matches!(after, Some(b'0'..=b'9'));
            if is_valid_positional {
                let slot = (next.unwrap() - b'1') as usize;
                if let Some(arg) = state.args.get(slot) {
                    out.push_str(arg);
                }
                i += 2; // consume `&` + single digit
                continue;
            }
            // Unrecognised: emit `&` literal, advance one byte. The next
            // iteration will handle whatever follows.
            out.push('&');
            i += 1;
        } else {
            out.push(b as char);
            i += 1;
        }
    }
    out
}

/// TinTin runtime state — the live world model.
///
/// `args` is the per-invocation `$1..$9` parameter list. Pueblo's
/// `TinTin::ParseInput` populates this from the actual command-line
/// arguments (`&1` is the first word after the verb, `&*` is all of
/// them, `&N` where N>9 wraps to `&10..`). v0.3.0 stores only the
/// direct positional 1..9 — wrap-around / `&*` are future work.
#[derive(Debug, Default)]
pub struct State {
    pub aliases: HashMap<String, String>,
    pub actions: Vec<(String, String)>,
    pub substitutes: Vec<(String, String)>,
    pub highlights: Vec<(String, String)>,
    pub variables: HashMap<String, String>,
    pub pathdirs: Vec<(String, String)>,
    pub session: Option<String>,
    pub verbatim: bool,
    /// Positional `&1..&9` argument slots, populated by the caller
    /// before `parse_line` is invoked.
    pub args: Vec<String>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, cmd: Command) -> Result<()> {
        match cmd {
            Command::Alias { left, right } => { self.aliases.insert(left, right); }
            Command::Action { pattern, body } => self.actions.push((pattern, body)),
            Command::Substitute { pattern, replacement } => self.substitutes.push((pattern, replacement)),
            Command::Antisubstitute { pattern, replacement } => self.substitutes.push((pattern, replacement.clone())),
            Command::Highlight { pattern, color } => self.highlights.push((pattern, color)),
            Command::Variable { name, value } => { self.variables.insert(name, value); }
            Command::Pathdir { dir, action } => self.pathdirs.push((dir, action)),
            Command::Session { name } => self.session = Some(name),
            Command::Verbatim { on } => self.verbatim = on,
            Command::Gag | Command::Noop | Command::End | Command::Info | Command::Help => {}
            Command::Echo(_) | Command::ShowMe(_) | Command::ShowMeHtml(_) => {}
            Command::Read { .. } | Command::Write { .. } => {}
            Command::Tick { .. } | Command::Tab { .. } | Command::If { .. } => {}
            Command::Stub { .. } => {}
        }
        Ok(())
    }

    /// Apply all configured substitutes/highlights to an incoming line of text.
    pub fn transform_line(&self, line: &str) -> String {
        // Highlight runs first (so the user sees colors), then substitute
        // replacements, then gag (any line that matches an antisubstitute
        // becomes empty).
        if !self.verbatim {
            for (pat, _color) in &self.highlights {
                // v0.2: surface a marker; real ANSI render is v0.3
                let _ = pat;
            }
        }
        let mut out = line.to_string();
        for (pat, repl) in &self.substitutes {
            if let Some(rest) = pat.strip_suffix(" .") {
                // gag: pattern was stored with leading `{` and trailing `}`.
                // Strip the braces for the actual substring match.
                let needle = rest
                    .strip_prefix('{')
                    .and_then(|s| s.strip_suffix('}'))
                    .unwrap_or(rest);
                if out.contains(needle) {
                    out = String::new();
                }
            } else if out.contains(pat.as_str()) {
                out = out.replace(pat, repl);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_alias() {
        let c = parse_line("#alias {n} {north}").unwrap();
        match c {
            Command::Alias { left, right } => {
                assert_eq!(left, "n");
                assert_eq!(right, "north");
            }
            _ => panic!("expected Alias"),
        }
    }

    #[test]
    fn empty_is_noop() {
        assert_eq!(parse_line("").unwrap(), Command::Noop);
    }

    #[test]
    fn unknown_verb_becomes_stub() {
        let c = parse_line("#noSuchVerb arg1 arg2").unwrap();
        match c {
            Command::Stub { verb, args } => {
                assert_eq!(verb, "nosuchverb");
                assert_eq!(args, "arg1 arg2");
            }
            _ => panic!("expected Stub"),
        }
    }

    #[test]
    fn abbrev_matches_action() {
        // "ac" is a unique prefix of "action".
        let c = parse_line("#ac {pat} {body}").unwrap();
        match c {
            Command::Action { pattern, body } => {
                assert_eq!(pattern, "pat");
                assert_eq!(body, "body");
            }
            _ => panic!("expected Action via abbreviation"),
        }
    }

    #[test]
    fn ambiguous_abbrev_is_unknown() {
        // "a" is ambiguous (action, alert, alias, all, antisubstitute).
        let c = parse_line("#a x y").unwrap();
        match c {
            Command::Stub { verb, args } => {
                assert_eq!(verb, "a");
                assert_eq!(args, "x y");
            }
            _ => panic!("expected Stub for ambiguous abbreviation"),
        }
    }

    #[test]
    fn substitute_and_gag_present() {
        let c = parse_line("#substitute {You hit} {<hit>}").unwrap();
        match c {
            Command::Substitute { pattern, replacement } => {
                assert_eq!(pattern, "You hit");
                assert_eq!(replacement, "<hit>");
            }
            _ => panic!("expected Substitute"),
        }
        let c = parse_line("#gag {spam line}").unwrap();
        match c {
            Command::Substitute { pattern, replacement } => {
                assert!(pattern.contains("spam line"));
                assert_eq!(replacement, "");
            }
            _ => panic!("expected gag-as-substitute"),
        }
    }

    #[test]
    fn speedwalk_unique_abbrev() {
        // "speedw" is a unique prefix of "speedwalk".
        let c = parse_line("#speedw nne").unwrap();
        match c {
            Command::Stub { verb, args } => {
                assert_eq!(verb, "speedwalk");
                assert_eq!(args, "nne");
            }
            _ => panic!("expected Stub for speedwalk"),
        }
    }

    #[test]
    fn state_transform_applies_substitutes() {
        let mut s = State::new();
        s.apply(parse_line("#substitute {slain} {***}").unwrap()).unwrap();
        s.apply(parse_line("#gag {you died}").unwrap()).unwrap();
        assert_eq!(s.transform_line("you died in battle"), "");
        assert_eq!(s.transform_line("you were slain"), "you were ***");
    }

    // --- substitute_my_vars (L305) -------------------------------------

    #[test]
    fn substitute_my_vars_l305_expands_dollar_name() {
        let mut s = State::new();
        s.variables.insert("hp".into(), "42".into());
        assert_eq!(substitute_my_vars("$hp", &s), "42");
    }

    #[test]
    fn substitute_my_vars_l305_expands_braced_name() {
        let mut s = State::new();
        s.variables.insert("hp".into(), "42".into());
        assert_eq!(substitute_my_vars("${hp}", &s), "42");
        // Braces delimit the name, so trailing `;`/text is preserved.
        assert_eq!(substitute_my_vars("hp=${hp}; ok", &s), "hp=42; ok");
    }

    #[test]
    fn substitute_my_vars_l305_unknown_dollar_becomes_empty() {
        let s = State::new();
        // Unset `$name` expands to "" (Pueblo behaviour), leaving both
        // surrounding spaces in place.
        assert_eq!(substitute_my_vars("hello $nope world", &s), "hello  world");
        assert_eq!(substitute_my_vars("${nope}", &s), "");
        assert_eq!(substitute_my_vars("$nope", &s), "");
    }

    #[test]
    fn substitute_my_vars_l305_respects_name_boundaries() {
        let mut s = State::new();
        s.variables.insert("hp".into(), "42".into());
        assert_eq!(substitute_my_vars("say $hp is low", &s), "say 42 is low");
        // A name ends at whitespace, `}`, or `;` only.
        assert_eq!(substitute_my_vars("$hp;next", &s), "42;next");
        assert_eq!(substitute_my_vars("${hp}next", &s), "42next");
    }

    #[test]
    fn substitute_my_vars_l305_expands_positional_args() {
        let mut s = State::new();
        s.args = (1..=9).map(|n| format!("a{n}")).collect();
        assert_eq!(substitute_my_vars("&1 &2 &9", &s), "a1 a2 a9");
        // Missing slot stays empty rather than panicking.
        let empty = State::new();
        assert_eq!(substitute_my_vars("[&1]", &empty), "[]");
    }

    #[test]
    fn substitute_my_vars_l305_out_of_range_args_keep_ampersand() {
        // `&0` and `&10` are outside the supported `&1..&9` range.
        // We keep the ampersand and the digits verbatim so the user
        // can spot the typo instead of silently dropping characters.
        let mut s = State::new();
        s.args = (1..=9).map(|n| n.to_string()).collect();
        assert_eq!(substitute_my_vars("&10", &s), "&10");
        assert_eq!(substitute_my_vars("&0", &s), "&0");
        assert_eq!(substitute_my_vars("&99x", &s), "&99x");
        // `&` followed by a non-digit (including start-of-input) stays
        // literal too.
        assert_eq!(substitute_my_vars("foo &", &s), "foo &");
        assert_eq!(substitute_my_vars("&foo", &s), "&foo");
        assert_eq!(substitute_my_vars("&", &s), "&");
    }

    // --- parse_line_with_state (L53) -----------------------------------

    #[test]
    fn parse_line_with_state_l53_substitutes_alias_right_side() {
        let mut s = State::new();
        s.variables.insert("foo".into(), "bar".into());
        let c = parse_line_with_state("#alias {n} {north $foo}", &s).unwrap();
        match c {
            Command::Alias { left, right } => {
                assert_eq!(left, "n");
                assert_eq!(right, "north bar");
            }
            _ => panic!("expected Alias"),
        }
    }

    #[test]
    fn parse_line_with_state_l53_stub_respects_substitution() {
        let mut s = State::new();
        s.variables.insert("foo".into(), "bar".into());

        // The aliased right side is parsed separately by the host; the
        // unknown verb must still be a Stub, but with `$foo` expanded.
        let stored = parse_line_with_state("#alias {q} {frobnicate $foo}", &s).unwrap();
        let right = match stored {
            Command::Alias { right, .. } => right,
            _ => panic!("expected Alias"),
        };
        match parse_line_with_state(&right, &s).unwrap() {
            Command::Stub { verb, args } => {
                assert_eq!(verb, "frobnicate");
                assert_eq!(args, "bar");
            }
            _ => panic!("expected Stub"),
        }

        // Directly, too: `#` prefix is optional and substitution still runs.
        match parse_line_with_state("#frobnicate $foo", &s).unwrap() {
            Command::Stub { verb, args } => {
                assert_eq!(verb, "frobnicate");
                assert_eq!(args, "bar");
            }
            _ => panic!("expected Stub"),
        }
    }
}