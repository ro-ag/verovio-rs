//! Tests for the typed JSON accessors: `Toolkit::timemap`,
//! `Toolkit::elements_at`. The shapes here mirror what Verovio emits
//! (verified by inspection — see `peek` probe in commit history).

use verovio::Toolkit;

const SAMPLE_PAE: &str = "\
@start:t
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:t
";

fn loaded() -> Toolkit {
    let mut tk = Toolkit::new();
    tk.load_data(SAMPLE_PAE).expect("PAE fixture should parse");
    tk
}

#[test]
fn timemap_parses_into_typed_events() {
    let mut tk = loaded();
    let timemap = tk.timemap().expect("timemap parse");
    assert!(!timemap.is_empty(), "expected at least one event");

    // The first event should carry the tempo and an `on` set for the first
    // articulation; subsequent events have `off` etc. This pins the shape.
    let first = &timemap[0];
    assert_eq!(first.tstamp, 0.0, "first event should be at t=0");
    assert!(first.tempo.is_some(), "first event should publish tempo");
    assert!(
        !first.on.is_empty(),
        "first event should have at least one onset"
    );
}

#[test]
fn timemap_events_are_chronologically_ordered() {
    let mut tk = loaded();
    let timemap = tk.timemap().unwrap();
    for w in timemap.windows(2) {
        assert!(
            w[0].tstamp <= w[1].tstamp,
            "events out of order: {:?} then {:?}",
            w[0],
            w[1]
        );
    }
}

#[test]
fn elements_at_returns_typed_struct_for_loaded_doc() {
    let mut tk = loaded();
    let elements = tk.elements_at(0).expect("elements parse");
    // At t=0 we expect to be in the first measure with at least one note.
    assert!(
        elements.measure.is_some(),
        "expected enclosing measure at t=0, got {elements:?}"
    );
    assert!(!elements.notes.is_empty(), "expected onset at t=0");
    assert_eq!(elements.page, Some(1));
}

#[test]
fn elements_at_empty_doc_returns_default() {
    let mut tk = Toolkit::new();
    let elements = tk.elements_at(0).expect("empty json parses fine");
    // The safe wrapper returns "{}" for empty docs; serde fills defaults.
    assert_eq!(elements.notes, Vec::<String>::new());
    assert_eq!(elements.measure, None);
    assert_eq!(elements.page, None);
}

#[test]
fn timemap_round_trips_through_serde() {
    let mut tk = loaded();
    let original = tk.timemap().unwrap();
    let serialized = serde_json::to_string(&original).expect("serialize");
    let reparsed: verovio::Timemap = serde_json::from_str(&serialized).expect("reparse");
    assert_eq!(original, reparsed);
}
