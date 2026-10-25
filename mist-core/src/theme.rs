//! Theme model, native format, derivation, and importers.
//!
//! The native format is Material 3's semantic vocabulary — `primary`,
//! `on_primary`, `surface`, `on_surface`, `secondary`, `error`, and so
//! on. That vocabulary is published and neutral, so any program can emit
//! it without being coupled to Mist. See `UI-REQS.md` §1 for the
//! rationale and the full token table.
//!
//! Importers for Alacritty, Ghostty, Kitty and KColorscheme exist so
//! theme files those programs already emit work here. They are
//! read-only conveniences; the native format is canonical.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Colour type
// ---------------------------------------------------------------------------

/// An sRGB colour with an explicit alpha channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Self = Self { r: 0, g: 0, b: 0, a: 0 };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        match s.len() {
            3 => {
                // #abc expands to #aabbcc.
                let d = s.as_bytes();
                let hx = |c: u8| {
                    let v = (c as char).to_digit(16)? as u8;
                    Some(v * 17)
                };
                Some(Self::rgb(hx(d[0])?, hx(d[1])?, hx(d[2])?))
            }
            6 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                Some(Self::rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                let a = u8::from_str_radix(&s[6..8], 16).ok()?;
                Some(Self { r, g, b, a })
            }
            _ => None,
        }
    }

    /// Linear blend toward `other` by `t` in `[0.0, 1.0]`.
    pub fn mix(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| {
            (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
        };
        Self {
            r: m(self.r, other.r),
            g: m(self.g, other.g),
            b: m(self.b, other.b),
            a: m(self.a, other.a),
        }
    }

    /// Perceived luminance, 0.0 (black) to 1.0 (white). Uses Rec. 709
    /// weights, which is close enough for deciding foreground contrast.
    pub fn luminance(self) -> f32 {
        (0.2126 * self.r as f32 + 0.7152 * self.g as f32 + 0.0722 * self.b as f32) / 255.0
    }

    /// Pick whichever of `dark` or `light` contrasts more with `self`.
    pub fn contrasting(self, dark: Self, light: Self) -> Self {
        let ld = (self.luminance() - dark.luminance()).abs();
        let ll = (self.luminance() - light.luminance()).abs();
        if ld >= ll { dark } else { light }
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Composited over `bg`, discarding this colour's alpha. The result
    /// is opaque, which is what the renderer wants.
    pub fn over(self, bg: Self) -> Self {
        if self.a == 255 {
            return self;
        }
        let af = self.a as f32 / 255.0;
        let m = |s: u8, d: u8| (s as f32 * af + d as f32 * (1.0 - af)).round() as u8;
        Self {
            r: m(self.r, bg.r),
            g: m(self.g, bg.g),
            b: m(self.b, bg.b),
            a: 255,
        }
    }
}

// The `Rgba` -> `egui::Color32` conversion deliberately lives in
// `mist-ui`, not here: `mist-core` carries no GUI dependency and must
// stay usable from a headless client or a server-side tool.

// ---------------------------------------------------------------------------
// The world's palette
// ---------------------------------------------------------------------------

/// The conventional xterm/VGA 16-colour palette, used as the world's own
/// palette under [`Precedence::AnsiPriority`].
///
/// These are the classic xterm values (the ones `xterm` itself ships in
/// `XTerm-col.ad`/`rgb.txt`), not the Linux console's or Windows
/// console's variants. A MUD that emits SGR 31 means "the red slot of a
/// terminal like the one its players use", so the widely-deployed xterm
/// set is the least surprising choice.
pub const STANDARD_ANSI: [Rgba; 16] = [
    Rgba::rgb(0x00, 0x00, 0x00), // 0 black
    Rgba::rgb(0xcd, 0x00, 0x00), // 1 red
    Rgba::rgb(0x00, 0xcd, 0x00), // 2 green
    Rgba::rgb(0xcd, 0xcd, 0x00), // 3 yellow
    Rgba::rgb(0x00, 0x00, 0xee), // 4 blue
    Rgba::rgb(0xcd, 0x00, 0xcd), // 5 magenta
    Rgba::rgb(0x00, 0xcd, 0xcd), // 6 cyan
    Rgba::rgb(0xe5, 0xe5, 0xe5), // 7 white
    Rgba::rgb(0x7f, 0x7f, 0x7f), // 8 bright black
    Rgba::rgb(0xff, 0x00, 0x00), // 9 bright red
    Rgba::rgb(0x00, 0xff, 0x00), // 10 bright green
    Rgba::rgb(0xff, 0xff, 0x00), // 11 bright yellow
    Rgba::rgb(0x5c, 0x5c, 0xff), // 12 bright blue
    Rgba::rgb(0xff, 0x00, 0xff), // 13 bright magenta
    Rgba::rgb(0x00, 0xff, 0xff), // 14 bright cyan
    Rgba::rgb(0xff, 0xff, 0xff), // 15 bright white
];

/// The standard ANSI slot at `i` (0-15). Out-of-range indices fall back
/// to the default foreground slot rather than panicking, matching
/// [`Theme::ansi`].
pub fn standard_ansi(i: usize) -> Rgba {
    STANDARD_ANSI.get(i).copied().unwrap_or(STANDARD_ANSI[7])
}

/// The six levels of the standard xterm 6x6x6 colour cube.
const CUBE_RAMP: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The standard xterm 256-colour palette: 0-15 are [`STANDARD_ANSI`],
/// 16-231 are the 6x6x6 cube, 232-255 are the 24-step greyscale ramp.
pub fn standard_256(n: u8) -> Rgba {
    match n {
        0..=15 => standard_ansi(n as usize),
        16..=231 => {
            let n = n - 16;
            let r = CUBE_RAMP[(n / 36) as usize];
            let g = CUBE_RAMP[((n / 6) % 6) as usize];
            let b = CUBE_RAMP[(n % 6) as usize];
            Rgba::rgb(r, g, b)
        }
        232..=255 => {
            let v = 8 + (n as u16 - 232) * 10;
            Rgba::rgb(v as u8, v as u8, v as u8)
        }
    }
}

/// Squared sRGB distance. Cheap, monotonic with perceptual distance
/// enough for palette snapping, and avoids pulling in a colour crate.
fn dist2(a: Rgba, b: Rgba) -> u32 {
    let d = |x: u8, y: u8| {
        let d = x as i32 - y as i32;
        (d * d) as u32
    };
    d(a.r, b.r) + d(a.g, b.g) + d(a.b, b.b)
}

/// The standard 256-colour entry nearest to `c` by squared sRGB
/// distance. Used by [`Precedence::Strict`] to approximate truecolor
/// through the theme's 256-colour palette.
pub fn nearest_standard_256(c: Rgba) -> u8 {
    (0u16..=255)
        .min_by_key(|n| dist2(c, standard_256(*n as u8)))
        .unwrap_or(0) as u8
}

// ---------------------------------------------------------------------------
// Native format
// ---------------------------------------------------------------------------

/// A colour role with its paired foreground.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Role {
    /// The colour itself.
    pub value: Option<String>,
    /// Legible foreground when painted on `value`.
    #[serde(rename = "on")]
    pub on: Option<String>,
}

