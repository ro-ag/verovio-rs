//! Concurrency stress tests. The safe wrapper makes two thread-safety claims:
//!
//!   1. `Toolkit::new` is safe to call from many threads at once — the
//!      `OnceLock<PathBuf>` that owns the resource tempdir initializes
//!      exactly once even under contention.
//!   2. `set_log_level` is safe to call from many threads at once — the
//!      private mutex serializes the FFI call to `vrv::EnableLog`.
//!
//! These tests don't prove the claims (race conditions are timing-sensitive
//! and may pass spuriously); they cover the obvious break cases and
//! ThreadSanitizer in CI's `sanitize-thread` job slot does the harder work.

use std::thread;

use verovio::{LogLevel, Toolkit};

const SAMPLE_PAE: &str = "\
@start:t
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:t
";

#[test]
fn many_concurrent_toolkit_news_all_succeed() {
    // The first call to Toolkit::new triggers the resource-tempdir
    // extraction (a ~12 MB include_dir → disk write); subsequent calls
    // hit the OnceLock fast path. Run 16 threads to maximize the
    // probability of two of them entering the OnceLock initializer
    // simultaneously.
    let handles: Vec<_> = (0..16)
        .map(|_| {
            thread::spawn(|| {
                let mut tk = Toolkit::new();
                tk.load_data(SAMPLE_PAE).unwrap();
                tk.page_count()
            })
        })
        .collect();

    let counts: Vec<u32> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(counts.iter().all(|&n| n >= 1), "got {counts:?}");
}

#[test]
fn concurrent_set_log_level_does_not_deadlock_or_corrupt() {
    let handles: Vec<_> = (0..32)
        .map(|i| {
            thread::spawn(move || {
                // Mix levels so we exercise multiple paths through the mutex.
                let level = match i % 5 {
                    0 => LogLevel::Off,
                    1 => LogLevel::Error,
                    2 => LogLevel::Warning,
                    3 => LogLevel::Info,
                    _ => LogLevel::Debug,
                };
                verovio::set_log_level(level);
            })
        })
        .collect();

    for (i, h) in handles.into_iter().enumerate() {
        h.join()
            .unwrap_or_else(|e| panic!("set_log_level thread {i} panicked: {e:?}"));
    }

    // Reset to default so subsequent tests in the same binary don't see
    // logs from this stress run.
    verovio::set_log_level(LogLevel::Warning);
}

#[test]
fn render_pipeline_runs_concurrently_on_distinct_toolkits() {
    // Each thread loads the same input, renders page 1, and asserts the SVG
    // looks well-formed. We do NOT share a toolkit between threads (that
    // would be unsound — `Toolkit: !Sync`).
    let handles: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(|| {
                let mut tk = Toolkit::new();
                tk.load_data(SAMPLE_PAE).unwrap();
                let svg = tk.render_to_svg(1).expect("render page 1");
                assert!(svg.contains("<svg"));
                assert!(svg.contains("</svg>"));
                let timemap = tk.timemap().expect("timemap");
                assert!(!timemap.is_empty());
            })
        })
        .collect();

    for (i, h) in handles.into_iter().enumerate() {
        h.join()
            .unwrap_or_else(|e| panic!("render thread {i} panicked: {e:?}"));
    }
}

// ---------------------------------------------------------------------------
// New coverage for the post-Phase-3 additions.
// ---------------------------------------------------------------------------

const TWO_STAFF_MEI: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mei xmlns="http://www.music-encoding.org/ns/mei" meiversion="4.0.0">
  <meiHead><fileDesc><titleStmt><title>conc</title></titleStmt><pubStmt/></fileDesc></meiHead>
  <music><body><mdiv><score>
    <scoreDef><staffGrp>
      <staffDef n="1" lines="5" clef.shape="G" clef.line="2"/>
      <staffDef n="2" lines="5" clef.shape="F" clef.line="4"/>
    </staffGrp></scoreDef>
    <section><measure>
      <staff n="1"><layer>
        <note pname="g" oct="4" dur="4" xml:id="t1"/>
        <note pname="g" oct="4" dur="4" xml:id="t2"/>
      </layer></staff>
      <staff n="2"><layer>
        <note pname="c" oct="3" dur="4" xml:id="b1"/>
        <note pname="c" oct="3" dur="4" xml:id="b2"/>
      </layer></staff>
    </measure></section>
  </score></mdiv></body></music></mei>"#;

