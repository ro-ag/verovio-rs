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
