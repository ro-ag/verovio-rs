//! Tests for [`TempoMap`] — the wall-clock ↔ quarter-beat converter.

use verovio::{TempoChange, TempoMap, TimemapEvent, Toolkit};

fn tm_const_120() -> TempoMap {
    TempoMap::new(vec![TempoChange {
        at_qstamp: 0.0,
        bpm: 120.0,
    }])
}

fn tm_step_120_to_180_at_q4() -> TempoMap {
    TempoMap::new(vec![
        TempoChange {
            at_qstamp: 0.0,
            bpm: 120.0,
        },
        TempoChange {
            at_qstamp: 4.0,
            bpm: 180.0,
        },
    ])
}

#[test]
fn qstamp_to_ms_constant_tempo_is_simple_ratio() {
    let tm = tm_const_120();
    // 120 BPM → 500 ms per quarter beat.
    assert_eq!(tm.qstamp_to_ms(0.0), 0.0);
    assert!((tm.qstamp_to_ms(1.0) - 500.0).abs() < 1e-9);
    assert!((tm.qstamp_to_ms(4.0) - 2000.0).abs() < 1e-9);
}

#[test]
fn qstamp_to_ms_with_tempo_change_accumulates_correctly() {
    let tm = tm_step_120_to_180_at_q4();
    // [0, 4) at 120 BPM = 4 * 500 = 2000 ms
    // [4, 8) at 180 BPM = 4 * (60000/180) = 1333.333 ms
    assert!((tm.qstamp_to_ms(4.0) - 2000.0).abs() < 1e-9);
    assert!((tm.qstamp_to_ms(8.0) - (2000.0 + 4.0 * 60_000.0 / 180.0)).abs() < 1e-9);
    // Mid-segment (between q=4 and q=8): proportional.
    let q = 6.0;
    let expected = 2000.0 + (q - 4.0) * 60_000.0 / 180.0;
    assert!((tm.qstamp_to_ms(q) - expected).abs() < 1e-9);
}

#[test]
fn qstamp_to_ms_negative_and_zero_return_zero() {
    let tm = tm_step_120_to_180_at_q4();
    assert_eq!(tm.qstamp_to_ms(0.0), 0.0);
    assert_eq!(tm.qstamp_to_ms(-1.0), 0.0);
}

#[test]
fn ms_to_qstamp_inverts_qstamp_to_ms() {
    let tm = tm_step_120_to_180_at_q4();
    for q in [0.5, 1.0, 3.0, 4.0, 5.5, 8.0, 12.0] {
        let ms = tm.qstamp_to_ms(q);
        let q_back = tm.ms_to_qstamp(ms);
        assert!(
            (q - q_back).abs() < 1e-6,
            "round trip failed for q={q}: ms={ms}, back={q_back}"
        );
    }
}

#[test]
fn ms_to_qstamp_negative_and_zero_return_zero() {
    let tm = tm_step_120_to_180_at_q4();
    assert_eq!(tm.ms_to_qstamp(0.0), 0.0);
    assert_eq!(tm.ms_to_qstamp(-100.0), 0.0);
}

#[test]
fn from_timemap_collapses_duplicate_consecutive_tempos() {
    let timemap = vec![
        TimemapEvent {
            tstamp: 0.0,
            qstamp: 0.0,
            on: vec!["n1".into()],
            off: vec![],
            tempo: Some(120.0),
        },
        // No tempo change here — should be ignored.
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
            tempo: Some(120.0),
        },
    ];
    let tm = TempoMap::from_timemap(&timemap).expect("first event has tempo");
    // 120 → 120 isn't a real change; only one entry expected.
    assert_eq!(tm.changes.len(), 1);
    assert_eq!(tm.changes[0].bpm, 120.0);
}

#[test]
fn from_timemap_records_real_tempo_changes() {
    let timemap = vec![
        TimemapEvent {
            tstamp: 0.0,
            qstamp: 0.0,
            on: vec!["n1".into()],
            off: vec![],
            tempo: Some(120.0),
        },
        TimemapEvent {
            tstamp: 1000.0,
            qstamp: 2.0,
            on: vec!["n2".into()],
            off: vec!["n1".into()],
            tempo: Some(180.0),
        },
        TimemapEvent {
            tstamp: 2000.0,
            qstamp: 3.0,
            on: vec![],
            off: vec!["n2".into()],
            tempo: None,
        },
    ];
    let tm = TempoMap::from_timemap(&timemap).expect("first event has tempo");
    assert_eq!(tm.changes.len(), 2);
    assert_eq!(
        tm.changes[0],
        TempoChange {
            at_qstamp: 0.0,
            bpm: 120.0
        }
    );
    assert_eq!(
        tm.changes[1],
        TempoChange {
            at_qstamp: 2.0,
            bpm: 180.0
        }
    );
}

#[test]
fn from_timemap_empty_returns_none() {
    let timemap: Vec<TimemapEvent> = vec![];
    assert!(TempoMap::from_timemap(&timemap).is_none());
}

#[test]
fn toolkit_tempo_map_matches_from_timemap_constructed_manually() {
    const SAMPLE_PAE: &str =
        "@start:t\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:\n@data:'4G/4-\n@end:t\n";
    let mut tk = Toolkit::from_data(SAMPLE_PAE).unwrap();
    let direct = tk.tempo_map().expect("tempo_map").expect("non-empty");

    let timemap = tk.timemap().unwrap();
    let manual = TempoMap::from_timemap(&timemap).expect("non-empty");

    assert_eq!(direct, manual);
}
