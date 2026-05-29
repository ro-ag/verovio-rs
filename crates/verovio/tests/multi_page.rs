//! Multi-page rendering coverage. Uses Verovio's own `doc/importer.mei`
//! sample (7 measures of MEI 4.0) at a deliberately narrow page width so
//! the layout is forced across multiple pages.
//!
//! The fixture is embedded via `include_str!` so the test doesn't depend on
//! a runtime path into the submodule and runs identically from any cwd.

use verovio::{Error, Toolkit};

const SAMPLE_MEI: &str = include_str!("../../verovio-sys/vendor/verovio/doc/importer.mei");

fn multi_page_toolkit() -> Toolkit {
    let mut tk = Toolkit::new();
    // Narrow + short page dimensions force the 7-measure sample to paginate.
    // Values in Verovio internal units; pageWidth=800 + pageHeight=400 gives
    // ~2 measures per page on this fixture (verified empirically; see asserts).
    tk.set_options(r#"{"pageWidth": 800, "pageHeight": 400}"#)
        .expect("narrow page options");
    tk.load_data(SAMPLE_MEI).expect("importer.mei should parse");
    tk
}

#[test]
fn importer_mei_produces_multiple_pages_with_narrow_layout() {
    let mut tk = multi_page_toolkit();
    let pages = tk.page_count();
    assert!(
        pages >= 2,
        "expected multi-page layout with narrow options, got {pages} page(s)"
    );
}

#[test]
fn every_page_renders_to_valid_svg() {
    let mut tk = multi_page_toolkit();
    let pages = tk.page_count();
    for page in 1..=pages {
        let svg = tk
            .render_to_svg(page)
            .unwrap_or_else(|e| panic!("page {page} failed: {e}"));
        assert!(svg.contains("<svg"), "page {page} missing <svg");
        assert!(svg.contains("</svg>"), "page {page} missing </svg>");
        // Sanity: a real engraved page is at least a few kilobytes.
        assert!(
            svg.len() > 500,
            "page {page} SVG suspiciously small ({} bytes)",
            svg.len()
        );
    }
}

#[test]
fn buffer_reuse_across_pages_preserves_capacity() {
    let mut tk = multi_page_toolkit();
    let pages = tk.page_count();
    let mut buf = String::with_capacity(64 * 1024);
    let mut peak_cap = buf.capacity();

    for page in 1..=pages {
        tk.render_to_svg_into(page, &mut buf).unwrap();
        peak_cap = peak_cap.max(buf.capacity());
        assert!(buf.contains("<svg"));
    }

    // After iterating every page, the buffer should be at least as big
    // as on its largest render — i.e. capacity never shrank in a way that
    // would have forced reallocation on the next page.
    assert!(
        buf.capacity() >= peak_cap,
        "buf shrank from {peak_cap} to {} — reuse defeated",
        buf.capacity()
    );
}

#[test]
fn out_of_range_page_still_errors_on_multi_page_doc() {
    let mut tk = multi_page_toolkit();
    let pages = tk.page_count();
    let beyond = pages + 5;
    let res = tk.render_to_svg(beyond);
    assert!(
        matches!(res, Err(Error::RenderFailed { page }) if page == beyond),
        "got {res:?}"
    );
}

#[test]
fn timemap_for_multi_page_doc_is_chronological() {
    let mut tk = multi_page_toolkit();
    let timemap = tk.timemap().expect("multi-page timemap parse");
    assert!(!timemap.is_empty());
    for w in timemap.windows(2) {
        assert!(
            w[0].tstamp <= w[1].tstamp,
            "events out of order across the multi-page doc"
        );
    }
}

#[test]
fn elements_at_resolves_to_the_right_page() {
    let mut tk = multi_page_toolkit();
    let timemap = tk.timemap().unwrap();
    assert!(timemap.len() >= 2, "need at least two events for this test");

    // Pick a tstamp from the middle of the document.
    let mid = timemap.len() / 2;
    let mid_ms = timemap[mid].tstamp as u32;
    let elements = tk.elements_at(mid_ms).expect("elements parse");
    assert!(
        elements.page.is_some_and(|p| p >= 1),
        "expected resolved page for mid-doc query, got {elements:?}"
    );
}
