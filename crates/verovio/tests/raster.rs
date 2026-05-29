//! Tests for the feature-gated [`verovio::raster`] module. Runs only when
//! the `png` and/or `pdf` features are enabled.

#![cfg(any(feature = "png", feature = "pdf"))]

use verovio::Toolkit;

const SAMPLE_PAE: &str =
    "@start:s\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:\n@data:'4G/4-\n@end:s\n";

// -- PNG ---------------------------------------------------------------------

#[cfg(feature = "png")]
#[test]
fn render_to_png_produces_png_magic() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let png = tk.render_to_png(1, 1.0).expect("render");

    // PNG signature is 8 bytes: 89 50 4E 47 0D 0A 1A 0A.
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    assert!(
        png.len() > 1000,
        "1-bar score PNG should be > 1 KB, got {}",
        png.len()
    );
}

#[cfg(feature = "png")]
#[test]
fn render_to_png_scale_increases_pixel_count() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let small = tk.render_to_png(1, 0.5).expect("scale 0.5");
    let large = tk.render_to_png(1, 2.0).expect("scale 2.0");
    // PNG byte size scales roughly with pixel area — 16× pixels is many
    // times bigger than 0.25× pixels even after PNG compression.
    assert!(
        large.len() > small.len() * 2,
        "expected larger PNG at higher scale, got small={} large={}",
        small.len(),
        large.len()
    );
}

#[cfg(feature = "png")]
#[test]
fn render_to_png_out_of_range_page_propagates_error() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let res = tk.render_to_png(999, 1.0);
    assert!(res.is_err(), "out-of-range page should error");
}

#[cfg(feature = "png")]
#[test]
fn svg_to_png_pure_function_matches_toolkit_method() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let svg = tk.render_to_svg(1).expect("svg");
    let via_function = verovio::raster::svg_to_png(&svg, 1.0).expect("svg_to_png");
    let via_method = tk.render_to_png(1, 1.0).expect("render_to_png");
    assert_eq!(via_function, via_method);
}

// -- PDF ---------------------------------------------------------------------

#[cfg(feature = "pdf")]
#[test]
fn render_to_pdf_produces_pdf_magic() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let pdf = tk.render_to_pdf(1).expect("render");

    // PDF signature is "%PDF-".
    assert_eq!(&pdf[..5], b"%PDF-");
    assert!(
        pdf.len() > 500,
        "1-bar score PDF should be > 500 B, got {}",
        pdf.len()
    );
}

#[cfg(feature = "pdf")]
#[test]
fn render_to_pdf_out_of_range_page_propagates_error() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let res = tk.render_to_pdf(999);
    assert!(res.is_err(), "out-of-range page should error");
}