impl Role {
    pub fn hex(value: &str) -> Self {
        Self { value: Some(value.into()), on: None }
    }

    pub fn pair(value: &str, on: &str) -> Self {
        Self { value: Some(value.into()), on: Some(on.into()) }
    }
}

/// The eight ANSI terminal colours plus their bright variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Ansi16 {
    pub black: Option<String>,
    pub red: Option<String>,
    pub green: Option<String>,
    pub yellow: Option<String>,
    pub blue: Option<String>,
    pub magenta: Option<String>,
    pub cyan: Option<String>,
    pub white: Option<String>,
}

/// A theme exactly as written in the file. Everything is optional except
/// `[ansi]`; derivation happens in [`Theme::resolve`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThemeFile {
    #[serde(default)]
    pub primary: Role,
    #[serde(default)]
    pub secondary: Role,
    #[serde(default)]
    pub tertiary: Role,
    #[serde(default)]
    pub error: Role,
    #[serde(default)]
    pub success: Role,
    #[serde(default)]
    pub warning: Role,
    #[serde(default)]
    pub surface: Role,
    #[serde(default)]
    pub surface_alt: Role,
    #[serde(default)]
    pub surface_raised: Role,
    #[serde(default)]
    pub outline: Role,
    #[serde(default)]
    pub outline_variant: Role,
    #[serde(default)]
    pub accent: Role,
    #[serde(default)]
    pub dim: Role,
    #[serde(default)]
    pub selection: Role,
    #[serde(default)]
    pub ansi: Ansi16,
    /// Optional 256-colour cube approximation for `38;5;N`.
    #[serde(default)]
    pub ansi_ext: BTreeMap<String, String>,
    #[serde(default)]
    pub cursor: Role,
}

