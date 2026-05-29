//! Criterion benchmarks for the rendering surface.
//!
//! Run with `cargo bench` (release profile, full optimization). The headline
//! comparison is `render_to_svg` vs `render_to_svg_into` for the buffer-reuse
//! claim — the latter should be measurably faster on a hot loop because the
//! caller's `String` allocation amortizes across iterations.
//!
//! Headline numbers from `cargo bench --bench render -- --quick` on a
//! 24-core Ryzen NixOS machine, Verovio 6.2.1 release build (criterion
//! `--quick` mode; expect ±5% per machine):
//!
//! ```text
//! svg/
//!   render_to_svg (1-bar PAE, alloc per call)       209.0 µs
//!   render_to_svg_into (1-bar PAE, reuse buffer)    214.0 µs
//! svg/multi-page/
//!   render_all_pages_into (5 pages of importer.mei) 1.394 ms
//! parse/
//!   timemap (typed parse, 1-bar PAE)                  5.6 µs
//!   elements_at (typed parse, 1-bar PAE, t=0)         2.4 µs
//! lookup/
//!   sounding_at (cached lookup, 1-bar PAE, t=250ms)   26 ns  ← 85× faster
//!   sounding_at_into (cached lookup, reuse buf)       22 ns  ← 100× faster
//!   Toolkit::elements_at_time (FFI + JSON, t=250ms)  2.21 µs
//! ```
//!
//! Conclusions worth keeping in mind:
//!   - **Cache the timemap.** `lookup::sounding_at` over a cached `Vec` is
//!     ~100× faster than re-entering Verovio per playback tick. The 5.6 µs
//!     timemap parse is paid once per `load_data`.
//!   - **SVG render dominates** any path it's on (200 µs single page, ms
//!     for multi-page). Don't render every frame — only when the page
//!     visible to the user changes.
//!   - Buffer reuse for SVG is **neutral on tiny outputs** (the alloc-per-
//!     call cost is already negligible; clear+push barely amortizes). On
//!     larger SVGs and tighter loops the reuse variant pulls ahead — keep
//!     it as the recommended path.

use criterion::{criterion_group, criterion_main, Criterion};

use verovio::Toolkit;

const SAMPLE_PAE: &str = "\
@start:s
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:s
";

const SAMPLE_MEI: &str = include_str!("../../verovio-sys/vendor/verovio/doc/importer.mei");

fn bench_svg(c: &mut Criterion) {
    let mut group = c.benchmark_group("svg");

    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    group.bench_function("render_to_svg (1-bar PAE, alloc per call)", |b| {
        b.iter(|| {
            let _ = tk.render_to_svg(1).expect("render");
        })
    });

    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let mut buf = String::with_capacity(64 * 1024);
    group.bench_function("render_to_svg_into (1-bar PAE, reuse buffer)", |b| {
        b.iter(|| {
            tk.render_to_svg_into(1, &mut buf).expect("render");
        })
    });

    group.finish();
}

fn bench_multi_page_svg(c: &mut Criterion) {
    let mut group = c.benchmark_group("svg/multi-page");

    let mut tk = Toolkit::new();
    tk.set_options(r#"{"pageWidth": 800, "pageHeight": 400}"#)
        .unwrap();
    tk.load_data(SAMPLE_MEI).expect("MEI load");
    let pages = tk.page_count();

    let mut buf = String::with_capacity(64 * 1024);
    group.bench_function(
        format!("render_all_pages_into ({pages} pages of importer.mei, reuse)"),
        |b| {
            b.iter(|| {
                for page in 1..=pages {
                    tk.render_to_svg_into(page, &mut buf).expect("render");
                }
            })
        },
    );

    group.finish();
}

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");

    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let _ = tk.render_to_timemap(); // warm Verovio's layout cache

    group.bench_function("timemap (typed parse, 1-bar PAE)", |b| {
        b.iter(|| {
            let _ = tk.timemap().expect("timemap");
        })
    });

    group.bench_function("elements_at (typed parse, 1-bar PAE, t=0)", |b| {
        b.iter(|| {
            let _ = tk.elements_at(0).expect("elements_at");
        })
    });

    group.finish();
}

fn bench_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("lookup");

    // Build a cached timemap once.
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let timemap = tk.timemap().expect("timemap parse");

    // Cache-aware lookup: pure-Rust walk over the parsed timemap.
    group.bench_function("sounding_at (cached lookup, 1-bar PAE, t=250ms)", |b| {
        b.iter(|| {
            let _ = verovio::lookup::sounding_at(&timemap, 250.0);
        })
    });

    // Buffer-reuse variant — same work, no per-call Vec alloc.
    let mut buf = Vec::with_capacity(16);
    group.bench_function("sounding_at_into (cached lookup, reuse buf)", |b| {
        b.iter(|| {
            verovio::lookup::sounding_at_into(&timemap, 250.0, &mut buf);
        })
    });

    // The FFI + JSON path for the same answer. This is what the cache
    // replaces. Expect 10-100× slower on a tiny score; the gap widens
    // for larger scores because FFI cost is fixed per call.
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    group.bench_function("Toolkit::elements_at_time (FFI + JSON, t=250ms)", |b| {
        b.iter(|| {
            let _ = tk.elements_at_time(250);
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_svg,
    bench_multi_page_svg,
    bench_parse,
    bench_lookup
);
criterion_main!(benches);
