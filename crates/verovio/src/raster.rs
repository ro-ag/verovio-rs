//! Feature-gated rasterization of Verovio's SVG output to PNG / PDF.
//!
//! Behind the `png` / `pdf` Cargo features. Adds `resvg` (and `svg2pdf`
//! for the PDF path) — pure Rust, no system dependencies.
//!
//! The functions in this module are pure: they take an SVG string and
//! return bytes. Convenience methods on [`Toolkit`](crate::Toolkit)
//! render to SVG internally for the per-page case.

#[cfg(feature = "png")]
use crate::{Error, Result};

/// Rasterize an SVG string to PNG bytes via `resvg`.
///
/// `scale` is a multiplier on the SVG's intrinsic size — `1.0` renders at
/// the SVG's nominal pixel size, `2.0` doubles each dimension (suitable
/// for HiDPI displays). Output dimensions are clamped to `u32::MAX`.
///
/// Behind the `png` Cargo feature.
#[cfg(feature = "png")]
pub fn svg_to_png(svg: &str, scale: f32) -> Result<Vec<u8>> {
    use resvg::tiny_skia;
    use resvg::usvg;

    let opts = usvg::Options::default();
    let tree =
        usvg::Tree::from_str(svg, &opts).map_err(|e| Error::Xml(format!("usvg parse: {e}")))?;
    let size = tree.size();
    let width = (size.width() * scale).round().max(1.0) as u32;
    let height = (size.height() * scale).round().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| Error::Xml(format!("pixmap allocation failed for {width}x{height}")))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|e| Error::Xml(format!("PNG encode: {e}")))
}

/// Convert an SVG string to a PDF document (single page) via `svg2pdf`.
///
/// Behind the `pdf` Cargo feature.
#[cfg(feature = "pdf")]
pub fn svg_to_pdf(svg: &str) -> crate::Result<Vec<u8>> {
    let options = svg2pdf::ConversionOptions::default();
    let page_options = svg2pdf::PageOptions::default();
    let bytes = svg2pdf::to_pdf(
        &svg2pdf::usvg::Tree::from_str(svg, &svg2pdf::usvg::Options::default())
            .map_err(|e| crate::Error::Xml(format!("usvg parse: {e}")))?,
        options,
        page_options,
    )
    .map_err(|e| crate::Error::Xml(format!("PDF encode: {e}")))?;
    Ok(bytes)
}

#[cfg(any(feature = "png", feature = "pdf"))]
impl crate::Toolkit {
    /// Render a single page to PNG bytes. `scale = 1.0` matches the SVG's
    /// nominal size; `2.0` is suitable for HiDPI. Behind the `png` feature.
    #[cfg(feature = "png")]
    pub fn render_to_png(&mut self, page: u32, scale: f32) -> crate::Result<Vec<u8>> {
        let svg = self.render_to_svg(page)?;
        svg_to_png(&svg, scale)
    }

    /// Render a single page to PDF bytes. Behind the `pdf` feature.
    #[cfg(feature = "pdf")]
    pub fn render_to_pdf(&mut self, page: u32) -> crate::Result<Vec<u8>> {
        let svg = self.render_to_svg(page)?;
        svg_to_pdf(&svg)
    }
}
