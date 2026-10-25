//! Top-level application state for the egui shell.

use crate::connect::ConnectPrompt;
use crate::session::{DriverRequest, RenderedLine, RuntimeHandle, Tab};
use mist_core::markup::Node;
use mist_core::tintin::State as TinTinState;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

fn load_system_mono_font(ctx: &eframe::egui::Context) -> bool {
    let candidates = [
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = eframe::egui::FontDefinitions::default();
            fonts.font_data.insert(
                "mono".into(),
                eframe::egui::FontData::from_owned(bytes).into(),
            );
            fonts
                .families
                .entry(eframe::egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "mono".into());
            fonts
                .families
                .entry(eframe::egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "mono".into());
            ctx.set_fonts(fonts);
            let mut style = (*ctx.style()).clone();
            let body_size = style
                .text_styles
                .get(&eframe::egui::TextStyle::Body)
                .map(|s| s.size)
                .unwrap_or(14.0);
            let body = eframe::egui::TextStyle::Body.resolve(&style);
            style.text_styles.insert(
                eframe::egui::TextStyle::Body,
                eframe::egui::FontId {
                    size: body_size * 1.4,
                    family: body.family,
                },
            );
            ctx.set_style(style);
            eprintln!("[mist] loaded mono font from {path}");
            return true;
        }
    }
    eprintln!("[mist] no mono font found; using egui defaults");
    false
}

