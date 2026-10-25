//! Login-sequence detection.
//!
//! MUD servers present their login sequence in several shapes, and the
//! client has to work out which one it is looking at before it can
//! decide what to type. UI-REQS §2.3 lists the shapes:
//!
//! 1. Telnet negotiation then a prompt — the server sends `login:` or
//!    `What is your name?`, possibly after a banner.
//! 2. Line-by-line with no prompt at all — the server just wants
//!    input. Many old MUDs do this.
//! 3. Password-only — the server asks for a password and nothing else.
//! 4. Post-connect character selection — the login succeeded and now
//!    the MUSH asks which character to play.
//! 5. MXP / secure-mode login — the prompt is an MXP `<SEND>` link, so
//!    the literal text "login:" never appears.
//!
//! Detection is a small state machine over the bytes that arrive. It
//! watches for known prompt words, tells password prompts apart from
//! name prompts, and gives up after a bounded window so the
//! unprompted shape (2) still works.
//!
//! The state machine only *observes*. What the client should send is
//! the caller's decision — the detection result says what kind of
//! prompt it saw, and the caller decides whether that warrants
//! sending a login.

/// What kind of prompt the server presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// The server is asking for a login name.
    Login,
    /// The server is asking for a password.
    Password,
    /// The server is asking which character to play.
    Character,
    /// The server asked something we do not recognise.
    Unknown,
}

/// A prompt the detector recognised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// Which kind of prompt it is.
    pub kind: PromptKind,
    /// The literal text that matched, lowercased.
    pub matched: String,
    /// Where in the stream it matched, in bytes from the start of the
    /// observed window.
    pub offset: usize,
}

/// The outcome of watching a connection's opening bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    /// A prompt was recognised. No action has been taken yet.
    Found(Prompt),
    /// No prompt appeared within the window. The server probably wants
    /// input with no prompt — send the login and see what happens.
    Unprompted,
    /// Detection is switched off for this world.
    Disabled,
}

/// Words that mean "tell me your name".
///
/// Ordered longest-first so that "user name" wins over "name" and
/// "what is your name" wins over "name".
const NAME_PROMPTS: &[&str] = &[
    "what is your name",
    "enter your name",
    "user name",
    "username",
    "your name",
    "please enter your name",
    "login name",
    "account name",
    "login",
    "name",
    "account",
    "ident",
    "call",
];

/// Words that mean "tell me the password".
const PASSWORD_PROMPTS: &[&str] = &[
    "password",
    "passphrase",
    "pass word",
    "passwd",
    "pass:",
    "pass ",
];

/// What the detector is currently doing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    /// Waiting for the first prompt.
    WaitingForLogin,
    /// The login was accepted; waiting for a password prompt.
    WaitingForPassword,
    /// The password was accepted; waiting for a character prompt.
    WaitingForCharacter,
    /// Detection is over.
    Done,
}

/// Watches a connection's opening bytes and works out what the server
/// is asking for.
///
/// The detector is fed the *decoded* text — ANSI stripped, telnet
/// negotiation removed — because a prompt can be wrapped in colour
/// codes or arrive split across reads.
#[derive(Debug, Clone)]
pub struct LoginDetector {
    stage: Stage,
    /// Text seen so far, lowercased, bounded in length.
    buffer: String,
    /// How many bytes of raw text have been observed.
    seen: usize,
    /// The window in which a prompt must appear.
    max_bytes: usize,
    /// The result, once detection settles.
    result: Option<Detection>,
    /// Whether detection is switched on at all.
    enabled: bool,
}

impl Default for LoginDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginDetector {
    /// A detector with the default window.
    pub fn new() -> Self {
        Self {
            stage: Stage::WaitingForLogin,
            buffer: String::new(),
            seen: 0,
            // 4 KiB is enough to cover a banner and a prompt without
            // swallowing a whole page of room description.
            max_bytes: 4096,
            result: None,
            enabled: true,
        }
    }