// ---------------------------------------------------------------------------
// Resolved theme
// ---------------------------------------------------------------------------

/// A fully-resolved theme. Every field is populated; nothing is
/// optional. Produced by [`ThemeFile::resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub source: ThemeSource,

    pub primary: Rgba,
    pub primary_on: Rgba,
    pub secondary: Rgba,
    pub secondary_on: Rgba,
    pub tertiary: Rgba,
    pub error: Rgba,
    pub error_on: Rgba,
    pub success: Rgba,
    pub warning: Rgba,

    pub surface: Rgba,
    pub on_surface: Rgba,
    pub surface_alt: Rgba,
    pub surface_raised: Rgba,
    pub outline: Rgba,
    pub outline_variant: Rgba,
    pub accent: Rgba,
    pub dim: Rgba,

    pub selection_bg: Rgba,
    pub selection_fg: Rgba,

    pub cursor: Rgba,

    pub ansi: [Rgba; 16],
}

/// Where a theme came from, for the picker's grouping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSource {
    Builtin(&'static str),
    File(PathBuf),
}

// Fallbacks for a theme that specifies almost nothing. Deliberately
// neutral so an empty file still produces a legible dark UI.
mod fallback {
    use super::Rgba;
    pub const SURFACE: Rgba = Rgba::rgb(0x17, 0x13, 0x0b);
    pub const ON_SURFACE: Rgba = Rgba::rgb(0xec, 0xe1, 0xd4);
    pub const ANSI: [Rgba; 16] = [
        Rgba::rgb(0x4e, 0x46, 0x39),
        Rgba::rgb(0xff, 0xb4, 0xab),
        Rgba::rgb(0xa6, 0xd9, 0x6a),
        Rgba::rgb(0xee, 0xbf, 0x6d),
        Rgba::rgb(0xb4, 0xce, 0xa6),
        Rgba::rgb(0xda, 0xc3, 0xa1),
        Rgba::rgb(0x8e, 0xc7, 0xc7),
        Rgba::rgb(0xec, 0xe1, 0xd4),
        Rgba::rgb(0x9a, 0x8f, 0x80),
        Rgba::rgb(0xff, 0xb4, 0xab),
        Rgba::rgb(0xa6, 0xd9, 0x6a),
        Rgba::rgb(0xee, 0xbf, 0x6d),
        Rgba::rgb(0xb4, 0xce, 0xa6),
        Rgba::rgb(0xda, 0xc3, 0xa1),
        Rgba::rgb(0x8e, 0xc7, 0xc7),
        Rgba::rgb(0xec, 0xe1, 0xd4),
    ];
}