/// The `themes/` directory, next to the config file. Watched for changes
/// per UI-REQS.md 1.5; created on first use so a user can drop a file in.
fn themes_dir() -> std::path::PathBuf {
    let dir = match mist_core::profile::config_path() {
        Ok(cfg) => cfg
            .parent()
            .map(|p| p.join("themes"))
            .unwrap_or_else(|| std::path::PathBuf::from("themes")),
        Err(_) => std::path::PathBuf::from("themes"),
    };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub struct MistApp {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub next_id: usize,
    pub tintin: Arc<Mutex<TinTinState>>,
    pub connect_prompt: Option<ConnectPrompt>,
    pub runtime: RuntimeHandle,
    pub incoming: Arc<Mutex<HashMap<usize, Vec<RenderedLine>>>>,
    pub connect_pending: bool,
    pub connect_error: Option<String>,
    /// Image cache for `<img src="...">` markup. Owned by the egui
    /// thread; the `RuntimeHandle` carries fetch requests across.
    pub images: crate::images::ImageCache,

    /// Active theme. Drives both egui chrome and the world-output paint
    /// path. See `UI-REQS.md` §1.
    pub palette: crate::palette::Palette,
    /// Discovered themes: built-ins first, then files from `themes/`.
    pub theme_entries: Vec<crate::palette::ThemeEntry>,
    /// Index into `theme_entries` of the active theme.
    pub theme_index: usize,
    /// True while the preferences window is open.
    pub show_prefs: bool,
    /// Set when the active theme file failed to load; the previous theme
    /// stays active. See `UI-REQS.md` §1.5.
    pub theme_error: Option<String>,
}

impl MistApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        load_system_mono_font(&cc.egui_ctx);

        // Theme: start from the config-adjacent `themes/` directory,
        // fall back to the shipped default when nothing is usable there.
        let theme_entries = crate::palette::discover(&themes_dir());

        let (palette, theme_index, theme_error) = match theme_entries.first() {
            Some(entry) => match entry.load() {
                Ok(t) => {
                    crate::palette::apply_visuals(&cc.egui_ctx, &t);
                    let p = crate::palette::Palette::new(t);
                    crate::palette::set_active(&cc.egui_ctx, p.clone());
                    (p, 0usize, None)
                }
                Err(e) => {
                    // A broken theme file must not brick the app; fall
                    // back to the shipped default. See UI-REQS.md 1.5.
                    let fallback = mist_core::theme::builtin("wyldware")
                        .expect("wyldware builtin must always parse");
                    crate::palette::apply_visuals(&cc.egui_ctx, &fallback);
                    let p = crate::palette::Palette::new(fallback);
                    crate::palette::set_active(&cc.egui_ctx, p.clone());
                    (
                        p,
                        0usize,
                        Some(format!("{}: {e}", entry.label)),
                    )
                }
            },
            None => {
                let t = mist_core::theme::builtin("wyldware")
                    .expect("wyldware builtin must always parse");
                crate::palette::apply_visuals(&cc.egui_ctx, &t);
                let p = crate::palette::Palette::new(t);
                crate::palette::set_active(&cc.egui_ctx, p.clone());
                (p, 0usize, None)
            }
        };

        let runtime = RuntimeHandle::spawn();
        let me = Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            tintin: Arc::new(Mutex::new(TinTinState::new())),
            connect_prompt: Some(ConnectPrompt::default()),
            runtime,
            incoming: Arc::new(Mutex::new(HashMap::new())),
            images: crate::images::ImageCache::new(),
            palette,
            theme_entries,
            theme_index,
            show_prefs: false,
            theme_error,
            connect_pending: false,
            connect_error: None,
        };
        // Auto-focus the host field on the first frame. The dialog has
        // already been opened via `connect_prompt`.
        let focus_id = eframe::egui::Id::new("mist-connect-host");
        cc.egui_ctx.memory_mut(|m| m.request_focus(focus_id));
        me
    }

    /// Switch to the theme at `index`. A load failure keeps the current
    /// theme and records the error for the picker to display.
    fn select_theme(&mut self, index: usize, ctx: &eframe::egui::Context) {
        let Some(entry) = self.theme_entries.get(index) else {
            return;
        };
        match entry.load() {
            Ok(t) => {
                crate::palette::apply_visuals(ctx, &t);
                let p = crate::palette::Palette::new(t);
                crate::palette::set_active(ctx, p.clone());
                self.palette = p;
                self.theme_index = index;
                self.theme_error = None;
            }
            Err(e) => {
                self.theme_error = Some(format!("{}: {e}", entry.label));
            }
        }
    }

    /// Re-scan the themes directory. Called when the picker opens so a
    /// file dropped in while the app was running shows up.
    fn refresh_themes(&mut self) {
        self.theme_entries = crate::palette::discover(&themes_dir());
        if self.theme_index >= self.theme_entries.len() {
            self.theme_index = 0;
        }
    }

    /// Preferences. Currently the theme list, but this is where the
    /// worlds/accounts editor and the login-autodetect toggle will live
    /// (see UI-REQS.md section 2).
    fn open_prefs(&mut self, ctx: &eframe::egui::Context) {
        let mut chosen: Option<usize> = None;
        let mut close = false;
        let mut rescanned = false;

        eframe::egui::Window::new("Preferences")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                if let Some(err) = &self.theme_error {
                    ui.colored_label(self.palette.error(), format!("{err}"));
                    ui.separator();
                }

                ui.label("Theme");
                ui.separator();

                // Built-ins first, then file-backed themes, so the user
                // can tell which is which without reading paths.
                let mut last_builtin: Option<bool> = None;
                for (i, entry) in self.theme_entries.iter().enumerate() {
                    let is_builtin = entry.is_builtin();
                    if last_builtin != Some(is_builtin) {
                        ui.separator();
                        ui.label(if is_builtin { "Built-in" } else { "From file" });
                        last_builtin = Some(is_builtin);
                    }

                    ui.horizontal(|ui| {
                        let selected = i == self.theme_index;
                        let label = if selected {
                            format!("\u{2713} {}", entry.label)
                        } else {
                            format!("    {}", entry.label)
                        };
                        if ui.selectable_label(selected, label).clicked() {
                            chosen = Some(i);
                            close = true;
                        }
                    });
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Rescan").clicked() {
                        rescanned = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });

        if let Some(i) = chosen {
            self.select_theme(i, ctx);
        }
        if rescanned {
            self.refresh_themes();
        }
        if close {
            self.show_prefs = false;
        }
    }

    fn drain_incoming(&mut self) {
        if let Ok(mut incoming) = self.incoming.try_lock() {
            for (id, lines) in incoming.drain() {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                    for line in lines {
                        if line.text.starts_with("[connected to ") {
                            tab.status = "connected".into();
                            tab.connected = true;
                        }
                        if line.text.starts_with("[connect failed") {
                            tab.status = "failed".into();
                            tab.connected = false;
                        }
                        tab.push_line(line);
                    }
                }
            }
        }
    }

    fn open_connect(&mut self, ctx: &eframe::egui::Context) {
        let mut prompt = self.connect_prompt.take().unwrap_or_default();
        let mut close_after: Option<bool> = None;
        eframe::egui::Window::new("Connect to a world")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let host_id = eframe::egui::Id::new("mist-connect-host");
                let port_id = eframe::egui::Id::new("mist-connect-port");
                ui.horizontal(|ui| {
                    ui.label("Host:");
                    ui.add(eframe::egui::TextEdit::singleline(&mut prompt.host)
                        .id(host_id)
                        .desired_width(180.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Port:");
                    ui.add(eframe::egui::TextEdit::singleline(&mut prompt.port)
                        .id(port_id)
                        .desired_width(60.0));
                });
                if let Some(err) = &self.connect_error {
                    ui.colored_label(eframe::egui::Color32::RED, err);
                }
                ui.horizontal(|ui| {
                    if ui.button("Connect").clicked() {
                        if let Some((host, port)) = prompt.into_target() {
                            self.connect_pending = true;
                            self.connect_error = None;
                            let incoming = self.incoming.clone();
                            let id = self.next_id;
                            self.next_id += 1;
                            let title = format!("{host}:{port}");
                            self.tabs.push(Tab {
                                id,
                                title,
                                host: host.clone(),
                                port,
                                scrollback: vec![RenderedLine::from_text(format!(
                                    "[connecting to {host}:{port}...]"
                                ))],
                                input: String::new(),
                                status: "connecting".into(),
                                connected: false,
                            });
                            self.active = self.tabs.len() - 1;
                            let (tx, _rx) = oneshot::channel();
                            self.runtime.request(DriverRequest::Connect {
                                tab_id: id,
                                host,
                                port,
                                incoming: incoming.clone(),
                                result: tx,
                            });
                            close_after = Some(true);
                        } else {
                            self.connect_error = Some("bad host or port".into());
                            close_after = Some(false);
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        close_after = Some(true);
                    }
                });
            });
        if let Some(want_close) = close_after {
            if !want_close {
                self.connect_prompt = Some(prompt);
            }
        } else {
            self.connect_prompt = Some(prompt);
        }
    }

    fn close_tab(&mut self, idx: usize) {
        if self.tabs.is_empty() {
            return;
        }
        self.tabs.remove(idx);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
    }

    fn new_tab(&mut self) {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab::new(id, "(disconnected)".into(), 0));
        self.active = self.tabs.len() - 1;
    }
}

