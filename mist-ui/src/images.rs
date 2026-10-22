//! Background image fetcher and per-URL texture cache.
//!
//! MUSH markup like `<img src="..." alt="...">` references external
//! image URLs. This module fetches them once via reqwest, decodes
//! them with the `image` crate, and pushes the resulting
//! [`egui::TextureHandle`] into a per-app cache keyed by URL.
//!
//! While a fetch is in flight (or fails), callers should fall back to
//! a placeholder like `[img:src|alt]`.

use crate::session::RuntimeHandle;
use std::collections::HashMap;
use std::sync::Arc;

/// One cached image: a `egui::TextureHandle` plus a natural size hint
/// for layout. `natural_size` is `(width, height)` in pixels.
#[derive(Clone)]
pub struct CachedImage {
    pub texture: eframe::egui::TextureHandle,
    pub natural_size: (usize, usize),
}

#[derive(Default)]
pub struct CacheInner {
    pub pending: std::collections::HashSet<String>,
    pub ready: HashMap<String, CachedImage>,
    pub failed: std::collections::HashSet<String>,
}

/// Application-wide image cache. Cloneable and `Send`-able.
///
/// Inner lock is a `std::sync::Mutex` so the egui thread can do sync
/// lookups without dropping out of the runtime. The fetch path runs
/// on the tokio runtime and only takes the lock briefly.
#[derive(Clone, Default)]
pub struct ImageCache {
    inner: Arc<std::sync::Mutex<CacheInner>>,
}

impl ImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Synchronously fetch a cached image. Returns `None` while the
    /// fetch is in flight or has failed.
    pub fn get(&self, url: &str) -> Option<CachedImage> {
        let g = self.inner.lock().ok()?;
        g.ready.get(url).cloned()
    }

    /// Whether the URL is currently being fetched, has been cached, or
    /// has a known-permanent failure.
    pub fn is_seen(&self, url: &str) -> bool {
        let Ok(g) = self.inner.lock() else { return false };
        g.pending.contains(url) || g.ready.contains_key(url) || g.failed.contains(url)
    }

    /// Begin fetching `url` from the given runtime. Subsequent calls
    /// for the same URL are no-ops. The fetched texture is inserted
    /// into the cache on success.
    pub fn fetch(&self, runtime: &RuntimeHandle, url: String, ctx: eframe::egui::Context) {
        // Reserve the slot synchronously so a back-to-back fetch() is a no-op.
        let url_for_state = url.clone();
        if let Ok(mut g) = self.inner.lock() {
            if g.pending.contains(&url_for_state)
                || g.ready.contains_key(&url_for_state)
                || g.failed.contains(&url_for_state)
            {
                return;
            }
            g.pending.insert(url_for_state);
        } else {
            return; // an in-flight task already holds the lock; skip.
        }

        let inner = self.inner.clone();
        runtime.request(crate::session::DriverRequest::FetchImage { url, ctx, cache: inner });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_initially_empty() {
        let cache = ImageCache::new();
        assert!(cache.get("https://example.com/missing.png").is_none());
        assert!(!cache.is_seen("https://example.com/missing.png"));
    }
}

/// Fetch `url`, decode as a portable image, return `(natural_size, ColorImage)`
/// ready for `ctx.load_texture`. `ColorImage` is `egui::ColorImage`.
///
/// Handles `http://`, `https://`, and `data:image/...;base64,...` directly
/// (reqwest does not handle data: URIs out of the box).
pub async fn fetch_and_cache(
    url: &str,
    _ctx: &eframe::egui::Context,
) -> anyhow::Result<((usize, usize), eframe::egui::ColorImage)> {
    let bytes = if let Some(stripped) = url.strip_prefix("data:") {
        parse_data_url(stripped)?
    } else {
        reqwest_bytes(url).await?
    };
    let img = image::load_from_memory(&bytes)?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let pixels: Vec<eframe::egui::Color32> = rgba
        .chunks_exact(4)
        .map(|c| eframe::egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        .collect();
    let color_image = eframe::egui::ColorImage { size: [w as usize, h as usize], pixels };
    Ok(((w as usize, h as usize), color_image))
}

/// Decode `image/<mime>;base64,<data>` into raw bytes. We support the
/// `base64` form because that's what MUSH markup actually produces.
fn parse_data_url(stripped: &str) -> anyhow::Result<Vec<u8>> {
    let comma = stripped.find(',').ok_or_else(|| anyhow::anyhow!("data: uri missing comma"))?;
    let (header, payload) = stripped.split_at(comma);
    let payload = &payload[1..]; // skip the comma
    let is_base64 = header.split(';').any(|part| part.trim().eq_ignore_ascii_case("base64"));
    if !is_base64 {
        anyhow::bail!("only base64-encoded data: URIs are supported");
    }
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| anyhow::anyhow!("base64 decode failed: {e}"))?;
    Ok(bytes)
}

/// Tiny helper so the actual `reqwest::Client` lives next to the call site.
async fn reqwest_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()?;
    let resp = client.get(url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("http {status} for {url}");
    }
    let bytes = resp.bytes().await?;
    Ok(bytes.to_vec())
}
