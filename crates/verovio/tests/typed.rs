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
fn elements_at_empty_doc_returns_render_failed() {
    let mut tk = Toolkit::new();
    // Empty document → render-family error, consistent with the rest of
    // the render surface. Callers that want a degrade-to-empty path can
    // map the error: `tk.elements_at(0).unwrap_or_default()`.
    let res = tk.elements_at(0);
    assert!(matches!(res, Err(verovio::Error::RenderFailed { page: 0 })));
}

#[test]
fn timemap_exact_returns_rational_quarter_beats() {
    let mut tk = loaded();
    let events = tk.timemap_exact().expect("exact timemap");
    assert!(!events.is_empty());

    // First event is at the start, qfrac == 0/1.
    assert_eq!(events[0].qfrac, [0, 1]);

    // Every qfrac denominator must be positive (otherwise the fraction is
    // ill-formed — Verovio shouldn't ever emit den=0, but pin it).
    for ev in &events {
        assert!(ev.qfrac[1] > 0, "denominator must be positive: {ev:?}");
    }
}

#[test]
fn timemap_exact_includes_rest_and_measure_markers() {
    let mut tk = loaded();
    let events = tk.timemap_exact().expect("exact timemap");

    // Verovio's `includeMeasures` option is on by default in timemap_exact —
    // at least one event in the one-bar fixture should carry the enclosing
    // measure ID (the first event always does).
    assert!(
        events.iter().any(|ev| ev.measure_on.is_some()),
        "expected at least one measureOn event in timemap_exact"
    );

    // The fixture's PAE `'4G/4-` is quarter-G followed by a quarter rest.
    // With `includeRests` on, the rest should turn on at q=1/1 and off at q=2/1.
    let rest_on_event = events
        .iter()
        .find(|ev| !ev.rests_on.is_empty())
        .expect("expected a rest-on event in the one-bar PAE");
    assert_eq!(rest_on_event.qfrac, [1, 1], "rest should turn on at q=1");
}

#[test]
fn quarter_beats_helper_returns_canonical_pair() {
    let mut tk = loaded();
    let events = tk.timemap_exact().expect("exact timemap");

    for ev in &events {
        let (num, den) = ev.quarter_beats();
        assert_eq!(num, ev.qfrac[0]);
        assert_eq!(den, ev.qfrac[1]);
    }
}

#[test]
fn tstamp_ms_at_tempo_matches_verovios_tstamp_at_published_tempo() {
    let mut tk = loaded();
    let events = tk.timemap_exact().expect("exact timemap");
    // The first event publishes the tempo (120 BPM for our fixture).
    let bpm = events[0].tempo.expect("first event should publish tempo");
    for ev in &events {
        let computed_ms = ev.tstamp_ms_at_tempo(bpm);
        let upstream_ms = ev.tstamp;
        // Verovio rounds tstamp to the nearest f64 ms; our helper is the
        // raw f64 arithmetic, so they should agree to within 1 µs.
        assert!(
            (computed_ms - upstream_ms).abs() < 1e-3,
            "computed {computed_ms} vs upstream {upstream_ms} at q={:?}",
            ev.qfrac,
        );
    }
}

#[test]
fn expansion_map_is_empty_for_score_without_repeats() {
    let mut tk = loaded();
    let expansion = tk.expansion_map().expect("expansion map parse");
    // Our PAE fixture has no <expansion> markers, so Verovio's
    // ExportExpansionMap returns "{}" — an empty BTreeMap.
    assert!(
        expansion.is_empty(),
        "expected empty expansion map for repeat-free score, got {expansion:?}"
    );
}

#[test]
fn render_to_expansion_map_returns_json_object() {
    let mut tk = loaded();
    let json = tk.render_to_expansion_map().expect("expansion map render");
    assert!(json.trim().starts_with('{'));
    assert!(json.trim().ends_with('}'));
}

#[test]
fn expansion_map_unloaded_doc_returns_err() {
    let mut tk = verovio::Toolkit::new();
    let res = tk.expansion_map();
    assert!(
        matches!(res, Err(verovio::Error::RenderFailed { page: 0 })),
        "got {res:?}"
    );
}

#[test]
fn timemap_round_trips_through_serde() {
    let mut tk = loaded();
    let original = tk.timemap().unwrap();
    let serialized = serde_json::to_string(&original).expect("serialize");
    let reparsed: verovio::Timemap = serde_json::from_str(&serialized).expect("reparse");
    assert_eq!(original, reparsed);
}