#[test]
fn concurrent_midi_policy_application_per_toolkit() {
    // Verovio's internal log buffer is process-global; mute it so the
    // concurrent renders below don't race on stdout. (Render-time
    // correctness doesn't depend on the buffer; this just keeps output
    // clean and avoids the documented soft race on the log buffer.)
    verovio::set_log_level(LogLevel::Off);

    use std::collections::BTreeMap;
    use verovio::midi::{summarize, MidiTrackPolicy, TrackOverride};

    let handles: Vec<_> = (0..8)
        .map(|i| {
            thread::spawn(move || {
                let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
                let mut overrides = BTreeMap::new();
                overrides.insert(
                    1,
                    TrackOverride {
                        program: Some(i as u8),
                        volume: Some(80 + (i as u8 * 2)),
                        ..Default::default()
                    },
                );
                let policy = MidiTrackPolicy {
                    overrides,
                    auto_distribute_channels: true,
                    ..MidiTrackPolicy::default()
                };
                let bytes = tk
                    .render_to_midi_bytes_with_policy(&policy)
                    .expect("policy");
                let infos = summarize(&bytes).expect("summarize");
                (infos[1].program, infos[1].volume, infos[1].channels.clone())
            })
        })
        .collect();

    for (i, h) in handles.into_iter().enumerate() {
        let (program, volume, channels) = h.join().unwrap_or_else(|e| panic!("thread {i}: {e:?}"));
        assert_eq!(program, Some(i as u8));
        assert_eq!(volume, Some(80 + (i as u8 * 2)));
        assert_eq!(channels, vec![0]);
    }

    verovio::set_log_level(LogLevel::Warning);
}

#[test]
fn concurrent_staff_map_per_toolkit() {
    verovio::set_log_level(LogLevel::Off);

    let handles: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(|| {
                let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
                let map = tk.staff_map().expect("staff_map");
                assert_eq!(map.get("t1"), Some(&1));
                assert_eq!(map.get("b1"), Some(&2));
            })
        })
        .collect();

    for (i, h) in handles.into_iter().enumerate() {
        h.join().unwrap_or_else(|e| panic!("thread {i}: {e:?}"));
    }

    verovio::set_log_level(LogLevel::Warning);
}

#[test]
fn concurrent_measures_classified_tempo_per_toolkit() {
    verovio::set_log_level(LogLevel::Off);

    let handles: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(|| {
                let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
                let measures = tk.measures().expect("measures");
                let classified = tk.classified_elements().expect("classify");
                let tempo = tk.tempo_map().expect("tempo_map");
                assert!(!measures.is_empty());
                assert!(!classified.is_empty());
                assert!(tempo.is_some());
            })
        })
        .collect();

    for (i, h) in handles.into_iter().enumerate() {
        h.join().unwrap_or_else(|e| panic!("thread {i}: {e:?}"));
    }

    verovio::set_log_level(LogLevel::Warning);
}

#[test]
fn pure_apply_track_policy_and_summarize_are_thread_safe_on_shared_input() {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use verovio::midi::{apply_track_policy, summarize, MidiTrackPolicy, TrackOverride};

    verovio::set_log_level(LogLevel::Off);

    // Render once, share the bytes across threads.
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let bytes = Arc::new(tk.render_to_midi_bytes().expect("midi bytes"));

    let handles: Vec<_> = (0..32)
        .map(|i| {
            let bytes = Arc::clone(&bytes);
            thread::spawn(move || {
                let mut overrides = BTreeMap::new();
                overrides.insert(
                    1,
                    TrackOverride {
                        program: Some((i % 128) as u8),
                        ..Default::default()
                    },
                );
                let policy = MidiTrackPolicy {
                    overrides,
                    auto_distribute_channels: true,
                    ..MidiTrackPolicy::default()
                };
                let rewritten = apply_track_policy(&bytes, &policy).expect("apply");
                let infos = summarize(&rewritten).expect("summarize");
                infos[1].program
            })
        })
        .collect();

    for (i, h) in handles.into_iter().enumerate() {
        let program = h.join().unwrap_or_else(|e| panic!("thread {i}: {e:?}"));
        assert_eq!(program, Some((i % 128) as u8));
    }

    verovio::set_log_level(LogLevel::Warning);
}

