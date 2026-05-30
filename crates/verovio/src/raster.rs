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

/// Assemble a multi-page PDF document from a slice of SVG strings.
/// Each SVG becomes one page, sized to the SVG's intrinsic dimensions
/// converted at 72 DPI (PDF's native unit).
///
/// Uses [`svg2pdf::to_chunk`] to convert each SVG into an embeddable
/// XObject, renumbers each chunk against a single PDF-wide ref
/// allocator, and references the XObject from a `Page` with the right
/// `MediaBox` and a scaling content stream.
///
/// Behind the `pdf` Cargo feature.
#[cfg(feature = "pdf")]
pub fn svgs_to_pdf(svgs: &[String]) -> crate::Result<Vec<u8>> {
    use std::collections::HashMap;

    use pdf_writer::{Chunk, Content, Finish, Name, Pdf, Rect, Ref};
    use svg2pdf::usvg::{Options, Tree};

    if svgs.is_empty() {
        return Err(crate::Error::RenderFailed { page: 0 });
    }

    let mut pdf = Pdf::new();
    let mut next_id: i32 = 1;
    let mut alloc = || -> Ref {
        let r = Ref::new(next_id);
        next_id += 1;
        r
    };

    let catalog_ref = alloc();
    let page_tree_ref = alloc();

    // First pass: allocate page refs and content refs so we can write
    // the pages tree's kids array up front.
    let mut page_refs: Vec<Ref> = Vec::with_capacity(svgs.len());
    let mut content_refs: Vec<Ref> = Vec::with_capacity(svgs.len());
    for _ in svgs {
        page_refs.push(alloc());
        content_refs.push(alloc());
    }

    pdf.catalog(catalog_ref).pages(page_tree_ref);
    {
        let mut pages = pdf.pages(page_tree_ref);
        pages.count(svgs.len() as i32).kids(page_refs.iter().copied());
        pages.finish();
    }

    // Second pass: for each SVG, get its embed chunk + xobject ref,
    // renumber into the document, write the page.
    for (i, svg) in svgs.iter().enumerate() {
        let tree = Tree::from_str(svg, &Options::default())
            .map_err(|e| crate::Error::Xml(format!("usvg parse page {}: {e}", i + 1)))?;
        let (chunk, svg_id) =
            svg2pdf::to_chunk(&tree, svg2pdf::ConversionOptions::default())
                .map_err(|e| crate::Error::Xml(format!("svg2pdf chunk page {}: {e}", i + 1)))?;

        // Renumber every ref in the chunk into our global allocator.
        let mut remap = HashMap::new();
        let renumbered: Chunk = chunk.renumber(|old| {
            *remap.entry(old).or_insert_with(|| {
                let r = Ref::new(next_id);
                next_id += 1;
                r
            })
        });
        let svg_ref = remap[&svg_id];

        // Append the chunk's bytes into our PDF.
        pdf.extend(&renumbered);

        // Compute the page's intrinsic size — usvg reports it in pixels;
        // at 72 DPI 1 px == 1 pt, which is PDF's native unit. So we use
        // the values as-is.
        let size = tree.size();
        let width = size.width();
        let height = size.height();

        // svg2pdf's XObject has unit (1×1) bbox — scale + flip Y so the
        // SVG fills the page with the correct orientation.
        let mut content = Content::new();
        content.transform([width, 0.0, 0.0, height, 0.0, 0.0]);
        let xobj_name = Name(b"S");
        content.x_object(xobj_name);
        let content_bytes = content.finish();
        pdf.stream(content_refs[i], &content_bytes);

        let mut page = pdf.page(page_refs[i]);
        page.media_box(Rect::new(0.0, 0.0, width, height));
        page.parent(page_tree_ref);
        page.contents(content_refs[i]);
        let mut resources = page.resources();
        resources.x_objects().pair(xobj_name, svg_ref);
        resources.finish();
        page.finish();
    }

    Ok(pdf.finish())
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

    /// Render every page to PNG bytes — one PNG per page, returned in
    /// page order (1, 2, 3, …). Convenience over looping
    /// [`Self::render_to_png`] yourself; useful for thumbnail strips and
    /// preview galleries. Behind the `png` feature.
    #[cfg(feature = "png")]
    pub fn render_to_png_all_pages(&mut self, scale: f32) -> crate::Result<Vec<Vec<u8>>> {
        let pages = self.page_count();
        let mut out = Vec::with_capacity(pages as usize);
        for page in 1..=pages {
            out.push(self.render_to_png(page, scale)?);
        }
        Ok(out)
    }

    /// Render a single page to PDF bytes. Behind the `pdf` feature.
    #[cfg(feature = "pdf")]
    pub fn render_to_pdf(&mut self, page: u32) -> crate::Result<Vec<u8>> {
        let svg = self.render_to_svg(page)?;
        svg_to_pdf(&svg)
    }

    /// Render every page into a single multi-page PDF document.
    /// Each page is sized to its rendered SVG's intrinsic dimensions.
    /// Behind the `pdf` feature.
    #[cfg(feature = "pdf")]
    pub fn render_to_pdf_all_pages(&mut self) -> crate::Result<Vec<u8>> {
        let pages = self.page_count();
        if pages == 0 {
            return Err(crate::Error::RenderFailed { page: 0 });
        }
        let mut svgs: Vec<String> = Vec::with_capacity(pages as usize);
        for page in 1..=pages {
            svgs.push(self.render_to_svg(page)?);
        }
        svgs_to_pdf(&svgs)
    }
}
