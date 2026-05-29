use verovio::{Error, LogLevel, Toolkit};

// A known-good Plaine & Easie sample, copied from Verovio's own test fixtures
// (`doc/tests/pae/4_duration/05_dur-4th.pae`). Verovio auto-detects format
// from content; PAE is the most compact format upstream supports.
const SAMPLE_PAE: &str = "\
@start:clefs
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:clefs
";

#[test]
fn load_valid_pae() {
    let mut tk = Toolkit::new();
    tk.load_data(SAMPLE_PAE).expect("PAE sample should parse");
}

#[test]
fn from_data_constructs_and_loads() {
    let tk = Toolkit::from_data(SAMPLE_PAE).expect("from_data should succeed");
    // Construction + load both succeeded; whether page_count > 0 is checked
    // separately. Just confirm we got a usable toolkit.
    assert!(!tk.version().is_empty());
}

#[test]
fn load_file_round_trips_a_pae_score() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("score.pae");
    std::fs::write(&path, SAMPLE_PAE).expect("write fixture");

    let mut tk = Toolkit::new();
    tk.load_file(&path).expect("load_file should succeed");
}

#[test]
fn from_file_constructs_and_loads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("score.pae");
    std::fs::write(&path, SAMPLE_PAE).expect("write fixture");

    let tk = Toolkit::from_file(&path).expect("from_file should succeed");
    assert!(!tk.version().is_empty());
}

#[test]
fn load_file_missing_path_returns_io_error() {
    let mut tk = Toolkit::new();
    let res = tk.load_file("/this/path/does/not/exist.pae");
    assert!(
        matches!(res, Err(Error::Io(_))),
        "expected Io error, got {res:?}"
    );
}

#[test]
fn from_file_propagates_load_failed_for_bad_content() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("garbage.txt");
    std::fs::write(&path, "this is not music notation").expect("write");

    let res = Toolkit::from_file(&path);
    assert!(
        matches!(res, Err(Error::LoadFailed)),
        "expected LoadFailed, got {res:?}"
    );
}

#[test]
fn load_invalid_data_fails() {
    let mut tk = Toolkit::new();
    let res = tk.load_data("this is not music notation");
    assert!(matches!(res, Err(Error::LoadFailed)), "got {res:?}");
}

#[test]
fn default_options_is_json_object() {
    let tk = Toolkit::new();
    let opts = tk.default_options();
    assert!(
        opts.starts_with('{') && opts.trim_end().ends_with('}'),
        "expected JSON object, got: {opts}"
    );
    assert!(opts.len() > 100, "default options should be substantial");
}

#[test]
fn options_is_json_object() {
    let tk = Toolkit::new();
    let opts = tk.options();
    assert!(
        opts.starts_with('{') && opts.trim_end().ends_with('}'),
        "expected JSON object, got: {opts}"
    );
}

#[test]
fn set_empty_options_succeeds() {
    let mut tk = Toolkit::new();
    tk.set_options("{}")
        .expect("empty options object should be valid");
}

#[test]
fn set_options_with_invalid_json_fails() {
    let mut tk = Toolkit::new();
    let res = tk.set_options("not json");
    assert!(matches!(res, Err(Error::OptionsRejected)), "got {res:?}");
}

#[test]
fn page_count_after_load_is_at_least_one() {
    let mut tk = Toolkit::new();
    tk.load_data(SAMPLE_PAE).unwrap();
    let pages = tk.page_count();
    assert!(pages >= 1, "expected at least one page, got {pages}");
    assert!(pages <= 1000, "page_count returned absurd value: {pages}");
}

#[test]
fn set_log_level_off_silences_subsequent_calls() {
    // Cannot easily intercept Verovio's stdout from the test harness, so
    // assert only that the API call is reachable and doesn't panic /
    // poison the internal mutex. Manual inspection of test output shows
    // the `[Warning]` chatter from render-out-of-range tests goes away
    // when this is wired in at process start.
    verovio::set_log_level(LogLevel::Off);
    let mut tk = Toolkit::new();
    tk.load_data(SAMPLE_PAE).unwrap();
    let _ = tk.render_to_svg(1).unwrap();

    // Restore default so concurrent tests in the same process see warnings.
    verovio::set_log_level(LogLevel::Warning);
}

// Compile-time assertion: Toolkit is `Send`. The bound is part of the public
// contract for one-toolkit-per-thread concurrent rendering.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<Toolkit>();
};

/// Runtime smoke test for the `Send + !Sync` claim: spin up another toolkit
/// on a separate thread while a primary toolkit runs on this thread. If
/// Verovio's process-global state (m_humdrumBuffer, log buffer, locale) leaked
/// into our exposed surface, this would race or deadlock. We deliberately
/// avoid touching any of those — no Humdrum methods, no `GetLog`, no
/// `SetLocale` — so this stays well-defined.
#[test]
fn two_toolkits_in_parallel_threads_each_render_independently() {
    const SAMPLE: &str = "\
@start:t
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:t
";

    let handle = std::thread::spawn(|| {
        let mut tk = Toolkit::new();
        tk.load_data(SAMPLE).unwrap();
        let svg = tk.render_to_svg(1).unwrap();
        assert!(svg.contains("<svg"));
    });

    let mut tk = Toolkit::new();
    tk.load_data(SAMPLE).unwrap();
    let svg = tk.render_to_svg(1).unwrap();
    assert!(svg.contains("<svg"));

    handle.join().expect("parallel toolkit thread panicked");
}