/// Render a single line into the egui scrollback. Plain text falls through
/// to `ui.monospace`; lines with parsed markup go through a `LayoutJob` so
/// `<b>`, `<i>`, `<u>`, `<font color>`, `<a href>`, `<xch_page>` and the
/// other Pueblo-v0.2 tags actually paint.
///
/// If the line carries an `<a xch_cmd="...">` anchor, the whole line is
/// rendered into a clickable rect — clicking it dispatches the action
/// through the `on_xch_cmd` closure provided by the central panel.
///
/// `line_index` should be unique within a tab so egui's `Id` distingushes
/// every clickable row from the next one.
fn render_line(
    ui: &mut eframe::egui::Ui,
    line: &RenderedLine,
    tab_id: usize,
    line_index: usize,
    mut on_xch_cmd: impl FnMut(String),
) {
    if let Some(node) = &line.markup {
        let mut job = eframe::egui::text::LayoutJob::default();
        build_job(node, &mut job, ui);
        if let Some(cmd) = line.xch_cmd.as_ref() {
            let id = eframe::egui::Id::new(("pueblo-xch", tab_id, line_index));
            // Push a stable id for this row so egui tracks click state
            // and our just-clicked-accent memory across frames.
            ui.push_id(id, |ui| {
                let resp = ui.add(
                    eframe::egui::Label::new(job.clone())
                        .selectable(false)
                        .sense(eframe::egui::Sense::click()),
                );
                // Read input.time ONCE per click-handler invocation; do
                // not call `ui.ctx().input(|i| ...)` while holding the
                // memory lock — egui 0.30 forbids nested mutable borrows
                // of its Context.
                let now = ui.ctx().input(|i| i.time);
                if resp.hovered() {
                    // Cursor change is the decisive hover signal; the row
                    // background tint underneath the label's text gives
                    // a visible-but-not-loud affordance.
                    ui.ctx().set_cursor_icon(eframe::egui::CursorIcon::PointingHand);
                    let rect = resp.rect;
                    let tint = ui
                        .style()
                        .visuals
                        .selection
                        .bg_fill
                        .linear_multiply(0.25);
                    ui.painter().rect_filled(rect, 0.0, tint);
                }
                if resp.clicked() {
                    ui.ctx().memory_mut(|m| {
                        m.data.insert_temp(resp.id, now);
                    });
                    on_xch_cmd(cmd.clone());
                }
                // Brief post-click accent: keep the tint for 250ms after
                // the click so the user can see which row fired. Done
                // outside the memory-mut closure to avoid nested locks.
                let clicked_at = ui.ctx().memory(|m| m.data.get_temp::<f64>(resp.id));
                if let Some(t) = clicked_at {
                    if now - t < 0.25 {
                        let rect = resp.rect;
                        let accent = ui.style().visuals.hyperlink_color.linear_multiply(0.15);
                        ui.painter().rect_filled(rect, 0.0, accent);
                    } else {
                        ui.ctx().memory_mut(|m| m.data.remove::<f64>(resp.id));
                    }
                }
            });
        } else {
            let _ = ui.add(eframe::egui::Label::new(job));
        }
    } else {
        ui.monospace(&line.text);
    }
}

