//! Tests for [`Toolkit::is_loaded`], [`Toolkit::measure_at`],
//! [`Toolkit::measures`] and their pure-lookup counterparts.

use verovio::lookup::{measure_at_in, measures_from_events};
use verovio::Toolkit;

// In Plaine & Easie, `/` is a barline. `'4G/4-` produces 2 measures: a
// quarter G in the first, a quarter rest in the second.
const TWO_MEASURE_PAE: &str =
    "@start:s\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:\n@data:'4G/4-\n@end:s\n";

// Longer PAE — 8 quarter-note "measures" separated by barlines.
const MANY_MEASURE_PAE: &str =
    "@start:s\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:4/4\n@data:'4G/4A/4B/4c/'4d/4e/4f/4g\n@end:s\n";

// -- is_loaded ---------------------------------------------------------------

#[test]
fn is_loaded_false_on_fresh_toolkit() {
    let mut tk = Toolkit::new();
    assert!(!tk.is_loaded());
}

#[test]
fn is_loaded_true_after_successful_load() {
    let mut tk = Toolkit::from_data(TWO_MEASURE_PAE).expect("PAE load");
    assert!(tk.is_loaded());
}

// -- measures ----------------------------------------------------------------

#[test]
fn measures_two_measure_pae_produces_two_measures() {
    let mut tk = Toolkit::from_data(TWO_MEASURE_PAE).expect("PAE load");
    let measures = tk.measures().expect("measures");
    assert_eq!(measures.len(), 2, "got measures: {measures:?}");
    assert_eq!(measures[0].start_ms, 0.0, "first measure begins at t=0");
    assert!(
        measures[0].end_ms > measures[0].start_ms,
        "measure should have positive duration"
    );
    assert!(
        measures[1].start_ms > measures[0].start_ms,
        "second measure should start after first"
    );
}

#[test]
fn measures_start_qfrac_is_canonical_zero_one() {
    let mut tk = Toolkit::from_data(TWO_MEASURE_PAE).expect("PAE load");
    let measures = tk.measures().expect("measures");
    assert_eq!(measures[0].start_qfrac, [0, 1]);
}

#[test]
fn measures_are_sorted_by_start_ms() {
    let mut tk = Toolkit::from_data(MANY_MEASURE_PAE).expect("PAE load");
    let measures = tk.measures().expect("measures");
    for w in measures.windows(2) {
        assert!(
            w[0].start_ms <= w[1].start_ms,
            "measures out of order: {:?} then {:?}",
            w[0],
            w[1]
        );
    }
}

#[test]
fn measures_end_ms_matches_next_start_ms() {
    let mut tk = Toolkit::from_data(MANY_MEASURE_PAE).expect("PAE load");
    let measures = tk.measures().expect("measures");
    if measures.len() < 2 {
        return; // single-measure fixture, nothing to check
    }
    for w in measures.windows(2) {
        assert!(
            (w[0].end_ms - w[1].start_ms).abs() < 1e-6,
            "measure {} end {} should equal measure {} start {}",
            w[0].id,
            w[0].end_ms,
            w[1].id,
            w[1].start_ms
        );
    }
}

#[test]
fn measures_from_events_matches_toolkit_measures() {
    let mut tk = Toolkit::from_data(TWO_MEASURE_PAE).expect("PAE load");
    let events = tk.timemap_exact().expect("timemap");
    let direct = tk.measures().expect("measures");
    let from_events = measures_from_events(&events);
    assert_eq!(direct, from_events);
}

// -- measure_at --------------------------------------------------------------

#[test]
fn measure_at_zero_returns_first_measure() {
    let mut tk = Toolkit::from_data(TWO_MEASURE_PAE).expect("PAE load");
    let measures = tk.measures().expect("measures");
    let first = &measures[0];
    let resolved = tk.measure_at(0.0).expect("measure_at").expect("Some(id)");
    assert_eq!(resolved, first.id);
}

#[test]
fn measure_at_before_first_measure_returns_none() {
    let mut tk = Toolkit::from_data(TWO_MEASURE_PAE).expect("PAE load");
    let resolved = tk.measure_at(-100.0).expect("measure_at");
    assert!(
        resolved.is_none(),
        "negative ms should be before any measure, got {resolved:?}"
    );
}

#[test]
fn measure_at_within_measure_resolves_to_its_id() {
    let mut tk = Toolkit::from_data(MANY_MEASURE_PAE).expect("PAE load");
    let measures = tk.measures().expect("measures");
    for m in &measures {
        let mid = (m.start_ms + m.end_ms) / 2.0;
        let resolved = tk
            .measure_at(mid)
            .expect("measure_at")
            .expect("some measure should enclose mid-measure ms");
        assert_eq!(
            resolved, m.id,
            "mid-ms {mid} of measure {} should resolve to itself, got {resolved}",
            m.id
        );
    }
}

#[test]
fn measure_at_in_pure_helper_matches_toolkit_method() {
    let mut tk = Toolkit::from_data(MANY_MEASURE_PAE).expect("PAE load");
    let events = tk.timemap_exact().expect("timemap");
    // Pick a mid-document tstamp; the helper and method should agree.
    let probe_ms = events[events.len() / 2].tstamp;
    let via_method = tk.measure_at(probe_ms).expect("measure_at");
    let via_helper = measure_at_in(&events, probe_ms).map(String::from);
    assert_eq!(via_method, via_helper);
}