/// Parse a colour slot, falling back to a derived or default value.
fn pick(slot: &Role, derive: impl FnOnce() -> Rgba) -> Rgba {
    slot.value
        .as_deref()
        .and_then(Rgba::parse)
        .or_else(|| slot.value.as_deref().filter(|s| s.trim().is_empty()).map(|_| Rgba::TRANSPARENT))
        .unwrap_or_else(derive)
}

/// Parse the paired `on` foreground. When absent, choose whichever of
/// near-black or near-white contrasts better with the background — which
/// is what Material 3 does for roles that don't pin their own contrast.
fn pick_on(slot: &Role, bg: Rgba, derive: impl FnOnce() -> Rgba) -> Rgba {
    slot.on
        .as_deref()
        .and_then(Rgba::parse)
        .unwrap_or_else(|| bg.contrasting(Rgba::rgb(0x00, 0x00, 0x00), derive()))
}

impl ThemeFile {
    /// Fill in every omitted token. Pure function of the file contents.
    pub fn resolve(self, name: impl Into<String>, source: ThemeSource) -> Theme {
        let name = name.into();
        let ansi_src = &self.ansi;

        // The ANSI 16 is the base everything else derives from. Omitted
        // entries fall back to the neutral default palette; bright slots
        // derive from their normal counterparts when absent.
        let mut ansi = fallback::ANSI;
        let normal_slots: [(&Option<String>, usize); 8] = [
            (&ansi_src.black, 0),
            (&ansi_src.red, 1),
            (&ansi_src.green, 2),
            (&ansi_src.yellow, 3),
            (&ansi_src.blue, 4),
            (&ansi_src.magenta, 5),
            (&ansi_src.cyan, 6),
            (&ansi_src.white, 7),
        ];
        for (slot, idx) in normal_slots {
            if let Some(c) = slot.as_deref().and_then(Rgba::parse) {
                ansi[idx] = c;
            }
        }
        // Bright slots fall back to their normal counterparts.
        let bright_base = [
            ansi[0], ansi[1], ansi[2], ansi[3], ansi[4], ansi[5], ansi[6], ansi[7],
        ];
        for i in 0..8 {
            ansi[8 + i] = fallback::ANSI[8 + i].mix(bright_base[i], 0.25);
        }

        let surface = pick(&self.surface, || fallback::SURFACE);
        let on_surface = pick_on(&self.surface, surface, || fallback::ON_SURFACE);

        let primary = pick(&self.primary, || ansi[3]);
        let primary_on = pick_on(&self.primary, primary, || fallback::ON_SURFACE);

        let secondary = pick(&self.secondary, || ansi[6]);
        let secondary_on = pick_on(&self.secondary, secondary, || fallback::ON_SURFACE);
        let tertiary = pick(&self.tertiary, || ansi[5]);

        let error = pick(&self.error, || ansi[1]);
        let error_on = pick_on(&self.error, error, || fallback::ON_SURFACE);
        let success = pick(&self.success, || ansi[2]);
        let warning = pick(&self.warning, || ansi[3]);

        // Authorable, but derived at 20% when absent. See UI-REQS.md §1.4:
        // every surveyed app derives borders from named roles.
        let outline = pick(&self.outline, || surface.mix(on_surface, 0.20));
        let outline_variant = pick(&self.outline_variant, || surface.mix(on_surface, 0.10));
        let surface_alt = pick(&self.surface_alt, || surface.mix(on_surface, 0.06));
        let surface_raised = pick(&self.surface_raised, || surface.mix(on_surface, 0.12));
        let dim = pick(&self.dim, || on_surface.mix(surface, 0.45));
        let accent = pick(&self.accent, || primary);

        let selection_bg = pick(&self.selection, || surface.mix(on_surface, 0.30));
        let selection_fg = pick_on(&self.selection, selection_bg, || on_surface);

        let cursor = pick(&self.cursor, || on_surface);

        Theme {
            name,
            source,
            primary,
            primary_on,
            secondary,
            secondary_on,
            tertiary,
            error,
            error_on,
            success,
            warning,
            surface,
            on_surface,
            surface_alt,
            surface_raised,
            outline,
            outline_variant,
            accent,
            dim,
            selection_bg,
            selection_fg,
            cursor,
            ansi,
        }
    }
}

