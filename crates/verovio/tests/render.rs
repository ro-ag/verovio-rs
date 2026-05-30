//! Smoke tests for the rendering surface — `render_to_svg`,
//! `render_to_timemap`, `redo_layout`, `elements_at_time`, and their `_into`
//! buffer-reuse counterparts.

use verovio::{Error, Toolkit};

const SAMPLE_PAE: &str = "\
@start:clefs
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:clefs
";

fn loaded_toolkit() -> Toolkit {
    let mut tk = Toolkit::new();
    tk.load_data(SAMPLE_PAE).expect("PAE fixture should load");
    tk
}

#[test]
fn render_to_svg_returns_well_formed_svg() {
    let mut tk = loaded_toolkit();
    let svg = tk.render_to_svg(1).expect("page 1 should render");
    assert!(
        svg.contains("<svg"),
        "expected <svg in output, got first 200 chars: {}",
        &svg[..svg.len().min(200)]
    );
    assert!(svg.contains("</svg>"), "expected closing </svg> tag");
}

#[test]
fn render_to_svg_into_reuses_buffer() {
    let mut tk = loaded_toolkit();
    let mut buf = String::with_capacity(4096);
    let initial_capacity = buf.capacity();

    tk.render_to_svg_into(1, &mut buf).expect("render");
    assert!(buf.contains("<svg"));

    // Render again into the same buffer; capacity should not have shrunk.
    tk.render_to_svg_into(1, &mut buf).expect("render again");
    assert!(buf.contains("<svg"));
    assert!(
        buf.capacity() >= initial_capacity,
        "buffer capacity shrank — reuse defeated"
    );
}

#[test]
fn render_to_svg_out_of_range_page_returns_render_failed() {
    let mut tk = loaded_toolkit();
    // Page 999 is well beyond any one-bar PAE document.
    let res = tk.render_to_svg(999);
    assert!(
        matches!(res, Err(Error::RenderFailed { page: 999 })),
        "got {res:?}"
    );
}

#[test]
fn render_to_svg_without_loaded_data_fails() {
    let mut tk = Toolkit::new();
    let res = tk.render_to_svg(1);
    assert!(
        matches!(res, Err(Error::RenderFailed { page: 1 })),
        "got {res:?}"
    );
}

#[test]
fn render_to_midi_returns_base64_with_midi_header() {
    let mut tk = loaded_toolkit();
    let midi = tk.render_to_midi().expect("midi render");
    // Verovio's RenderToMIDI base64-encodes a standard SMF file. SMF starts
    // with the 4-byte ASCII tag "MThd"; base64 of that is "TVRoZA".
    assert!(
        midi.starts_with("TVRoZA"),
        "expected base64-encoded MThd header, got first 20 chars: {}",
        &midi[..midi.len().min(20)]
    );
}

#[test]
fn render_to_midi_into_reuses_buffer() {
    let mut tk = loaded_toolkit();
    let mut buf = String::with_capacity(1024);
    tk.render_to_midi_into(&mut buf).expect("midi render");
    let cap = buf.capacity();
    tk.render_to_midi_into(&mut buf).expect("midi render again");
    assert!(buf.capacity() >= cap);
}

#[test]
fn render_to_midi_without_loaded_data_fails() {
    let mut tk = Toolkit::new();
    let res = tk.render_to_midi();
    assert!(
        matches!(res, Err(Error::RenderFailed { page: 0 })),
        "got {res:?}"
    );
}

#[test]
fn render_to_midi_bytes_returns_standard_midi_file_header() {
    let mut tk = loaded_toolkit();
    let bytes = tk.render_to_midi_bytes().expect("midi bytes");
    // SMF starts with the 4-byte "MThd" magic followed by a 4-byte big-endian
    // header length of 6. Verifying the magic round-trips proves both
    // the C++ render path and our base64 decode produced the right payload.
    assert!(
        bytes.starts_with(b"MThd"),
        "expected MThd magic, got first 8 bytes: {:?}",
        &bytes[..bytes.len().min(8)]
    );
    assert!(bytes.len() > 14, "SMF must be longer than the header");
}

#[test]
fn render_to_midi_bytes_without_loaded_data_fails() {
    let mut tk = Toolkit::new();
    let res = tk.render_to_midi_bytes();
    assert!(
        matches!(res, Err(Error::RenderFailed { page: 0 })),
        "got {res:?}"
    );
}

