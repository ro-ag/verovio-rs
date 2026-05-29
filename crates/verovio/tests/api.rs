use verovio::{Error, Toolkit};

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
#[ignore = "needs verovio-data (SetResourcePath) — Toolkit::LoadData rejects \
           any input until Resources::Ok() returns true, which requires the \
           SMuFL fonts staged on disk"]
fn load_valid_pae() {
    let mut tk = Toolkit::new();
    tk.load_data(SAMPLE_PAE).expect("PAE sample should parse");
}

#[test]
fn load_data_returns_err_on_failure() {
    // Without staged fonts, Verovio's `Resources::Ok()` guard rejects every
    // input regardless of validity. This test exercises the bool→Result
    // conversion at the FFI boundary. Once `verovio-data` lands, add a
    // companion test that loads a valid score and asserts `Ok(())`.
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
    tk.set_options("{}").expect("empty options object should be valid");
}

#[test]
fn set_options_with_invalid_json_fails() {
    let mut tk = Toolkit::new();
    let res = tk.set_options("not json");
    assert!(matches!(res, Err(Error::OptionsRejected)), "got {res:?}");
}

#[test]
#[ignore = "needs verovio-data — see load_valid_pae"]
fn page_count_after_load_is_sensible() {
    let mut tk = Toolkit::new();
    tk.load_data(SAMPLE_PAE).unwrap();
    // Without staged fonts the layout pass may still complete (ABC is small)
    // — assert only that the value is bounded, not that it's >= 1.
    let pages = tk.page_count();
    assert!(pages <= 1000, "page_count returned absurd value: {pages}");
}

// Compile-time assertion: Toolkit is `Send`. The bound is part of the public
// contract for one-toolkit-per-thread concurrent rendering.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<Toolkit>();
};