impl Theme {
    /// The theme's ANSI palette at index `i` (0-15).
    pub fn ansi(&self, i: usize) -> Rgba {
        self.ansi.get(i).copied().unwrap_or(self.on_surface)
    }

    /// The 256-colour cube. Authored values win; everything else is
    /// derived from the 16 via the standard xterm cube and greyscale
    /// ramps, which is what every terminal emulator does.
    pub fn colour_256(&self, n: u8) -> Rgba {
        match n {
            0..=15 => self.ansi(n as usize),
            16..=231 => {
                const RAMP: [u8; 6] = [0, 95, 135, 175, 215, 255];
                let n = n - 16;
                let r = RAMP[(n / 36) as usize];
                let g = RAMP[((n / 6) % 6) as usize];
                let b = RAMP[(n % 6) as usize];
                Rgba::rgb(r, g, b)
            }
            232..=255 => {
                let v = 8 + (n as u16 - 232) * 10;
                Rgba::rgb(v as u8, v as u8, v as u8)
            }
        }
    }

    /// Paint a colour the world sent, honouring the theme's precedence
    /// mode.
    ///
    /// The modes exist because an *index* is a reference into a palette
    /// while a truecolor triple is a colour in itself. Only references
    /// are remapped, except under [`Precedence::Strict`], which forces
    /// even literal colours through the theme. See `UI-REQS.md` §1.6.
    pub fn paint_world_colour(&self, mode: Precedence, world: WorldColour) -> Rgba {
        match mode {
            Precedence::AnsiPriority => match world {
                // The world's own palette, not the theme's.
                WorldColour::Indexed(i) => standard_ansi(i),
                WorldColour::Cube(n) => standard_256(n),
                WorldColour::True(r, g, b) => Rgba::rgb(r, g, b),
            },
            Precedence::ThemePriority => match world {
                WorldColour::Indexed(i) => self.ansi(i),
                WorldColour::Cube(n) => self.themed_cube(n),
                WorldColour::True(r, g, b) => Rgba::rgb(r, g, b),
            },
            Precedence::Strict => match world {
                WorldColour::Indexed(i) => self.ansi(i),
                WorldColour::Cube(n) => self.colour_256(n),
                WorldColour::True(r, g, b) => {
                    self.colour_256(nearest_standard_256(Rgba::rgb(r, g, b)))
                }
            },
        }
    }

    /// Map a 256-colour index onto the theme's own palette.
    ///
    /// Indices 0-15 *are* the theme's authored ANSI slots, so they map
    /// one-to-one. For 16-255 a theme only owns its 16 authored colours;
    /// the rest of its cube is the standard xterm derivation. So the rule
    /// is comparative: if one of the theme's 16 colours is nearer to the
    /// requested cube colour than the nearest *standard* ANSI colour is,
    /// the theme has expressed a better version of that hue and we use
    /// it. Otherwise the theme has no opinion about that region and the
    /// literal cube entry stands.
    ///
    /// This lets a theme recolor a coherent subset (e.g. its red at
    /// `#ff5555` catches cube entries in the red ramp) without mangling
    /// 256-colour art into sixteen flat slots.
    fn themed_cube(&self, n: u8) -> Rgba {
        if n <= 15 {
            return self.ansi(n as usize);
        }
        let literal = standard_256(n);
        let theme_best = (0..16)
            .map(|i| self.ansi(i))
            .min_by_key(|c| dist2(*c, literal))
            .unwrap_or(literal);
        let standard_best = (0..16)
            .map(|i| STANDARD_ANSI[i])
            .min_by_key(|c| dist2(*c, literal))
            .unwrap_or(literal);
        if dist2(theme_best, literal) < dist2(standard_best, literal) {
            theme_best
        } else {
            literal
        }
    }
}

