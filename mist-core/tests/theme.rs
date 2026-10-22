//! Tests for the theme model, derivation rules, and importers.

use mist_core::theme::{
    builtin, load_file, Precedence, Rgba, ThemeFile, ThemeSource, WorldColour,
};

// ---------------------------------------------------------------------------
// Colour arithmetic
// ---------------------------------------------------------------------------

#[test]
fn parses_hex_forms() {
    assert_eq!(Rgba::parse("#ff0000"), Some(Rgba::rgb(255, 0, 0)));
    assert_eq!(Rgba::parse("ff0000"), Some(Rgba::rgb(255, 0, 0)));
    // Short form expands each nibble.
    assert_eq!(Rgba::parse("#f00"), Some(Rgba::rgb(255, 0, 0)));
    // Eight digits carry alpha.
    assert_eq!(Rgba::parse("#ff000080"), Some(Rgba { r: 255, g: 0, b: 0, a: 128 }));
    assert_eq!(Rgba::parse("nonsense"), None);
    assert_eq!(Rgba::parse("#ff00"), None);
}

#[test]
fn mix_interpolates() {
    let black = Rgba::rgb(0, 0, 0);
    let white = Rgba::rgb(255, 255, 255);
    assert_eq!(black.mix(white, 0.0), black);
    assert_eq!(black.mix(white, 1.0), white);
    let mid = black.mix(white, 0.5);
    assert_eq!(mid, Rgba::rgb(128, 128, 128));
}

#[test]
fn contrasting_picks_the_farther_legible_option() {
    let white = Rgba::rgb(255, 255, 255);
    let black = Rgba::rgb(0, 0, 0);
    // On a light background, dark text wins.
    assert_eq!(white.contrasting(black, white), black);
    // On a dark background, light text wins.
    let dark = Rgba::rgb(0x17, 0x13, 0x0b);
    assert_eq!(dark.contrasting(black, white), white);
}

#[test]
fn over_composites_alpha_onto_background() {
    let bg = Rgba::rgb(0, 0, 0);
    let half_white = Rgba { r: 255, g: 255, b: 255, a: 128 };
    let out = half_white.over(bg);
    assert_eq!(out.a, 255);
    assert_eq!(out.r, 128);
}

// ---------------------------------------------------------------------------
// Derivation
// ---------------------------------------------------------------------------

#[test]
fn empty_file_still_produces_a_legible_theme() {
    let t = ThemeFile::default().resolve("empty", ThemeSource::Builtin("test"));
    // Surface and text must differ enough to be readable.
    assert!(
        (t.surface.luminance() - t.on_surface.luminance()).abs() > 0.5,
        "empty theme should still be legible, got surface={:?} on={:?}",
        t.surface,
        t.on_surface
    );
    // Nothing may end up transparent, or the UI disappears.
    for (name, c) in [
        ("surface", t.surface),
        ("on_surface", t.on_surface),
        ("outline", t.outline),
        ("accent", t.accent),
        ("error", t.error),
    ] {
        assert_eq!(c.a, 255, "{name} should be opaque, got {c:?}");
    }
}

#[test]
fn missing_outline_derives_from_surface() {
    // UI-REQS.md §1.4: outline is authorable but derived at 20% when absent.
    let t = ThemeFile::default().resolve("d", ThemeSource::Builtin("test"));
    let expected = t.surface.mix(t.on_surface, 0.20);
    assert_eq!(t.outline, expected, "outline should be surface.mix(on_surface, 0.20)");
}

#[test]
fn authored_outline_wins_over_derivation() {
    let f: ThemeFile = toml::from_str(
        r##"
[outline]
value = "#ff00ff"
[surface]
value = "#000000"
on = "#ffffff"
"##,
    )
    .unwrap();
    let t = f.resolve("authored", ThemeSource::Builtin("test"));
    assert_eq!(t.outline, Rgba::rgb(255, 0, 255));
}

