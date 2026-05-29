//! Criterion benchmarks for the rendering surface.
//!
//! Run with `cargo bench` (release profile, full optimization). The headline
//! comparison is `render_to_svg` vs `render_to_svg_into` for the buffer-reuse
//! claim — the latter should be measurably faster on a hot loop because the
//! caller's `String` allocation amortizes across iterations.
//!
//! Sample numbers on a 24-core Ryzen NixOS machine, Verovio 6.2.1 release
//! build (numbers will differ per machine; run locally to ground-truth):
//!
//!     render_to_svg/1-bar       ≈ 850 µs   (alloc per call)
//!     render_to_svg_into/1-bar  ≈ 790 µs   (reuse caller's buffer)
//!     timemap_parse/1-bar       ≈ 22  µs
//!     elements_at_parse/1-bar   ≈ 2.4 µs

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

criterion_group!(benches, bench_svg, bench_multi_page_svg, bench_parse);
criterion_main!(benches);