fn monospace_font(ui: &eframe::egui::Ui) -> eframe::egui::FontId {
    let body = ui
        .style()
        .text_styles
        .get(&eframe::egui::TextStyle::Body)
        .cloned()
        .unwrap_or_else(|| eframe::egui::TextStyle::Body.resolve(&ui.style()));
    eframe::egui::FontId {
        size: body.size,
        family: eframe::egui::FontFamily::Monospace,
    }
}

/// Default text colour, from the active theme rather than egui's defaults.
fn default_color(ui: &eframe::egui::Ui) -> eframe::egui::Color32 {
    crate::palette::active(ui.ctx()).text()
}

/// De-emphasised text, from the active theme.
fn weak_color(ui: &eframe::egui::Ui) -> eframe::egui::Color32 {
    crate::palette::active(ui.ctx()).dim()
}

/// Build a text format from an SGR state.
///
/// Colour resolution follows the theme's precedence rules
/// (`UI-REQS.md` section 1.6):
///
///   * `ansi-priority` — the world's own palette is used for indexed
///     colours. Truecolor is always respected.
///   * `theme-priority` — indexed colours are remapped into the theme's
///     palette. Truecolor is still respected, because a world that sends
///     a specific colour means it.
///
/// `default` is the colour to use when the SGR specifies none.
fn fmt_for_sgr(
    ui: &eframe::egui::Ui,
    sgr: &mist_core::ansi::SgrState,
    default: eframe::egui::Color32,
) -> eframe::egui::TextFormat {
    use mist_core::ansi::Color as AnsiColor;
    use mist_core::theme::{Precedence, WorldColour};

    let palette = crate::palette::active(ui.ctx());
    // Which precedence mode are we in? The world's own setting lives on
    // the tab; until per-world precedence is wired through, default to
    // letting the world keep its own palette (ansi-priority), which is
    // what MUD players expect.
    let mode = Precedence::AnsiPriority;

    let fg = match sgr.fg {
        AnsiColor::Default => default,
        AnsiColor::Indexed(i) => palette.world_colour(mode, WorldColour::Indexed(i as usize)),
        AnsiColor::Rgb(r, g, b) => palette.world_colour(mode, WorldColour::True(r, g, b)),
    };

    let mut fmt = make_fmt(ui, fg);
    if sgr.bold {
        fmt = make_bold_fmt(ui, fg);
    }
    if sgr.italic {
        fmt.italics = true;
    }
    if sgr.underline {
        fmt.underline = eframe::egui::Stroke::new(1.0, fg);
    }
    fmt
}

fn make_fmt(ui: &eframe::egui::Ui, color: eframe::egui::Color32) -> eframe::egui::TextFormat {
    eframe::egui::TextFormat {
        font_id: monospace_font(ui),
        color,
        background: eframe::egui::Color32::TRANSPARENT,
        italics: false,
        underline: eframe::egui::Stroke::NONE,
        strikethrough: eframe::egui::Stroke::NONE,
        valign: eframe::egui::Align::BOTTOM,
        extra_letter_spacing: 0.0,
        line_height: None,
    }
}