#[test]
fn missing_on_falls_back_to_contrast() {
    // A role with a value but no `on` picks whichever of black or white
    // is more legible against the value.
    let f: ThemeFile = toml::from_str(
        r##"
[primary]
value = "#ffffff"
"##,
    )
    .unwrap();
    let t = f.resolve("p", ThemeSource::Builtin("test"));
    // White primary -> black text.
    assert!(t.primary_on.luminance() < 0.5, "got {:?}", t.primary_on);

    let f: ThemeFile = toml::from_str(
        r##"
[primary]
value = "#000000"
"##,
    )
    .unwrap();
    let t = f.resolve("p2", ThemeSource::Builtin("test"));
    // Black primary -> white text.
    assert!(t.primary_on.luminance() > 0.5, "got {:?}", t.primary_on);
}

// ---------------------------------------------------------------------------
// ANSI palette
// ---------------------------------------------------------------------------

#[test]
fn ansi_slots_are_read_from_the_file() {
    let f: ThemeFile = toml::from_str(
        r##"
[ansi]
black   = "#010101"
red     = "#020202"
green   = "#030303"
yellow  = "#040404"
blue    = "#050505"
magenta = "#060606"
cyan    = "#070707"
white   = "#080808"
"##,
    )
    .unwrap();
    let t = f.resolve("a", ThemeSource::Builtin("test"));
    assert_eq!(t.ansi(0), Rgba::rgb(1, 1, 1));
    assert_eq!(t.ansi(1), Rgba::rgb(2, 2, 2));
    assert_eq!(t.ansi(7), Rgba::rgb(8, 8, 8));
}

#[test]
fn colour_256_cube_is_derived_correctly() {
    let t = ThemeFile::default().resolve("c", ThemeSource::Builtin("test"));
    // 16 is the first cube entry: black.
    assert_eq!(t.colour_256(16), Rgba::rgb(0, 0, 0));
    // 196 is pure red in the standard cube.
    assert_eq!(t.colour_256(196), Rgba::rgb(255, 0, 0));
    // 231 is white.
    assert_eq!(t.colour_256(231), Rgba::rgb(255, 255, 255));
    // 232 is the start of the greyscale ramp.
    assert_eq!(t.colour_256(232), Rgba::rgb(8, 8, 8));
    assert_eq!(t.colour_256(255), Rgba::rgb(238, 238, 238));
}

// ---------------------------------------------------------------------------
// Precedence — UI-REQS.md §1.6
// ---------------------------------------------------------------------------

#[test]
fn truecolor_passes_through_in_every_mode() {
    let t = ThemeFile::default().resolve("t", ThemeSource::Builtin("test"));
    for mode in [Precedence::AnsiPriority, Precedence::ThemePriority] {
        let painted = t.paint_world_colour(mode, WorldColour::True(0x8a, 0x2b, 0xe2));
        assert_eq!(
            painted,
            Rgba::rgb(0x8a, 0x2b, 0xe2),
            "truecolor must pass through under {mode:?}"
        );
    }
}

#[test]
fn indexed_colours_use_the_theme_palette() {
    let f: ThemeFile = toml::from_str(
        r##"
[ansi]
red = "#ff0000"
"##,
    )
    .unwrap();
    let t = f.resolve("i", ThemeSource::Builtin("test"));
    let painted = t.paint_world_colour(Precedence::AnsiPriority, WorldColour::Indexed(1));
    assert_eq!(painted, Rgba::rgb(255, 0, 0));
}

// ---------------------------------------------------------------------------
// Built-ins
// ---------------------------------------------------------------------------

#[test]
fn every_builtin_parses_and_is_legible() {
    for (name, src) in mist_core::theme::builtins() {
        let f: ThemeFile = toml::from_str(src)
            .unwrap_or_else(|e| panic!("builtin {name} failed to parse: {e}"));
        let t = f.resolve(name, ThemeSource::Builtin("builtin"));
        assert!(
            (t.surface.luminance() - t.on_surface.luminance()).abs() > 0.5,
            "builtin {name} is not legible"
        );
    }
}