/// How world-supplied colours interact with the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Precedence {
    /// The world's own colours win.
    #[default]
    AnsiPriority,
    /// The theme's palette is authoritative for indexed references.
    ThemePriority,
    /// Everything, including truecolor, is forced through the theme.
    Strict,
}

/// A colour as the world expressed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldColour {
    /// SGR 30-37 / 90-97: an ANSI palette index.
    Indexed(usize),
    /// SGR 38;5;N: a 256-colour cube index.
    Cube(u8),
    /// SGR 38;2;R;G;B: literal truecolor.
    True(u8, u8, u8),
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ThemeError {
    Io(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ThemeError {}

impl From<std::io::Error> for ThemeError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Load a theme from a file, choosing the parser by content sniffing
/// rather than extension, so a `.conf` Kitty file and a `.toml`
/// Alacritty file both work.
pub fn load_file(path: &Path) -> Result<Theme, ThemeError> {
    let text = std::fs::read_to_string(path)?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("theme")
        .to_string();
    let source = ThemeSource::File(path.to_path_buf());

    // Sniff in order of specificity.
    if text.contains("[colors.normal]") || text.contains("[colors.primary]") {
        return Ok(parse_alacritty(&text, name, source));
    }
    if text.contains("[colors]") && text.contains(":h:") {
        return Ok(parse_kcolorscheme(&text, name, source));
    }
    if text.contains("palette = ") || text.contains("background = ") {
        return Ok(parse_ghostty(&text, name, source));
    }
    if text.contains("color0") && text.contains("foreground") {
        return Ok(parse_kitty(&text, name, source));
    }
    // Otherwise: native.
    match toml::from_str::<ThemeFile>(&text) {
        Ok(f) => Ok(f.resolve(name, source)),
        Err(e) => Err(ThemeError::Parse(format!("{e}"))),
    }
}

fn resolve_hex(s: &str) -> Rgba {
    Rgba::parse(s.trim()).unwrap_or(Rgba::TRANSPARENT)
}

// ---------------------------------------------------------------------------
// Importers
// ---------------------------------------------------------------------------

/// Alacritty TOML:
/// `[colors.normal] black = '#…'` …, `[colors.primary] background/foreground`.
fn parse_alacritty(text: &str, name: String, source: ThemeSource) -> Theme {
    let mut f = ThemeFile::default();
    let mut section: Option<&'static str> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            let s = line.trim_matches(['[', ']']);
            section = match s {
                "colors.normal" => Some("normal"),
                "colors.bright" => Some("bright"),
                "colors.primary" => Some("primary"),
                "colors.cursor" => Some("cursor"),
                "colors.selection" => Some("selection"),
                _ => None,
            };
            continue;
        }
        let Some((key, val)) = line.split_once('=') else { continue };
        let key = key.trim();
        let val = val.trim().trim_matches('\'').trim_matches('"');

        match (section, key) {
            (Some("normal"), k) | (Some("bright"), k) if !k.is_empty() => {
                let c = val.to_string();
                match k {
                    "black" => f.ansi.black = Some(c),
                    "red" => f.ansi.red = Some(c),
                    "green" => f.ansi.green = Some(c),
                    "yellow" => f.ansi.yellow = Some(c),
                    "blue" => f.ansi.blue = Some(c),
                    "magenta" => f.ansi.magenta = Some(c),
                    "cyan" => f.ansi.cyan = Some(c),
                    "white" => f.ansi.white = Some(c),
                    _ => {}
                }
            }
            (Some("primary"), "background") => f.surface.value = Some(val.into()),
            (Some("primary"), "foreground") => f.surface.on = Some(val.into()),
            (Some("cursor"), "cursor") => f.cursor.value = Some(val.into()),
            (Some("selection"), "background") => f.selection.value = Some(val.into()),
            (Some("selection"), "text") | (Some("selection"), "foreground") => {
                f.selection.on = Some(val.into())
            }
            _ => {}
        }
    }
    f.resolve(name, source)
}