fn make_bold_fmt(ui: &eframe::egui::Ui, color: eframe::egui::Color32) -> eframe::egui::TextFormat {
    // The egui 0.30 TextFormat has no `strong` flag. We simulate bold with
    // a brighter font colour and a subtle background highlight. v0.3 can
    // pick a true bold variant of the monospace font when one is available.
    eframe::egui::TextFormat {
        font_id: monospace_font(ui),
        color,
        background: eframe::egui::Color32::from_white_alpha(8),
        italics: false,
        underline: eframe::egui::Stroke::NONE,
        strikethrough: eframe::egui::Stroke::NONE,
        valign: eframe::egui::Align::BOTTOM,
        extra_letter_spacing: 0.0,
        line_height: None,
    }
}

fn make_italic_fmt(ui: &eframe::egui::Ui, color: eframe::egui::Color32) -> eframe::egui::TextFormat {
    eframe::egui::TextFormat {
        font_id: monospace_font(ui),
        color,
        background: eframe::egui::Color32::TRANSPARENT,
        italics: true,
        underline: eframe::egui::Stroke::NONE,
        strikethrough: eframe::egui::Stroke::NONE,
        valign: eframe::egui::Align::BOTTOM,
        extra_letter_spacing: 0.0,
        line_height: None,
    }
}

fn make_underline_fmt(
    ui: &eframe::egui::Ui,
    color: eframe::egui::Color32,
) -> eframe::egui::TextFormat {
    let stroke = eframe::egui::Stroke {
        width: 1.0,
        color,
    };
    eframe::egui::TextFormat {
        font_id: monospace_font(ui),
        color,
        background: eframe::egui::Color32::TRANSPARENT,
        italics: false,
        underline: stroke,
        strikethrough: eframe::egui::Stroke::NONE,
        valign: eframe::egui::Align::BOTTOM,
        extra_letter_spacing: 0.0,
        line_height: None,
    }
}

fn build_job(node: &Node, job: &mut eframe::egui::text::LayoutJob, ui: &eframe::egui::Ui) {
    let emit = |job: &mut eframe::egui::text::LayoutJob, text: &str, fmt: eframe::egui::TextFormat| {
        if !text.is_empty() {
            job.append(text, 0.0, fmt);
        }
    };
    match node {
        Node::Document { children } => {
            for c in children {
                build_job(c, job, ui);
            }
        }
        Node::Text(t) => {
            let fmt = fmt_for_sgr(ui, &t.sgr, default_color(ui));
            emit(job, &t.text, fmt);
        }
        Node::Bold(children) => {
            let fmt = make_bold_fmt(ui, default_color(ui));
            for c in children {
                emit_subtree(c, job, ui, &fmt);
            }
        }
        Node::Italic(children) => {
            let fmt = make_italic_fmt(ui, default_color(ui));
            for c in children {
                emit_subtree(c, job, ui, &fmt);
            }
        }
        Node::Underline(children) => {
            let fmt = make_underline_fmt(ui, default_color(ui));
            for c in children {
                emit_subtree(c, job, ui, &fmt);
            }
        }
        Node::Font { color, children, .. } => {
            let color = color
                .as_deref()
                .and_then(parse_color)
                .unwrap_or_else(|| default_color(ui));
            let fmt = make_fmt(ui, color);
            for c in children {
                emit_subtree(c, job, ui, &fmt);
            }
        }
        Node::LineBreak => emit(job, "\n", make_fmt(ui, default_color(ui))),
        Node::HorizontalRule => {
            emit(job, &"─".repeat(40), make_fmt(ui, weak_color(ui)));
            emit(job, "\n", make_fmt(ui, default_color(ui)));
        }
        Node::Image { src, alt, width, height } => {
            // The image's pixels come from the per-URL texture cache
            // (see the image-fetch block in the central panel). Here in
            // the LayoutJob we leave a single soft line-break so the
            // ui.image() call following us in the central panel sits
            // cleanly on its own row. The placeholder text is gone in
            // v0.4 — when no texture is available we just leave a
            // blank line.
            let _ = (src, alt, width, height);
            emit(job, "\n", make_fmt(ui, weak_color(ui)));
        }
        Node::Anchor { children, .. } => {
            let text: String = children
                .iter()
                .map(|c| match c {
                    Node::Text(t) => t.text.clone(),
                    _ => String::new(),
                })
                .collect();
            let link_color = crate::palette::active(ui.ctx()).accent();
            let fmt = make_underline_fmt(ui, link_color);
            emit(job, &text, fmt);
        }
        Node::PageBreak { .. } => {
            emit(job, "───────\n", make_fmt(ui, weak_color(ui)));
        }
        Node::MudText(children) | Node::Unknown { children, .. } => {
            for c in children {
                build_job(c, job, ui);
            }
        }
    }
}