#[test]
fn builtin_lookup_by_name() {
    assert!(builtin("dracula").is_some());
    assert!(builtin("nord").is_some());
    assert!(builtin("gruvbox-dark").is_some());
    assert!(builtin("wyldware").is_some());
    assert!(builtin("no-such-theme").is_none());
}

// ---------------------------------------------------------------------------
// Importers
// ---------------------------------------------------------------------------

#[test]
fn imports_alacritty_format() {
    let dir = std::env::temp_dir().join("mist-theme-test-alacritty");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.toml");
    std::fs::write(
        &path,
        r##"[colors.primary]
background = '#17130b'
foreground = '#ece1d4'

[colors.normal]
black   = '#4e4639'
red     = '#ffb4ab'
green   = '#a6d96a'
yellow  = '#eebf6d'
blue    = '#b4cea6'
magenta = '#dac3a1'
cyan    = '#8ec7c7'
white   = '#ece1d4'
"##,
    )
    .unwrap();

    let t = load_file(&path).expect("should import alacritty");
    assert_eq!(t.name, "test");
    assert_eq!(t.surface, Rgba::rgb(0x17, 0x13, 0x0b));
    assert_eq!(t.on_surface, Rgba::rgb(0xec, 0xe1, 0xd4));
    assert_eq!(t.ansi(1), Rgba::rgb(0xff, 0xb4, 0xab));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn imports_ghostty_format() {
    let dir = std::env::temp_dir().join("mist-theme-test-ghostty");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("noctalia");
    std::fs::write(
        &path,
        "palette = 0=#4e4639\n\
         palette = 1=#ffb4ab\n\
         palette = 2=#eebf6d\n\
         palette = 7=#ece1d4\n\
         background = #17130b\n\
         foreground = #ece1d4\n",
    )
    .unwrap();

    let t = load_file(&path).expect("should import ghostty");
    assert_eq!(t.surface, Rgba::rgb(0x17, 0x13, 0x0b));
    assert_eq!(t.ansi(0), Rgba::rgb(0x4e, 0x46, 0x39));
    assert_eq!(t.ansi(1), Rgba::rgb(0xff, 0xb4, 0xab));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn imports_kitty_format() {
    let dir = std::env::temp_dir().join("mist-theme-test-kitty");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("noctalia.conf");
    std::fs::write(
        &path,
        "color0 #4e4639\n\
         color1 #ffb4ab\n\
         color7 #ece1d4\n\
         background #17130b\n\
         foreground #ece1d4\n",
    )
    .unwrap();

    let t = load_file(&path).expect("should import kitty");
    assert_eq!(t.surface, Rgba::rgb(0x17, 0x13, 0x0b));
    assert_eq!(t.ansi(1), Rgba::rgb(0xff, 0xb4, 0xab));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn native_format_round_trips_through_load_file() {
    let dir = std::env::temp_dir().join("mist-theme-test-native");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mine.toml");
    std::fs::write(
        &path,
        r##"[primary]
value = "#123456"
on    = "#fedcba"

[surface]
value = "#010203"
on    = "#f0f0f0"

[ansi]
black  = "#000000"
red    = "#aa0000"
green  = "#00aa00"
yellow = "#aaaa00"
blue   = "#0000aa"
magenta= "#aa00aa"
cyan   = "#00aaaa"
white  = "#aaaaaa"
"##,
    )
    .unwrap();

    let t = load_file(&path).expect("should load native");
    assert_eq!(t.primary, Rgba::rgb(0x12, 0x34, 0x56));
    assert_eq!(t.primary_on, Rgba::rgb(0xfe, 0xdc, 0xba));
    assert_eq!(t.surface, Rgba::rgb(1, 2, 3));
    assert!(matches!(t.source, ThemeSource::File(_)));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn malformed_native_file_is_an_error_not_a_panic() {
    let dir = std::env::temp_dir().join("mist-theme-test-bad");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("broken.toml");
    std::fs::write(&path, "this is not = valid = toml [[[").unwrap();
    let r = load_file(&path);
    assert!(r.is_err(), "malformed theme should error, got {:?}", r.is_ok());
    std::fs::remove_dir_all(&dir).ok();
}