/// Ghostty: `palette = 0=#rrggbb` …, flat `background = #…`.
fn parse_ghostty(text: &str, name: String, source: ThemeSource) -> Theme {
    let mut f = ThemeFile::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') && !line.contains('=') {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else { continue };
        let key = key.trim();
        let val = val.trim();
        if key == "palette" {
            if let Some((i, hex)) = val.split_once('=') {
                if let Ok(i) = i.trim().parse::<usize>() {
                    if i < 8 {
                        let c = hex.trim().to_string();
                        match i {
                            0 => f.ansi.black = Some(c),
                            1 => f.ansi.red = Some(c),
                            2 => f.ansi.green = Some(c),
                            3 => f.ansi.yellow = Some(c),
                            4 => f.ansi.blue = Some(c),
                            5 => f.ansi.magenta = Some(c),
                            6 => f.ansi.cyan = Some(c),
                            _ => f.ansi.white = Some(c),
                        }
                    }
                }
            }
            continue;
        }
        let c = val.to_string();
        match key {
            "background" => f.surface.value = Some(c),
            "foreground" => f.surface.on = Some(c),
            "cursor-color" => f.cursor.value = Some(c),
            "selection-background" => f.selection.value = Some(c),
            "selection-foreground" => f.selection.on = Some(c),
            _ => {}
        }
    }
    f.resolve(name, source)
}

/// Kitty: `color0 #…` … `color15`, plus named keys.
fn parse_kitty(text: &str, name: String, source: ThemeSource) -> Theme {
    let mut f = ThemeFile::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else { continue };
        let Some(val) = parts.next() else { continue };
        let val = val.trim_matches(';').trim();
        let c = val.to_string();

        match key {
            "color0" => f.ansi.black = Some(c),
            "color1" => f.ansi.red = Some(c),
            "color2" => f.ansi.green = Some(c),
            "color3" => f.ansi.yellow = Some(c),
            "color4" => f.ansi.blue = Some(c),
            "color5" => f.ansi.magenta = Some(c),
            "color6" => f.ansi.cyan = Some(c),
            "color7" => f.ansi.white = Some(c),
            "background" => f.surface.value = Some(c),
            "foreground" => f.surface.on = Some(c),
            "cursor" | "cursor_color" => f.cursor.value = Some(c),
            "selection_background" => f.selection.value = Some(c),
            "selection_foreground" => f.selection.on = Some(c),
            _ => {}
        }
    }
    f.resolve(name, source)
}

/// KColorscheme JSON. Parsed with a hand-rolled reader because the shape
/// is a flat object of `"key": "#hex"` and we don't want a serde_json
/// dependency in the hot path.
fn parse_kcolorscheme(text: &str, name: String, source: ThemeSource) -> Theme {
    let mut f = ThemeFile::default();
    // Strip JSON syntax down to key/value pairs we recognise.
    for entry in text.split('{').skip(1) {
        for pair in entry.split(',') {
            let Some((k, v)) = pair.split_once(':') else { continue };
            let k = k.trim().trim_matches('"');
            let v = v.trim().trim_matches('"').trim();
            if !v.starts_with('#') {
                continue;
            }
            let c = v.to_string();
            match k {
                "color0" => f.ansi.black = Some(c),
                "color1" => f.ansi.red = Some(c),
                "color2" => f.ansi.green = Some(c),
                "color3" => f.ansi.yellow = Some(c),
                "color4" => f.ansi.blue = Some(c),
                "color5" => f.ansi.magenta = Some(c),
                "color6" => f.ansi.cyan = Some(c),
                "color7" => f.ansi.white = Some(c),
                "background" => f.surface.value = Some(c),
                "foreground" => f.surface.on = Some(c),
                "selectionBackground" => f.selection.value = Some(c),
                "selectionForeground" => f.selection.on = Some(c),
                "cursorColor" => f.cursor.value = Some(c),
                _ => {}
            }
        }
    }
    f.resolve(name, source)
}

