//! Tests for the cache-aware lookup helpers in `verovio::lookup`.

use verovio::lookup::{sounding_at, sounding_at_into};
use verovio::{TimemapEvent, Toolkit};

const SAMPLE_PAE: &str = "\
@start:s
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:s
";

fn fixture_timemap() -> Vec<TimemapEvent> {
    vec![
        TimemapEvent {
            tstamp: 0.0,
            qstamp: 0.0,
            on: vec!["n1".into()],
            off: vec![],
            tempo: Some(120.0),
        },
        TimemapEvent {
            tstamp: 500.0,
            qstamp: 1.0,
            on: vec!["n2".into()],
            off: vec!["n1".into()],
            tempo: None,
        },
        TimemapEvent {
            tstamp: 1000.0,
            qstamp: 2.0,
            on: vec![],
            off: vec!["n2".into()],
            tempo: None,
        },
    ]
}

#[test]
fn sounding_at_before_anything_returns_empty() {
    let tm = fixture_timemap();
    let s = sounding_at(&tm, -1.0);
    assert!(s.is_empty(), "before t=0 nothing should sound, got {s:?}");
}

#[test]
fn sounding_at_first_onset_includes_the_note() {
    let tm = fixture_timemap();
    assert_eq!(sounding_at(&tm, 0.0), vec!["n1".to_string()]);
}

#[test]
fn sounding_at_between_events_returns_currently_active() {
    let tm = fixture_timemap();
    // Between 0 and 500ms, n1 is on alone.
    assert_eq!(sounding_at(&tm, 250.0), vec!["n1".to_string()]);
    // Between 500 and 1000ms, n2 is on (n1 turned off when n2 turned on).
    assert_eq!(sounding_at(&tm, 750.0), vec!["n2".to_string()]);
}

#[test]
fn sounding_at_event_boundary_keeps_ending_notes_sounding() {
    let tm = fixture_timemap();
    // At t=500 exactly, n1's off event fires AND n2's on event fires.
    // Verovio's `elements_at_time` semantics: n1 is still sounding (last
    // instant of the note, off hasn't fully released yet) AND n2 is
    // sounding (it begins at this instant). Matches the cross-check test
    // `sounding_at_matches_verovios_elements_at_time_at_key_moments`.
    let active = sounding_at(&tm, 500.0);
    assert!(
        active.contains(&"n1".to_string()),
        "n1 should still sound at its offset moment, got {active:?}"
    );
    assert!(
        active.contains(&"n2".to_string()),
        "n2 should sound at its onset moment, got {active:?}"
    );
}

#[test]
fn sounding_at_after_everything_returns_empty() {
    let tm = fixture_timemap();
    let s = sounding_at(&tm, 5000.0);
    assert!(
        s.is_empty(),
        "after all off-events nothing should sound, got {s:?}"
    );
}

#[test]
fn sounding_at_results_are_sorted_for_deterministic_consumers() {
    // BTreeSet output ordering means lookup answers are deterministic
    // and trivially comparable.
    let tm = vec![TimemapEvent {
        tstamp: 0.0,
        qstamp: 0.0,
        on: vec!["zeta".into(), "alpha".into(), "mu".into()],
        off: vec![],
        tempo: Some(120.0),
    }];
    let s = sounding_at(&tm, 0.0);
    assert_eq!(
        s,
        vec!["alpha".to_string(), "mu".to_string(), "zeta".to_string()]
    );
}

#[test]
fn sounding_at_into_reuses_buffer_capacity() {
    let tm = fixture_timemap();
    let mut buf = Vec::with_capacity(16);
    let initial_cap = buf.capacity();
    sounding_at_into(&tm, 250.0, &mut buf);
    assert_eq!(buf, vec!["n1".to_string()]);
    sounding_at_into(&tm, 750.0, &mut buf);
    assert_eq!(buf, vec!["n2".to_string()]);
    assert!(buf.capacity() >= initial_cap, "buffer capacity shrank");
}

#[test]
fn sounding_at_matches_verovios_elements_at_time_at_key_moments() {
    // Cross-check the cached lookup against Verovio's authoritative
    // `elements_at_time` for the real one-bar PAE fixture. They should
    // agree at every event boundary.
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let timemap = tk.timemap().expect("timemap parse");

    for ev in &timemap {
        let cached = sounding_at(&timemap, ev.tstamp);
        let upstream = tk.elements_at(ev.tstamp as u32).expect("elements parse");

        // We only check the notes field — `chords`, `measure`, `page` come
        // from doc structure that the timemap doesn't carry, so the cached
        // lookup intentionally ignores them.
        let mut upstream_notes = upstream.notes.clone();
        upstream_notes.sort();
        assert_eq!(
            cached, upstream_notes,
            "mismatch at tstamp={} qstamp={}: cached={:?} upstream={:?}",
            ev.tstamp, ev.qstamp, cached, upstream_notes,
        );
    }
}
