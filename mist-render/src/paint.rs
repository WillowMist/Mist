//! Paint a [`PaneLayout`] into a `tiny-skia` Pixmap.
//!
//! v1 scaffolding. The actual glyph rasterisation will be done through
//! `egui-wgpu` in the UI milestone; this function exists so render integration
//! has a real call site.

use crate::layout::PaneLayout;
use tiny_skia::Pixmap;

/// Render `pane` into a new Pixmap of the given size.
///
/// v1: black background, glyphs not yet drawn. Returns the pixmap so callers
/// can plumb it into an image widget for smoke-testing the layout pipeline.
pub fn render(pane: &PaneLayout, width: u32, height: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).expect("pixmap alloc");
    pixmap.fill(tiny_skia::Color::from_rgba8(0, 0, 0, 255));
    let _ = pane;
    pixmap
}