// ---------------------------------------------------------------------------
// Built-ins
// ---------------------------------------------------------------------------

/// Built-in themes, compiled in. Kept as source strings so they're
/// editable without a codegen step.
pub fn builtins() -> Vec<(&'static str, &'static str)> {
    vec![
        ("wyldware", WYLDWARE_TOML),
        ("dracula", DRACULA_TOML),
        ("nord", NORD_TOML),
        ("gruvbox-dark", GRUVBOX_DARK_TOML),
    ]
}

/// Parse a built-in by name.
pub fn builtin(name: &str) -> Option<Theme> {
    let src = builtins().into_iter().find(|(n, _)| *n == name).map(|(_, s)| s)?;
    let f: ThemeFile = toml::from_str(src).ok()?;
    Some(f.resolve(name.to_string(), ThemeSource::Builtin("builtin")))
}

const WYLDWARE_TOML: &str = r##"
[primary]
value = "#eebf6d"
on = "#422c00"

[secondary]
value = "#dac3a1"
on = "#3c2e16"

[error]
value = "#ffb4ab"
on = "#690005"

[success]
value = "#a6d96a"
on = "#1e3d0f"

[surface]
value = "#17130b"
on = "#ece1d4"

[accent]
value = "#eebf6d"

[outline]
value = "#4e4639"

[selection]
value = "#4e4639"
on = "#d1c5b4"

[ansi]
black   = "#4e4639"
red     = "#ffb4ab"
green   = "#a6d96a"
yellow  = "#eebf6d"
blue    = "#b4cea6"
magenta = "#dac3a1"
cyan    = "#8ec7c7"
white   = "#ece1d4"
"##;

const DRACULA_TOML: &str = r##"
[primary]
value = "#bd93f9"
on = "#1e1f29"

[secondary]
value = "#8be9fd"
on = "#1e1f29"

[error]
value = "#ff5555"
on = "#1e1f29"

[success]
value = "#50fa7b"
on = "#1e1f29"

[surface]
value = "#282a36"
on = "#f8f8f2"

[outline]
value = "#44475a"

[ansi]
black   = "#21222c"
red     = "#ff5555"
green   = "#50fa7b"
yellow  = "#f1fa8c"
blue    = "#bd93f9"
magenta = "#ff79c6"
cyan    = "#8be9fd"
white   = "#f8f8f2"
"##;

const NORD_TOML: &str = r##"
[primary]
value = "#88c0d0"
on = "#2e3440"

[secondary]
value = "#a3be8c"
on = "#2e3440"

[error]
value = "#bf616a"
on = "#2e3440"

[success]
value = "#a3be8c"
on = "#2e3440"

[surface]
value = "#2e3440"
on = "#d8dee9"

[outline]
value = "#4c566a"

[ansi]
black   = "#3b4252"
red     = "#bf616a"
green   = "#a3be8c"
yellow  = "#ebcb8b"
blue    = "#81a1c1"
magenta = "#b48ead"
cyan    = "#88c0d0"
white   = "#e5e9f0"
"##;

const GRUVBOX_DARK_TOML: &str = r##"
[primary]
value = "#fabd2f"
on = "#282828"

[secondary]
value = "#8ec07c"
on = "#282828"

[error]
value = "#fb4934"
on = "#282828"

[success]
value = "#b8bb26"
on = "#282828"

[surface]
value = "#282828"
on = "#ebdbb2"

[outline]
value = "#504945"

[ansi]
black   = "#282828"
red     = "#cc241d"
green   = "#98971a"
yellow  = "#d79921"
blue    = "#458588"
magenta = "#b16286"
cyan    = "#689d6a"
white   = "#a89984"
"##;