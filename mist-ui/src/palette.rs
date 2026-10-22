//! Bridge between `mist_core::theme::Theme` and egui.
//!
//! Two consumers, one source of truth:
//!
//!   * **Chrome** — the toolbar, status bar, tab strip, dialogs, input
//!     line. These are painted with egui's `Visuals`, so we project a
//!     `Theme` onto `Visuals` and let egui do the rest.
//!   * **World output** — text the game sends. These are painted by our
//!     own renderer, so they read `Palette` directly rather than going
//!     through egui at all.
//!
//! `mist-core` stays GUI-free; the `Rgba` -> `Color32` conversion lives
//! here.

use mist_core::theme::{Precedence, Rgba, Theme, WorldColour};

// ---------------------------------------------------------------------------
// Rgba -> egui
// ---------------------------------------------------------------------------

pub fn c32(c: Rgba) -> eframe::egui::Color32 {
    eframe::egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

// ---------------------------------------------------------------------------
// Palette — what the renderer reads directly
// ---------------------------------------------------------------------------

/// Flat, cheap-to-clone view of a theme for the render path.
#[derive(Debug, Clone)]
pub struct Palette {
    pub theme: Theme,
}

impl Palette {
    pub fn new(theme: Theme) -> Self {
        Self { theme }
    }

    /// Default text colour on the scrollback background.
    pub fn text(&self) -> eframe::egui::Color32 {
        c32(self.theme.on_surface)
    }

    /// De-emphasised text: timestamps, hints, disabled items.
    pub fn dim(&self) -> eframe::egui::Color32 {
        c32(self.theme.dim)
    }

    /// Background behind the scrollback.
    pub fn surface(&self) -> eframe::egui::Color32 {
        c32(self.theme.surface)
    }

    /// Toolbar / status bar background.
    pub fn surface_alt(&self) -> eframe::egui::Color32 {
        c32(self.theme.surface_alt)
    }

    /// Dialog and panel background.
    pub fn surface_raised(&self) -> eframe::egui::Color32 {
        c32(self.theme.surface_raised)
    }

    pub fn outline(&self) -> eframe::egui::Color32 {
        c32(self.theme.outline)
    }

    pub fn outline_variant(&self) -> eframe::egui::Color32 {
        c32(self.theme.outline_variant)
    }

    /// Links, active tab underline, focus rings.
    pub fn accent(&self) -> eframe::egui::Color32 {
        c32(self.theme.accent)
    }

    pub fn error(&self) -> eframe::egui::Color32 {
        c32(self.theme.error)
    }

    pub fn success(&self) -> eframe::egui::Color32 {
        c32(self.theme.success)
    }

    pub fn warning(&self) -> eframe::egui::Color32 {
        c32(self.theme.warning)
    }

    pub fn selection_bg(&self) -> eframe::egui::Color32 {
        c32(self.theme.selection_bg)
    }

    pub fn selection_fg(&self) -> eframe::egui::Color32 {
        c32(self.theme.selection_fg)
    }

    /// Paint a colour the world asked for, honouring the precedence mode.
    ///
    /// Truecolor passes through untouched in every mode — see
    /// `UI-REQS.md` §1.6.
    pub fn world_colour(&self, mode: Precedence, w: WorldColour) -> eframe::egui::Color32 {
        c32(self.theme.paint_world_colour(mode, w))
    }
}

// ---------------------------------------------------------------------------
// Theme -> egui Visuals
// ---------------------------------------------------------------------------

/// Project a theme onto egui's `Visuals`. Called on theme change, and
/// once at startup.
pub fn apply_visuals(ctx: &eframe::egui::Context, theme: &Theme) {
    let p = Palette::new(theme.clone());
    let mut style = (*ctx.style()).clone();
    let mut v = ctx.style().visuals.clone();

    let rounding: eframe::egui::Rounding = 4.0.into();

    // Chrome surfaces. egui has no `override_background_color`; the
    // window and panel fills are the knobs we have.
    v.window_fill = p.surface_raised();
    v.window_stroke = eframe::egui::Stroke::new(1.0, p.outline());
    v.panel_fill = p.surface_alt();
    v.faint_bg_color = p.surface_alt();
    v.extreme_bg_color = p.surface(); // egui's "dark" sentinel
    v.code_bg_color = p.surface_raised();

    v.override_text_color = Some(p.text());
    v.hyperlink_color = p.accent();
    v.error_fg_color = p.error();
    v.warn_fg_color = p.warning();

    v.selection.bg_fill = p.selection_bg();
    v.selection.stroke = eframe::egui::Stroke::new(1.0, p.selection_fg());

    let noninteractive = &mut v.widgets.noninteractive;
    noninteractive.bg_fill = p.surface_alt();
    noninteractive.weak_bg_fill = p.surface_alt();
    noninteractive.bg_stroke = eframe::egui::Stroke::new(1.0, p.outline_variant());
    noninteractive.fg_stroke = eframe::egui::Stroke::new(1.0, p.dim());
    noninteractive.rounding = rounding;

    let inactive = &mut v.widgets.inactive;
    inactive.bg_fill = p.surface_raised();
    inactive.weak_bg_fill = p.surface_alt();
    inactive.bg_stroke = eframe::egui::Stroke::new(1.0, p.outline());
    inactive.fg_stroke = eframe::egui::Stroke::new(1.0, p.text());
    inactive.rounding = rounding;

    let hovered = &mut v.widgets.hovered;
    hovered.bg_fill = c32(theme.surface_raised.mix(theme.surface, 0.5));
    hovered.weak_bg_fill = p.surface_alt();
    hovered.bg_stroke = eframe::egui::Stroke::new(1.0, p.accent());
    hovered.fg_stroke = eframe::egui::Stroke::new(1.0, p.text());
    hovered.rounding = rounding;

    let active = &mut v.widgets.active;
    active.bg_fill = p.accent();
    active.weak_bg_fill = p.accent();
    active.bg_stroke = eframe::egui::Stroke::new(1.0, p.accent());
    active.fg_stroke = eframe::egui::Stroke::new(1.0, c32(theme.primary_on));
    active.rounding = rounding;

    let open = &mut v.widgets.open;
    open.bg_fill = p.surface_raised();
    open.weak_bg_fill = p.surface_raised();
    open.bg_stroke = eframe::egui::Stroke::new(1.0, p.outline());
    open.fg_stroke = eframe::egui::Stroke::new(1.0, p.text());

    v.striped = false;
    v.dark_mode = theme.surface.luminance() < 0.5;

    style.visuals = v;
    ctx.set_style(style);
}

/// The theme list offered in the picker: built-ins first, then any files
/// found in `dir`.
pub fn discover(dir: &std::path::Path) -> Vec<ThemeEntry> {
    let mut out: Vec<ThemeEntry> = mist_core::theme::builtins()
        .into_iter()
        .map(|(name, _)| ThemeEntry {
            label: name.to_string(),
            path: None,
        })
        .collect();

    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut files: Vec<ThemeEntry> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| {
                let path = e.path();
                let label = path.file_stem()?.to_str()?.to_string();
                Some(ThemeEntry {
                    label,
                    path: Some(path),
                })
            })
            .collect();
        files.sort_by(|a, b| a.label.cmp(&b.label));
        out.extend(files);
    }
    out
}