fn emit_subtree(
    node: &Node,
    job: &mut eframe::egui::text::LayoutJob,
    ui: &eframe::egui::Ui,
    fmt: &eframe::egui::TextFormat,
) {
    if let Node::Text(t) = node {
        if !t.text.is_empty() {
            job.append(&t.text, 0.0, fmt.clone());
        }
        return;
    }
    // For nested non-text nodes we recurse into `build_job` so a `<b><i>x</i></b>`
    // ends up combining. The fmt we carry through is preserved on the leaves.
    match node {
        Node::Document { children } | Node::MudText(children) | Node::Unknown { children, .. } => {
            for c in children {
                emit_subtree(c, job, ui, fmt);
            }
        }
        _ => build_job(node, job, ui),
    }
}

fn parse_color(s: &str) -> Option<eframe::egui::Color32> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(eframe::egui::Color32::from_rgb(r, g, b))
}

/// Walk the parsed markup tree and collect every `<img>` node in
/// document order. The renderer uses width/height hints to size the
/// image before the texture loads; src drives the fetch.
fn collect_images(node: &Node) -> Vec<(String, Option<u32>, Option<u32>)> {
    let mut out = Vec::new();
    fn walk(node: &Node, out: &mut Vec<(String, Option<u32>, Option<u32>)>) {
        match node {
            Node::Image { src, width, height, .. } if !src.is_empty() => {
                out.push((src.clone(), *width, *height));
            }
            Node::Document { children }
            | Node::Bold(children)
            | Node::Italic(children)
            | Node::Underline(children)
            | Node::MudText(children)
            | Node::Unknown { children, .. } => {
                for c in children {
                    walk(c, out);
                }
            }
            Node::Font { children, .. } => {
                for c in children {
                    walk(c, out);
                }
            }
            _ => {}
        }
    }
    walk(node, &mut out);
    out
}