    /// A detector that will not report anything.
    ///
    /// Used when the world's `login_autodetect` flag is off — the
    /// caller then knows it is responsible for the whole sequence.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            result: Some(Detection::Disabled),
            ..Self::new()
        }
    }

    /// Set the byte budget for the detection window.
    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// Whether detection is switched on.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The detection result, if one has been reached.
    pub fn result(&self) -> Option<&Detection> {
        self.result.as_ref()
    }

    /// Feed decoded text from the server.
    ///
    /// Returns the detection result when this call settles it. The
    /// detector keeps running after a `Login` prompt is found, because
    /// the password prompt comes later.
    pub fn feed(&mut self, text: &str) -> Option<Detection> {
        if !self.enabled {
            return self.result.clone();
        }
        if matches!(self.stage, Stage::Done) {
            return self.result.clone();
        }

        self.seen += text.len();
        self.buffer.push_str(&text.to_lowercase());
        // Keep the tail: a prompt may straddle the truncation point.
        const MAX_BUFFER: usize = 8192;
        if self.buffer.len() > MAX_BUFFER {
            let excess = self.buffer.len() - MAX_BUFFER;
            self.buffer.drain(..excess);
        }

        self.scan();

        if self.seen >= self.max_bytes
            && !matches!(self.result, Some(Detection::Found(_)))
        {
            // The window closed without a prompt. The server wants
            // input with no prompt — shape 2.
            self.stage = Stage::Done;
            self.result = Some(Detection::Unprompted);
            return self.result.clone();
        }

        self.result.clone()
    }

    /// Tell the detector that the login name was sent.
    ///
    /// This advances the state machine to looking for a password
    /// prompt. It is the caller's job to send the login; the detector
    /// only observes.
    pub fn sent_login(&mut self) {
        if self.stage == Stage::WaitingForLogin {
            self.stage = Stage::WaitingForPassword;
            self.buffer.clear();
        }
    }

    /// Tell the detector that the password was sent.
    pub fn sent_password(&mut self) {
        if self.stage == Stage::WaitingForPassword {
            self.stage = Stage::WaitingForCharacter;
            self.buffer.clear();
        }
    }

    /// Tell the detector that the character name was sent.
    pub fn sent_character(&mut self) {
        self.stage = Stage::Done;
    }

    /// Look for a prompt in the buffered text.
    ///
    /// The stage says what we are waiting for, so the same words mean
    /// different things at different points in the sequence: "name" is
    /// the login at the start and the character selection after a
    /// password.
    fn scan(&mut self) {
        match self.stage {
            Stage::WaitingForLogin => {
                // A password prompt before any name prompt is shape 3
                // — a password-only world. A name prompt is the
                // ordinary case.
                if let Some(p) = self.find(PASSWORD_PROMPTS) {
                    self.result = Some(Detection::Found(Prompt {
                        kind: PromptKind::Password,
                        matched: p.0,
                        offset: p.1,
                    }));
                } else if let Some(p) = self.find(NAME_PROMPTS) {
                    self.result = Some(Detection::Found(Prompt {
                        kind: PromptKind::Login,
                        matched: p.0,
                        offset: p.1,
                    }));
                }
            }
            Stage::WaitingForPassword => {
                if let Some(p) = self.find(PASSWORD_PROMPTS) {
                    self.result = Some(Detection::Found(Prompt {
                        kind: PromptKind::Password,
                        matched: p.0,
                        offset: p.1,
                    }));
                }
            }
            Stage::WaitingForCharacter => {
                // Character selection looks exactly like a name prompt,
                // but we only reach this stage after a password, so a
                // name prompt here means "which character?".
                if let Some(p) = self.find(NAME_PROMPTS) {
                    self.result = Some(Detection::Found(Prompt {
                        kind: PromptKind::Character,
                        matched: p.0,
                        offset: p.1,
                    }));
                }
            }
            Stage::Done => {}
        }
    }

    /// Find the earliest matching word in the buffer.
    ///
    /// Returns the matched word and its byte offset in the buffer.
    fn find(&self, words: &[&str]) -> Option<(String, usize)> {
        let mut best: Option<(String, usize)> = None;
        for word in words {
            if let Some(pos) = self.buffer.find(word) {
                match &best {
                    // Earliest match wins; on a tie, the longer word
                    // wins so "user name" beats "name".
                    Some((b, bp)) if *bp < pos || (*bp == pos && b.len() >= word.len()) => {}
                    _ => best = Some(((*word).to_string(), pos)),
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(text: &str) -> Detection {
        let mut d = LoginDetector::new();
        let mut result = d.feed(text);
        // Keep feeding so a later prompt can supersede an earlier one.
        for _ in 0..4 {
            match &result {
                Some(Detection::Found(_)) | Some(Detection::Disabled) => break,
                Some(Detection::Unprompted) => break,
                None => {}
            }
            result = d.feed("");
            if result.is_some() {
                break;
            }
        }
        result.expect("expected a detection result")
    }

    #[test]
    fn detects_a_login_prompt() {
        let d = detect("Welcome to the MUD!\r\nWhat is your name? ");
        match d {
            Detection::Found(p) => {
                assert_eq!(p.kind, PromptKind::Login);
                assert!(p.matched.contains("name"));
            }
            other => panic!("expected a login prompt, got {other:?}"),
        }
    }

    #[test]
    fn detects_a_password_prompt() {
        let d = detect("Password: ");
        match d {
            Detection::Found(p) => assert_eq!(p.kind, PromptKind::Password),
            other => panic!("expected a password prompt, got {other:?}"),
        }
    }

    #[test]
    fn a_password_prompt_is_not_mistaken_for_a_name_prompt() {
        // "account" appears in "account password"; the password prompt
        // must win.
        let d = detect("Your account password:");
        match d {
            Detection::Found(p) => assert_eq!(p.kind, PromptKind::Password),
            other => panic!("expected a password prompt, got {other:?}"),
        }
    }

    #[test]
    fn recognises_a_range_of_login_words() {
        for word in ["login:", "username:", "user name:", "name:", "ident:", "call:"] {
            let d = detect(word);
            match d {
                Detection::Found(p) => assert_eq!(p.kind, PromptKind::Login, "for {word:?}"),
                other => panic!("expected a login prompt for {word:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_prompt_split_across_reads_is_still_found() {
        let mut d = LoginDetector::new();
        assert!(d.feed("What is your ").is_none());
        let r = d.feed("name? ");
        match r {
            Some(Detection::Found(p)) => assert_eq!(p.kind, PromptKind::Login),
            other => panic!("expected a login prompt, got {other:?}"),
        }
    }

    #[test]
    fn an_unprompted_server_times_out_into_the_unprompted_shape() {
        let mut d = LoginDetector::new().with_max_bytes(64);
        let mut result = None;
        for _ in 0..20 {
            result = d.feed("some room description with no prompt at all");
            if result.is_some() {
                break;
            }
        }
        assert_eq!(result, Some(Detection::Unprompted));
    }

    #[test]
    fn the_timeout_does_not_fire_once_a_prompt_was_seen() {
        // The window is about finding the *first* prompt. Once one is
        // found, the machine waits for the next stage instead of
        // timing out into the unprompted shape.
        let mut d = LoginDetector::new().with_max_bytes(32);
        let first = d.feed("login: ");
        assert!(matches!(first, Some(Detection::Found(_))));
        d.sent_login();
        let after = d.feed("a fairly long stretch of room description text");
        assert!(
            !matches!(after, Some(Detection::Unprompted)),
            "must not report unprompted after a prompt was seen"
        );
    }

    #[test]
    fn a_disabled_detector_reports_disabled() {
        let mut d = LoginDetector::disabled();
        assert_eq!(d.feed("login:"), Some(Detection::Disabled));
        assert!(!d.enabled());
    }

    #[test]
    fn after_a_login_a_password_prompt_is_recognised() {
        let mut d = LoginDetector::new();
        let first = d.feed("login: ");
        assert!(matches!(first, Some(Detection::Found(_))));
        d.sent_login();
        let second = d.feed("Password: ");
        match second {
            Some(Detection::Found(p)) => assert_eq!(p.kind, PromptKind::Password),
            other => panic!("expected a password prompt, got {other:?}"),
        }
    }

    #[test]
    fn after_a_password_a_name_prompt_is_a_character_prompt() {
        let mut d = LoginDetector::new();
        d.feed("login: ");
        d.sent_login();
        d.feed("Password: ");
        d.sent_password();
        let r = d.feed("What is your name? ");
        match r {
            Some(Detection::Found(p)) => assert_eq!(p.kind, PromptKind::Character),
            other => panic!("expected a character prompt, got {other:?}"),
        }
    }

    #[test]
    fn the_longer_word_wins_a_tie() {
        // "user name" and "name" both match at the same offset.
        let d = detect("user name: ");
        match d {
            Detection::Found(p) => assert_eq!(p.matched, "user name"),
            other => panic!("expected a login prompt, got {other:?}"),
        }
    }

    #[test]
    fn a_banner_before_the_prompt_does_not_confuse_it() {
        let d = detect(
            "\u{1b}[1;36mPueblo Mock MUSH\u{1b}[0m\r\n\r\nA small stone square.\r\nlogin: ",
        );
        match d {
            Detection::Found(p) => assert_eq!(p.kind, PromptKind::Login),
            other => panic!("expected a login prompt, got {other:?}"),
        }
    }
}