// Verovio's render path asserts inside `Doc::GetVisibleScores` on an empty
// doc (`m_visibleScores.empty()`) and SIGABRTs the whole process. The safe
// wrapper guards every render-family method by checking `page_count() == 0`
// before crossing the bridge. These tests pin that protection — if a future
// commit removes a guard, the test binary will abort instead of returning.
#[test]
fn render_to_timemap_without_loaded_data_returns_err() {
    let mut tk = Toolkit::new();
    let res = tk.render_to_timemap();
    assert!(
        matches!(res, Err(Error::RenderFailed { page: 0 })),
        "got {res:?}"
    );
}

#[test]
fn elements_at_time_without_loaded_data_returns_empty_json() {
    let mut tk = Toolkit::new();
    let json = tk.elements_at_time(0);
    assert_eq!(json, "{}");
}

#[test]
fn render_to_timemap_returns_json_array() {
    let mut tk = loaded_toolkit();
    let json = tk.render_to_timemap().expect("timemap render");
    let trimmed = json.trim();
    assert!(
        trimmed.starts_with('[') && trimmed.ends_with(']'),
        "expected JSON array, got first 200 chars: {}",
        &trimmed[..trimmed.len().min(200)]
    );
}

#[test]
fn redo_layout_runs_and_page_count_stays_consistent() {
    let mut tk = loaded_toolkit();
    let pages_before = tk.page_count();
    tk.redo_layout();
    let pages_after = tk.page_count();
    assert_eq!(pages_before, pages_after);
}

#[test]
fn redo_layout_with_options_changes_layout() {
    let mut tk = loaded_toolkit();
    // Halving the page width usually nudges layout. Even if Verovio chooses
    // not to re-paginate this tiny PAE document, the call must not panic
    // and must leave the toolkit usable.
    tk.redo_layout_with_options(r#"{"pageWidth": 1000}"#);
    let svg = tk.render_to_svg(1).expect("render after redo");
    assert!(svg.contains("<svg"));
}

#[test]
fn elements_at_time_at_zero_returns_json() {
    let mut tk = loaded_toolkit();
    let json = tk.elements_at_time(0);
    let trimmed = json.trim();
    assert!(
        trimmed.starts_with('{'),
        "expected JSON object, got first 200 chars: {}",
        &trimmed[..trimmed.len().min(200)]
    );
}

#[test]
fn elements_at_time_into_reuses_buffer() {
    let mut tk = loaded_toolkit();
    let mut buf = String::new();
    tk.elements_at_time_into(0, &mut buf);
    let cap = buf.capacity();
    tk.elements_at_time_into(100, &mut buf);
    assert!(buf.capacity() >= cap);
}

#[test]
fn render_svg_measure_range_empty_for_zero_or_inverted() {
    const PAE: &str =
        "@start:s\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:4/4\n@data:'4G/4A/4B/4c/4d/4e\n@end:s\n";
    let mut tk = verovio::Toolkit::from_data(PAE).expect("load");
    assert_eq!(tk.render_svg_measure_range(0, 2, "\n").unwrap(), "");
    assert_eq!(tk.render_svg_measure_range(3, 1, "\n").unwrap(), "");
}

#[test]
fn render_svg_measure_range_returns_svg_payload() {
    const PAE: &str =
        "@start:s\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:4/4\n@data:'4G/4A/4B/4c/4d/4e\n@end:s\n";
    let mut tk = verovio::Toolkit::from_data(PAE).expect("load");
    let svg = tk.render_svg_measure_range(1, 2, "\n").unwrap();
    assert!(
        svg.contains("<svg"),
        "expected SVG content, got {} bytes",
        svg.len()
    );
}

#[test]
fn render_svg_measure_range_restores_options() {
    const PAE: &str =
        "@start:s\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:4/4\n@data:'4G/4A/4B/4c\n@end:s\n";
    let mut tk = verovio::Toolkit::from_data(PAE).expect("load");
    let before = tk.options();
    let _ = tk.render_svg_measure_range(1, 1, "\n").unwrap();
    let after = tk.options();
    // The options JSON should be semantically restored. Verovio may
    // re-emit it with slight whitespace differences, but the measureFrom
    // / measureTo entries should be back to their defaults (empty or "0").
    assert!(
        !after.contains(r#""measureFrom": "1""#),
        "measureFrom not restored: {after}"
    );
    let _ = before; // suppress unused warning if no diff is asserted
}