/// One row in the theme picker.
#[derive(Debug, Clone)]
pub struct ThemeEntry {
    pub label: String,
    /// `None` for a built-in.
    pub path: Option<std::path::PathBuf>,
}

impl ThemeEntry {
    pub fn is_builtin(&self) -> bool {
        self.path.is_none()
    }

    /// Resolve to a concrete theme.
    pub fn load(&self) -> Result<mist_core::theme::Theme, mist_core::theme::ThemeError> {
        match &self.path {
            None => mist_core::theme::builtin(&self.label).ok_or_else(|| {
                mist_core::theme::ThemeError::Parse(format!("no built-in theme {}", self.label))
            }),
            Some(p) => mist_core::theme::load_file(p),
        }
    }
}

// ---------------------------------------------------------------------------
// Context-scoped access
// ---------------------------------------------------------------------------

/// Store the active palette on the egui context so the free render helpers
/// can reach it without threading a reference through every signature.
pub fn set_active(ctx: &eframe::egui::Context, p: Palette) {
    ctx.data_mut(|d| d.insert_temp(active_key(), p));
}

/// Read the active palette. Falls back to the shipped default if unset, so
/// the renderer is never colour-less even before the first frame.
pub fn active(ctx: &eframe::egui::Context) -> Palette {
    ctx.data(|d| d.get_temp::<Palette>(active_key()))
        .unwrap_or_else(|| {
            Palette::new(
                mist_core::theme::builtin("wyldware")
                    .expect("wyldware builtin must always parse"),
            )
        })
}

fn active_key() -> eframe::egui::Id {
    eframe::egui::Id::new("mist.active-palette")
}
