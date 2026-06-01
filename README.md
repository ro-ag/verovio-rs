# verovio-rs

[![CI](https://github.com/ro-ag/verovio-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/ro-ag/verovio-rs/actions/workflows/ci.yml)
[![License: LGPL-3.0-or-later](https://img.shields.io/badge/License-LGPL%20v3%2B-blue.svg)](LICENSE)
[![Verovio version](https://img.shields.io/badge/Verovio-6.2.1-informational)](https://github.com/rism-digital/verovio/releases/tag/version-6.2.1)

Safe Rust bindings to [Verovio](https://www.verovio.org/), RISM's C++
music notation engraving library. Loads MusicXML / MEI / Humdrum / ABC /
PAE; renders SVG / PNG / single- and multi-page PDF; produces SMF MIDI
with full multi-track playback control; synthesizes offline WAV via
SoundFont; exposes the timemap, tempo map, measure timeline, bbox map,
and score metadata for syncing UI to playback.

> **Status: 0.3.3, published on [crates.io](https://crates.io/crates/verovio).**
> Feature-complete for read+play workflows (rendering, MIDI policy,
> offline audio, score reading). MEI editing is the deliberate
> non-goal for the 0.x line; v1.0 will follow real-world consumer
> feedback.

## Crates

| Crate            | Description                                                   |
| ---------------- | ------------------------------------------------------------- |
| `verovio`        | Safe wrapper. The crate you depend on.                        |
| `verovio-sys`    | `cxx::bridge` plus the C++ build of vendored Verovio sources. |
| `verovio-data`   | Bundled SMuFL fonts + resource files (Bravura, Leipzig, …).   |

## Using verovio-rs in your project

```toml
# in your Cargo.toml
[dependencies]
verovio = "0.3"

# optional features
verovio = { version = "0.3", features = ["png", "pdf", "audio"] }
```

Building from source requires a C++20 toolchain and the Verovio git
submodule. If you clone the repo directly:

```sh
git clone --recurse-submodules \
    --branch v0.3.3 \
    https://github.com/ro-ag/verovio-rs.git
```

### NixOS consumers — libstdc++ at runtime

Binaries built by *your* crate against `verovio` need to find
`libstdc++.so.6` at runtime. NixOS doesn't have it at any FHS path, so
either run inside `nix-shell` (sets `LD_LIBRARY_PATH` for you) or
expose it explicitly:

```sh
LD_LIBRARY_PATH="$(dirname $(c++ -print-file-name=libstdc++.so.6))" \
    ./target/release/your-binary
```

The `verovio-sys` build script emits an rpath for its own
tests/benches, but Cargo's `rustc-link-arg` only propagates within the
emitting package — your crate's binaries aren't affected. On
Linux/macOS with FHS paths this is a non-issue (the standard library
search path resolves `libstdc++.so.6` directly).

### Why not `cargo add --git`?

Cargo's git-dependency resolver does **not** initialize submodules by
default (tracked in
[rust-lang/cargo#4247](https://github.com/rust-lang/cargo/issues/4247)).
A direct `verovio = { git = "..." }` will fetch the parent repo but
leave `crates/verovio-sys/vendor/verovio/` empty, and the build will
fail with a clear "Verovio submodule not initialized" error.

If you really want a git dep, you can:

1. Set `[net] git-fetch-with-cli = true` in your `.cargo/config.toml`,
   **and**
2. Set `git config --global fetch.recurseSubmodules true`.

The path-dep route is more reliable and is the supported workflow
until the crate is published to crates.io.

## Quick start

```rust
use verovio::Toolkit;

let mut tk = Toolkit::new();
tk.load_data(r#"
@start:demo
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:demo
"#)?;

for page in 1..=tk.page_count() {
    let svg = tk.render_to_svg(page)?;
    // … write svg to disk, or render in a UI
}

// Playhead-sync timemap, typed:
let timemap: Vec<verovio::TimemapEvent> = tk.timemap()?;

// Score header metadata extracted from the loaded MEI/MusicXML:
let metadata = tk.metadata()?;
println!("{:?} by {:?}", metadata.title, metadata.composer);

// Pixel-rect map for click-to-seek and highlight overlays:
let bboxes = tk.bbox_map()?;
# Ok::<(), verovio::Error>(())
```

Buffer-reuse variants (`render_to_svg_into(&mut String)`) and
streaming-shape writers (`render_to_svg_writer(&mut w)`) are available
on every allocating method.

## Feature matrix

All features are off by default.

### Render / export

| Feature | Adds                                  | Use when |
|---------|---------------------------------------|----------|
| `png`   | `dep:resvg`                           | Rasterize a page to PNG bytes |
| `pdf`   | `dep:svg2pdf`, `dep:pdf-writer`       | Single- or multi-page PDF assembly |

### Audio

| Feature       | Adds                          | Use when |
|---------------|-------------------------------|----------|
| `audio`       | `dep:rustysynth`              | Offline PCM/WAV from a SoundFont |
| `live-audio`  | `audio` + `dep:cpal`          | Drive the OS audio device (example only) |

### Sanitizers (C++ side)

| Feature           | Adds to the C++ build                                  |
|-------------------|--------------------------------------------------------|
| `sanitize`        | `-fsanitize=address,undefined -fno-omit-frame-pointer` |
| `sanitize-thread` | `-fsanitize=thread -fno-omit-frame-pointer`            |

Mutually exclusive — `build.rs` panics if both are enabled. See the
[Building wiki page](https://github.com/ro-ag/verovio-rs/wiki/Building#sanitizers)
for the `RUSTFLAGS` invocation needed on stable Rust.

```toml
verovio = { version = "0.3", features = ["png", "pdf", "audio"] }
```

## What this binding does

| Surface | API |
|---|---|
| Loading | `load_data`, `load_file` (UTF-16 / `.mxl` aware via upstream), `load_zip_data_buffer` (raw `.mxl` bytes) |
| SVG render | `render_to_svg`, `render_to_svg_into`, `render_to_svg_writer`, `render_svg_measure_range` |
| PNG render | `render_to_png`, `render_to_png_all_pages` (`png` feature) |
| PDF render | `render_to_pdf`, `render_to_pdf_all_pages` (`pdf` feature) |
| MIDI render | `render_to_midi_bytes` (primary), `render_to_midi_bytes_with_policy`, `render_to_midi_writer`, `render_to_midi` (base64) |
| MIDI policy | `MidiTrackPolicy` with channel / program / volume / pan / mute / sustain / transpose / expression / modulation / reverb / chorus / bank / port / track-name / instrument-name / time-sig / key-sig / tempo-curve / lyrics / cue points / measure markers |
| MIDI helpers | `iter_smf_events`, `summarize`, `build_panic_smf`, `gm::{program_name, drum_key_name, note_name, midi_key_from_name}` |
| Audio render | `render_to_wav`, `render_to_pcm`, `render_to_wav_with_policy` (`audio` feature) |
| Audio live | `examples/live_playback.rs` (`live-audio` feature, cpal-based) |
| Format conversion | `to_mei`, `to_mei_with_options(&MeiOptions)`, `render_to_pae`, `validate_pae` |
| Timemap | `timemap`, `timemap_exact`, `render_to_timemap`, `elements_at_time` (returns `Result`) |
| Element introspection | `page_with_element`, `time_for_element`, `times_for_element`, `midi_values_for_element`, `element_attr`, `notated_id_for_element`, `expansion_ids_for_element` |
| Tempo | `TempoMap` with `qstamp_to_ms`, `ms_to_qstamp`, `bpm_at_qstamp`, `bpm_at_ms`, `scaled` |
| Cursors | `PlaybackCursor` (monotonic, amortized O(1)), `LoopCursor` (`[start, end)` wrap) |
| Lookup | `sounding_at`, `chord_at`, `note_duration`, `events_in_range`, `next_event_after`, `prev_event_before`, `measure_by_id`, … |
| Score reading | `metadata`, `measures`, `staff_map`, **`bbox_map`** (click-to-seek + highlight rects), `classified_elements`, `expansion_map` |
| Options surface | `available_options` (schema), `reset_options`, `select(region_json)`, `set_layout_options(&LayoutOptions)`, `set_scale` / `scale`, `set_input_from`, `set_output_to`, `reset_xml_id_seed` |
| Layout setters | `set_font`, `set_zoom`, `set_page_size`, `set_breaks`, `set_landscape`, `option_value`, `redo_page_pitch_pos_layout` |
| Styling | `styling::stripe_tracks_by_id`, `styling::fade_others` (CSS class generators) |
| Diagnostics | `id`, `resource_path`, `version` |
| Log | `set_log_level` (mutex-gated) |

## Documentation

- **API reference**: `cargo doc --open` from the workspace root
- **Long-form docs**: [Wiki](https://github.com/ro-ag/verovio-rs/wiki)
  — Quick Start, Features, Rendering, MIDI Playback, Audio, Score
  Reading, Concurrency, Building
- **Examples**: `cargo run -p verovio --example <name>`
  - `render_to_file` — basic SVG-per-page
  - `playback_simulation` — timemap-driven highlight loop
  - `render_to_midi` — multi-track policy in action
  - `live_playback` — cpal + rustysynth (`live-audio` feature)

## Platforms

Linux (x86_64, aarch64), macOS (Apple Silicon, Intel), and Windows
(MSVC x86_64). All three are tested in CI.

## Build requirements

A working C++20 toolchain (clang 14+ or gcc 11+) and Rust 1.85+ stable.
The Verovio C++ source is vendored as a git submodule and built via
`cc::Build` — no `cmake`, no system `verovio` required.

```sh
git clone --recurse-submodules https://github.com/ro-ag/verovio-rs
cd verovio-rs
cargo test
```

First clean build takes ~6 minutes (295 C++ files in `-O0` + debug
info); subsequent incremental builds are seconds. See the [Building
wiki page](https://github.com/ro-ag/verovio-rs/wiki/Building) for
NixOS, sanitizers, and troubleshooting.

### NixOS

`shell.nix` provides the toolchain plus `sccache` and `mold`:

```sh
nix-shell
cargo test
```

For the `live-audio` example, also pull `alsa-lib` and `pkg-config`
(see [Audio wiki page](https://github.com/ro-ag/verovio-rs/wiki/Audio)).

## Thread safety

`Toolkit: Send + !Sync`. Verovio's render and layout methods mutate
internal state even when shaped as `const`; sharing a `&Toolkit`
between threads would be unsound. For concurrent rendering: one
`Toolkit` per thread, or a single worker thread fronted by a channel.

The crate deliberately omits a few upstream surfaces that touch
process-global state — Humdrum methods, `SetLocale`, the unmutexed log
toggle — because those would break the `Send` guarantee. See the
[Concurrency wiki page](https://github.com/ro-ag/verovio-rs/wiki/Concurrency)
for the full TSan audit (8 races, all upstream, none observable in our
tests).

## License

`verovio-rs` is licensed under **LGPL-3.0-or-later**, matching the
upstream Verovio library. The vendored Verovio source tree is a mix of
LGPL-3.0 (Verovio itself) and permissive licenses for individual
dependencies (pugixml MIT, jsonxx MIT, humlib BSD-2-Clause, midifile
BSD-2-Clause, miniz-cpp MIT, crc public domain). All are compatible
with LGPL-3.0 downstream.

## Acknowledgements

- [Verovio](https://www.verovio.org) — the engraving engine this crate
  wraps. Developed by RISM Digital Center.
- [`verovioxide`](https://github.com/oxur/verovioxide) — independent
  prior-art Rust binding; `verovio-rs/build.rs` borrows its
  tarball-pinning patterns.
- [`cxx`](https://github.com/dtolnay/cxx) — the Rust ↔ C++ binding
  generator this crate is built on.
- [`rustysynth`](https://github.com/sinshu/rustysynth) — pure-Rust
  SoundFont synthesizer used behind the `audio` feature.
