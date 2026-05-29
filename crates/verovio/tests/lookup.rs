//! Tests for the cache-aware lookup helpers in `verovio::lookup`.

use verovio::lookup::{
    duration_ms, events_in_range, next_event_after, prev_event_before, sounding_at,
    sounding_at_into, PlaybackCursor,
};
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

// -- duration_ms ------------------------------------------------------------

#[test]
fn duration_ms_returns_last_event_tstamp() {
    let tm = fixture_timemap();
    assert_eq!(duration_ms(&tm), 1000.0);
}

#[test]
fn duration_ms_empty_timemap_returns_zero() {
    let tm: Vec<TimemapEvent> = vec![];
    assert_eq!(duration_ms(&tm), 0.0);
}

// -- events_in_range --------------------------------------------------------

#[test]
fn events_in_range_returns_inclusive_slice() {
    let tm = fixture_timemap();
    // Inclusive on both ends.
    let slice = events_in_range(&tm, 0.0, 500.0);
    assert_eq!(slice.len(), 2);
    assert_eq!(slice[0].tstamp, 0.0);
    assert_eq!(slice[1].tstamp, 500.0);
}

#[test]
fn events_in_range_with_inverted_bounds_returns_empty() {
    let tm = fixture_timemap();
    assert!(events_in_range(&tm, 500.0, 0.0).is_empty());
}

#[test]
fn events_in_range_misses_open_intervals() {
    let tm = fixture_timemap();
    // No events in (500, 1000).
    let slice = events_in_range(&tm, 500.1, 999.9);
    assert!(slice.is_empty(), "got {slice:?}");
}

#[test]
fn events_in_range_with_huge_window_returns_all() {
    let tm = fixture_timemap();
    let slice = events_in_range(&tm, f64::NEG_INFINITY, f64::INFINITY);
    assert_eq!(slice.len(), tm.len());
}

// -- next_event_after / prev_event_before -----------------------------------

#[test]
fn next_event_after_returns_strictly_later_event() {
    let tm = fixture_timemap();
    assert_eq!(next_event_after(&tm, -100.0).map(|e| e.tstamp), Some(0.0));
    assert_eq!(next_event_after(&tm, 0.0).map(|e| e.tstamp), Some(500.0));
    assert_eq!(next_event_after(&tm, 499.9).map(|e| e.tstamp), Some(500.0));
    assert_eq!(next_event_after(&tm, 500.0).map(|e| e.tstamp), Some(1000.0));
    assert_eq!(next_event_after(&tm, 1000.0), None);
    assert_eq!(next_event_after(&tm, 9999.0), None);
}

#[test]
fn prev_event_before_returns_strictly_earlier_event() {
    let tm = fixture_timemap();
    assert_eq!(prev_event_before(&tm, -100.0), None);
    assert_eq!(prev_event_before(&tm, 0.0), None);
    assert_eq!(prev_event_before(&tm, 0.1).map(|e| e.tstamp), Some(0.0));
    assert_eq!(prev_event_before(&tm, 500.0).map(|e| e.tstamp), Some(0.0));
    assert_eq!(
        prev_event_before(&tm, 1000.0).map(|e| e.tstamp),
        Some(500.0)
    );
    assert_eq!(
        prev_event_before(&tm, 9999.0).map(|e| e.tstamp),
        Some(1000.0)
    );
}

// -- PlaybackCursor ---------------------------------------------------------

#[test]
fn cursor_advance_matches_sounding_at_at_each_tick() {
    let tm = fixture_timemap();
    let mut cursor = PlaybackCursor::new(&tm);

    // Tick through a fine grid that crosses every event boundary.
    for tick in 0..=12 {
        let ms = tick as f64 * 100.0;
        let cursor_set = cursor.advance_to(ms).clone();
        let cursor_vec: Vec<String> = cursor_set.into_iter().collect();
        let oneshot = sounding_at(&tm, ms);
        assert_eq!(
            cursor_vec, oneshot,
            "cursor disagrees with sounding_at at ms={ms}"
        );
    }
}

#[test]
fn cursor_idempotent_at_same_ms() {
    let tm = fixture_timemap();
    let mut cursor = PlaybackCursor::new(&tm);
    let first = cursor.advance_to(500.0).clone();
    let second = cursor.advance_to(500.0).clone();
    assert_eq!(
        first, second,
        "calling advance_to twice with same ms drifted"
    );
}

#[test]
fn cursor_seek_to_rewinds_correctly() {
    let tm = fixture_timemap();
    let mut cursor = PlaybackCursor::new(&tm);
    let _ = cursor.advance_to(800.0);
    let after_seek = cursor.seek_to(250.0).clone();
    let oneshot = sounding_at(&tm, 250.0);
    let cursor_vec: Vec<String> = after_seek.into_iter().collect();
    assert_eq!(
        cursor_vec, oneshot,
        "seek_to didn't rewind to the right state"
    );
    assert_eq!(cursor.position_ms(), 250.0);
}

#[test]
fn cursor_off_at_boundary_releases_on_next_tick() {
    let tm = fixture_timemap();
    let mut cursor = PlaybackCursor::new(&tm);
    // At exactly t=500, n1 is still sounding (off event hasn't fired yet
    // in the boundary semantic). The off applies only when we move past.
    assert!(cursor.advance_to(500.0).contains("n1"));
    assert!(cursor.advance_to(501.0).contains("n2"));
    assert!(!cursor.advance_to(501.0).contains("n1"));
}

#[test]
fn cursor_at_event_boundary_includes_onsets() {
    let tm = fixture_timemap();
    let mut cursor = PlaybackCursor::new(&tm);
    // t=0 is the first event's tstamp; n1 should be sounding immediately.
    let set = cursor.advance_to(0.0);
    assert!(set.contains("n1"));
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
