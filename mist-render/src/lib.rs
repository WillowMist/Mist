//! Custom HTML/MWorld renderer (no CEF).
//!
//! Takes the `pueblo-core::markup::Node` tree and lays it out into glyph
//! runs using `cosmic-text`, then paints into a `tiny-skia` canvas.
//!
//! v1 supports the SPEC §3 tag subset. The data model is laid out as one
//! vertical column per pane; the pane width is `wrap_width` and lines wrap
//! at word boundaries.

use mist_core::markup::Node;

pub mod layout;
pub mod paint;

pub use layout::PaneLayout;

/// Convert a parsed document into a sequence of pane layouts.
pub fn layout_document(doc: &Node, wrap_width: f32) -> Vec<PaneLayout> {
    let mut out = Vec::new();
    match doc {
        Node::Document { children } => {
            let mut cur = PaneLayout::new();
            for child in children {
                layout::layout_node(child, &mut cur, wrap_width);
            }
            out.push(cur);
        }
        _ => {
            let mut p = PaneLayout::new();
            layout::layout_node(doc, &mut p, wrap_width);
            out.push(p);
        }
    }
    out
}