#[test]
fn lookup_sounding_at_is_safe_across_shared_timemap() {
    use std::sync::Arc;
    use verovio::lookup::sounding_at;

    verovio::set_log_level(LogLevel::Off);

    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let timemap = Arc::new(tk.timemap().expect("timemap"));

    let handles: Vec<_> = (0..32)
        .map(|i| {
            let tm = Arc::clone(&timemap);
            thread::spawn(move || {
                // Each thread queries a different time; outputs are
                // mutually consistent because the underlying timemap is
                // immutable while shared.
                let ms = (i as f64) * 50.0;
                let active = sounding_at(&tm, ms);
                let _ = active.len();
            })
        })
        .collect();

    for (i, h) in handles.into_iter().enumerate() {
        h.join().unwrap_or_else(|e| panic!("thread {i}: {e:?}"));
    }

    verovio::set_log_level(LogLevel::Warning);
}

#[cfg(feature = "png")]
#[test]
fn concurrent_png_render_per_toolkit() {
    verovio::set_log_level(LogLevel::Off);

    let handles: Vec<_> = (0..4)
        .map(|_| {
            thread::spawn(|| {
                let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
                let png = tk.render_to_png(1, 1.0).expect("png");
                assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
            })
        })
        .collect();

    for (i, h) in handles.into_iter().enumerate() {
        h.join().unwrap_or_else(|e| panic!("thread {i}: {e:?}"));
    }

    verovio::set_log_level(LogLevel::Warning);
}

// ---------------------------------------------------------------------------
// Compile-time Send + Sync assertions for every public type a consumer
// might cache, share, or move across threads. The const-fn block runs at
// type-check time; a regression here breaks the build, not just a test.
// ---------------------------------------------------------------------------

const _: fn() = || {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    // Already covered (Toolkit: Send, !Sync) in tests/api.rs.

    // Owned data types — should be Send + Sync.
    assert_send::<verovio::TimemapEvent>();
    assert_sync::<verovio::TimemapEvent>();
    assert_send::<verovio::TimemapEventExact>();
    assert_sync::<verovio::TimemapEventExact>();
    assert_send::<verovio::ElementsAtTime>();
    assert_sync::<verovio::ElementsAtTime>();
    assert_send::<verovio::ExpansionMap>();
    assert_sync::<verovio::ExpansionMap>();
    assert_send::<verovio::TempoMap>();
    assert_sync::<verovio::TempoMap>();
    assert_send::<verovio::TempoChange>();
    assert_sync::<verovio::TempoChange>();
    assert_send::<verovio::ClassifiedElements>();
    assert_sync::<verovio::ClassifiedElements>();
    assert_send::<verovio::ElementKind>();
    assert_sync::<verovio::ElementKind>();
    assert_send::<verovio::MeasureInfo>();
    assert_sync::<verovio::MeasureInfo>();
    assert_send::<verovio::SvgOptions>();
    assert_sync::<verovio::SvgOptions>();
    assert_send::<verovio::MidiOptions>();
    assert_sync::<verovio::MidiOptions>();
    assert_send::<verovio::Error>();
    assert_sync::<verovio::Error>();

    // midi module types — needed for thread-pool policy distribution.
    assert_send::<verovio::midi::TrackOverride>();
    assert_sync::<verovio::midi::TrackOverride>();
    assert_send::<verovio::midi::MidiTrackPolicy>();
    assert_sync::<verovio::midi::MidiTrackPolicy>();
    assert_send::<verovio::midi::TrackInfo>();
    assert_sync::<verovio::midi::TrackInfo>();
};