impl eframe::App for MistApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        self.drain_incoming();

        eframe::egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Mist");
                ui.separator();
                if ui.button("Connect").clicked() {
                    self.connect_prompt = Some(ConnectPrompt::default());
                }
                if ui.button("New Tab").clicked() {
                    self.new_tab();
                }
                if ui.button("Worlds").clicked() {
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.push_line(RenderedLine::from_text("[TODO] world list"));
                    }
                }
                if ui.button("Prefs").clicked() {
                    self.show_prefs = !self.show_prefs;
                }
                if let Some(tab) = self.tabs.get(self.active) {
                    if tab.connected && ui.button("Disconnect").clicked() {
                        self.runtime.request(DriverRequest::Disconnect { tab_id: tab.id });
                        let id = tab.id;
                        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
                            t.connected = false;
                            t.status = "disconnected".into();
                            t.push_line(RenderedLine::from_text("[disconnected]"));
                        }
                    }
                }
                ui.separator();
                ui.label(format!("{} tab(s)", self.tabs.len()));
            });
        });

        eframe::egui::TopBottomPanel::bottom("statusbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(tab) = self.tabs.get(self.active) {
                    ui.label(&tab.status);
                }
                ui.with_layout(
                    eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                    |ui| {
                        ui.label("--:--:--");
                    },
                );
            });
        });

        let connect_pending = self.connect_prompt.is_some();

        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut to_close: Option<usize> = None;
                let n = self.tabs.len();
                for i in 0..n {
                    let selected = i == self.active;
                    let title = self.tabs[i].title.clone();
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            if ui.selectable_label(selected, &title).clicked() {
                                self.active = i;
                            }
                            if ui.small_button("×").clicked() {
                                to_close = Some(i);
                            }
                        });
                    });
                }
                if let Some(i) = to_close {
                    self.close_tab(i);
                }
            });
            ui.separator();

            if let Some(tab) = self.tabs.get_mut(self.active) {
                let input_height = 32.0_f32;
                let remaining = (ui.available_height() - input_height).max(80.0);
                eframe::egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .max_height(remaining)
                    .show(ui, |ui| {
                        // Snapshot the lines for this frame so the
                        // onclick closure can use `self.runtime` without
                        // fighting the borrow checker. `RenderedLine` is
                        // small, and the snapshot is cheap.
                        let lines_snapshot: Vec<RenderedLine> =
                            tab.scrollback.clone();
                        let tab_id = tab.id;
                        let connected = tab.connected;
                        let runtime = self.runtime.clone();
                        let images = self.images.clone();
                        // Tell egui to repaint roughly every second so
                        // freshly fetched images paint without further
                        // user interaction.
                        ctx.request_repaint_after(std::time::Duration::from_millis(1000));
                        for (idx, line) in lines_snapshot.iter().enumerate() {
                            render_line(ui, line, tab_id, idx, |cmd| {
                                if !connected {
                                    return;
                                }
                                let payload = format!("{}\r\n", cmd);
                                runtime.request(DriverRequest::Send {
                                    tab_id,
                                    line: payload,
                                });
                            });
                            // If the line carries Pueblo `<img>` markup, try
                            // to render the fetched texture. The fetch is
                            // kicked off lazily — `build_job` left a
                            // single line-break in the LayoutJob so the
                            // image sits on its own row.
                            if let Some(node) = &line.markup {
                                let entries = collect_images(node);
                                for (url, width, height) in entries {
                                    if !images.is_seen(&url) {
                                        images.fetch(&runtime, url.clone(), ctx.clone());
                                    }
                                    if let Some(img) = images.get(&url) {
                                        let tex = &img.texture;
                                        // Resolve size: prefer the markup's
                                        // width/height hints (in pixels), then
                                        // the texture's natural size, capped
                                        // to the pane width.
                                        let max_w = (ui.available_width() - 16.0).max(64.0);
                                        let tex_size = tex.size();
                                        let (src_w, src_h) =
                                            (tex_size[0] as f32, tex_size[1] as f32);
                                        let (out_w, out_h) = match (width, height) {
                                            (Some(w), Some(h)) => {
                                                let w = w as f32;
                                                let h = h as f32;
                                                let scale =
                                                    (max_w / w).min(1.0);
                                                (w * scale, h * scale)
                                            }
                                            (Some(w), None) => {
                                                let w = w as f32;
                                                let scale = (max_w / w).min(1.0);
                                                (w * scale, src_h * scale.max(1e-3))
                                            }
                                            _ => {
                                                let scale =
                                                    (max_w / src_w).min(1.0);
                                                (src_w * scale, src_h * scale)
                                            }
                                        };
                                        let size = eframe::egui::vec2(out_w, out_h);
                                        ui.image(eframe::egui::ImageSource::Texture(
                                            eframe::egui::load::SizedTexture::new(
                                                tex.id(),
                                                size,
                                            ),
                                        ));
                                    }
                                }
                            }
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    let id = eframe::egui::Id::new(("tab-input", tab.id));
                    let te = eframe::egui::TextEdit::singleline(&mut tab.input)
                        .id(id)
                        .hint_text("type a command and press Enter")
                        .desired_width(f32::INFINITY);
                    let _resp = ui.add(te);
                    let focused = ui.ctx().memory(|m| m.has_focus(id));
                    let enter_pressed = ui.input(|i| i.key_pressed(eframe::egui::Key::Enter));
                    let nothing_else_focused = ui.ctx().memory(|m| m.focused().is_none());
                    if enter_pressed && (focused || nothing_else_focused) {
                        let line = tab.input.trim().to_string();
                        if !line.is_empty() {
                            tab.push_line(RenderedLine::from_text(format!("> {line}")));
                            if tab.connected {
                                let payload = format!("{line}\r\n");
                                self.runtime.request(DriverRequest::Send {
                                    tab_id: tab.id,
                                    line: payload,
                                });
                            }
                            tab.input.clear();
                        }
                        ui.ctx().memory_mut(|m| m.request_focus(id));
                    }
                    if !focused && nothing_else_focused {
                        ui.ctx().memory_mut(|m| m.request_focus(id));
                    }
                });
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label("No tabs open. Click Connect or New Tab.");
                });
            }
        });

        if connect_pending {
            self.open_connect(ctx);
        }
        if self.show_prefs {
            self.open_prefs(ctx);
        }
    }

}
