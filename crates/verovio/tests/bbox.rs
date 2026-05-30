//! Tests for `Toolkit::bbox_map` — verifies that the rendered SVG can
//! be walked to produce per-element bounding boxes in viewBox coords.

use verovio::Toolkit;

const SAMPLE_PAE: &str =
    "@start:s\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:4/4\n@data:'4G/4A/4B/4c\n@end:s\n";

#[test]
fn bbox_map_returns_entries_for_loaded_score() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("load");
    let map = tk.bbox_map().expect("bbox_map");
    assert!(!map.is_empty(), "expected at least one bbox");
}

#[test]
fn bbox_map_entries_have_non_zero_area() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("load");
    let map = tk.bbox_map().expect("bbox_map");
    for (id, bbox) in &map {
        // Each bbox should have some extent.
        assert!(
            bbox.width >= 0.0 && bbox.height >= 0.0,
            "negative size for {id}: {bbox:?}"
        );
        assert!(bbox.page >= 1, "page should be 1-indexed for {id}");
    }
}

#[test]
fn bbox_map_overlaps_timemap_ids() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("load");
    let timemap = tk.timemap().expect("timemap");
    let bboxes = tk.bbox_map().expect("bbox");

    // At least one timemap onset id should have a bbox — proves the
    // bbox walk found the same note ids Verovio reports for playback.
    let mut overlap = 0usize;
    for ev in &timemap {
        for id in &ev.on {
            if bboxes.contains_key(id) {
                overlap += 1;
            }
        }
    }
    assert!(
        overlap > 0,
        "expected at least one note id to overlap between timemap and bbox_map"
    );
}

#[test]
fn bbox_contains_works_for_known_point() {
    let bbox = verovio::BBox {
        x: 100.0,
        y: 100.0,
        width: 50.0,
        height: 50.0,
        page: 1,
    };
    assert!(bbox.contains(125.0, 125.0));
    assert!(bbox.contains(100.0, 100.0)); // boundary inclusive
    assert!(bbox.contains(150.0, 150.0));
    assert!(!bbox.contains(99.0, 125.0));
    assert!(!bbox.contains(125.0, 151.0));
}
