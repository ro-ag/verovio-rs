# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).
Until 1.0, every minor bump (0.x → 0.y) may carry breaking API changes.

## [Unreleased]

## [0.2.0] — 2026-05-30

First public release. Repository made public on 2026-05-30 after the
feature-complete read+play surface landed and the safety contract was
TSan-audited.

### Added — MIDI surface saturation (`verovio::midi`)

- **`TrackOverride`** new fields: `bank_select` (CC#0 + CC#32),
  `midi_port` (Meta::MidiPort), `transpose`, `expression` (CC#11),
  `modulation` (CC#1), `sustain` (CC#64), `reverb` (CC#91),
  `chorus` (CC#93), `pan` (CC#10), `name` (Meta::TrackName),
  `instrument_name` (Meta::InstrumentName).
- **`MidiTrackPolicy`** new fields: `tempo_override` (replace Verovio's
  curve from a `TempoMap`), `time_signature`, `key_signature` (+ minor
  bit), `measure_markers` (auto-insert Meta::Marker per measure),
  `lyrics` (Meta::Lyric at quarter-stamps), `cue_points` (Meta::CuePoint).
- **Builders**: `MidiTrackPolicy::with_solo`, `with_mute`,
  `with_programs`, `with_auto_distribute_channels` for common policies
  without hand-building the `BTreeMap`.
- **`iter_smf_events`** — chronologically-sorted `Vec<TimedEvent>`
  with wall-clock millisecond onsets, derived from the SMF's own
  tempo curve. `TimedMessage` covers note-on/off, aftertouch,
  controllers, program changes, pitch bend, tempo, opaque meta, sysex.
- **`summarize`** — `Vec<TrackInfo>` inspector reporting channels,
  audible-note counts, program/volume/pan, total tick length per track.
- **`build_panic_smf`** — single-track SMF that sends CC#120 (All Sound
  Off) + CC#123 (All Notes Off) on all 16 channels for emergency stop.
- **`gm` module** — General MIDI lookup tables:
  - `program_name(0..127)` / `all_programs()` / `drum_key_name(35..81)`
  - `note_name` (scientific pitch notation, 0..127 → "C-1".."G9")
  - `midi_key_from_name` (parser accepting `#` or `b`)

### Added — Cache-aware lookup helpers (`verovio::lookup`)

- `chord_at(timemap, ms)` — IDs sharing the latest onset.
- `note_duration(timemap, id)` — `(on_ms, off_ms)` lifespan.
- `measure_by_id(measures, id)` — `MeasureInfo` by MEI ID.
- `sounding_count_at`, `distinct_element_count` — cheap counts.
- `LoopCursor` — `PlaybackCursor` variant bounded by `[start, end)`
  with auto-wrap, for practice-loop UI.

### Added — Read-side closures (`Toolkit`)

- **`bbox_map() -> HashMap<String, BBox>`** — walks each rendered page's
  SVG with a `translate` transform stack; aggregates per-element bbox
  from `<use>` glyph anchors and `<path>` M/L coordinates. Enables
  click-to-seek hit testing and highlight-overlay rectangles.
  - New `BBox { x, y, width, height, page }` type with `contains(x, y)`.
- **`metadata() -> ScoreMetadata`** — parses title / composer /
  lyricist / arranger / copyright / instruments from cached input.
  Supports MEI, MusicXML (DTD allowed), and PAE / ABC plaintext.
  Verovio's C++ Toolkit doesn't expose these — they're parsed from a
  cached `load_data` input retained on the `Toolkit` struct.

### Added — Ergonomic `Toolkit` API

- Streaming-shape writers: `render_to_svg_writer<W: Write>`,
  `render_to_midi_writer`, `render_to_timemap_writer`.
- Layout setters: `set_font`, `set_zoom`, `set_page_size`,
  `set_breaks`, `set_landscape`.
- `option_value(name) -> Option<serde_json::Value>` — read one option
  without parsing the whole JSON.
- `render_svg_measure_range(from, to, joiner)` — Verovio's
  `measureFrom`/`measureTo` with auto-restore.
- `is_loaded()`, `measures()`, `measure_at(ms)`, `tempo_map()`,
  `staff_map()`, `classified_elements()`, `expansion_map()`,
  `set_midi_options(&MidiOptions)`, `set_svg_options(&SvgOptions)`.

### Added — Audio (`audio` feature)

- New `audio` cargo feature pulling `rustysynth` (pure-Rust SoundFont 2
  synthesizer; the crate does NOT bundle an SF2).
- `verovio::audio::render_pcm` / `render_wav` / `pcm_to_wav` —
  free-function offline synthesis.
- `Pcm { sample_rate, left, right }` with `duration_secs` +
  `interleaved`.
- `Toolkit::render_to_pcm`, `render_to_wav`, `render_to_wav_with_policy`.
- WAV output: 16-bit signed PCM stereo RIFF (universal compatibility).
- New `Error::Audio` variant wrapping rustysynth init / SF2 errors.

### Added — Multi-page PDF (`pdf` feature, extended)

- `Toolkit::render_to_pdf_all_pages()` — single PDF document, every
  page sized to its rendered SVG.
- `verovio::raster::svgs_to_pdf(&[String])` — pure-function form.
- Implementation: `svg2pdf::to_chunk` per page → renumbered into a
  unified ref allocator → `pdf-writer` assembly.

### Added — Live audio (`live-audio` feature)

- `examples/live_playback.rs` — working cpal-based demo (~120 LoC).
- `live-audio` cargo feature gates the cpal dependency so default
  `cargo test` stays portable on environments without `alsa-lib`
  (NixOS, CI without the dev shell).

### Added — Styling helpers (`verovio::styling`)

- `stripe_tracks_by_id(staff_map, palette)` — per-track CSS coloring.
- `fade_others(keep_ids, fade_color)` — solo-track visualization.

### Added — Data + types

- `verovio_data::AVAILABLE_FONTS` constant for font-picker UIs.
- New typed wrappers: `MidiOptions`, `SvgOptions`, `MeasureInfo`,
  `TempoMap`, `TempoChange`, `ClassifiedElements`, `ElementKind`,
  `ExpansionMap`, `TimemapEventExact`.
- `TempoMap` methods: `qstamp_to_ms`, `ms_to_qstamp`, `bpm_at_qstamp`,
  `bpm_at_ms`, `scaled(factor)` (practice-speed without mutation).

### Added — Documentation

- GitHub Wiki (9 pages) — Home, Quick-Start, Features, Rendering,
  MIDI-Playback, Audio, Score-Reading, Concurrency, Building.
- README expanded with cargo feature matrix and capability table.
- All public items documented with rustdoc; intra-doc links resolve clean.

### Tests + concurrency

- **222 tests** with `png + pdf + audio` features, including 8
  concurrency tests covering toolkit `Send` invariants, multi-thread
  policy application, lookup-helper safety on shared timemaps, and
  PNG render across threads.
- TSan audit performed: 8 races detected, all in Verovio's upstream
  C++ code, none with observable runtime impact. Documented in the
  Concurrency wiki page.

### Changed

- The `verovio` crate `examples/` directory expanded: `render_to_file`,
  `playback_simulation`, `render_to_midi`, `styled_render`,
  `multi_track_playback`, `live_playback` (`live-audio` feature).

### Process

- Repository made public on 2026-05-30.
- Wiki repository initialized at
  `github.com/ro-ag/verovio-rs/wiki` with the same 9 pages.

## [0.1.0] — 2026-05-29

Initial workspace and surface (private development phase).

### Added — initial public surface

**Workspace layout (3 crates):**
- `verovio-sys` — `cxx::bridge` against Verovio's C++ `Toolkit` class, plus
  the `cc::Build`-driven static compile of Verovio's vendored source tree
  (~295 `.cpp` files, C++20).
- `verovio` — safe wrapper with no `unsafe` in the public API.
- `verovio-data` — Bravura + Leipzig + optional SMuFL fonts bundled via
  `include_dir!()`, extracted to a process-lifetime tempdir at first
  `Toolkit::new`.

**Verovio pinning:** vendored as a git submodule at release `version-6.2.1`.

**`Toolkit` API (everything below takes `&mut self` because every render
path is non-`const` upstream):**

- Construction: `Toolkit::new`, `Toolkit::default`, `Toolkit::from_data`,
  `Toolkit::from_file`.
- Loading: `load_data`, `load_file`.
- Queries: `version`, `page_count`, `options`, `default_options`.
- Mutation: `set_options`, `redo_layout`, `redo_layout_with_options`.
- Rendering (all guarded against the upstream empty-doc `assert`):
  `render_to_svg` + `_into`, `render_to_midi` + `_into`, `render_to_midi_bytes`
  (decoded SMF), `render_to_timemap` + `_into`, `elements_at_time` + `_into`.
- Typed JSON access: `timemap() -> Vec<TimemapEvent>`,
  `elements_at(ms) -> ElementsAtTime`. Both derive `Serialize` + `Deserialize`.

**Process-global state (mutex-gated):**
- `set_log_level(LogLevel)` — silences Verovio's stdout chatter
  (`LogLevel::{Off, Error, Warning, Info, Debug}`).

**Error type:** `Error::{LoadFailed, OptionsRejected, RenderFailed{page},
Io(io::Error), Json(serde_json::Error), Base64(base64::DecodeError)}` —
`std::error::Error::source` chains through the wrapped errors.

**Trait impls:** `Toolkit: Send + Debug` (NOT `Sync`, NOT `Clone`).
Compile-time and runtime `Send` assertions in `tests/api.rs`.

### Build & infrastructure

- `cc::Build` directly compiles Verovio's source tree into a static
  archive (`libverovio.a`) embedded in the rlib — no shared library at
  runtime, no `cmake` dependency.
- `git_commit.h` synthesized into `OUT_DIR` since cmake's
  `tools/get_git_commit.sh` isn't invoked.
- NixOS handling: `c++ -print-file-name=libstdc++.so.6` discovery emits
  the right `rpath` so tests run outside `nix-shell` without
  `LD_LIBRARY_PATH`; `nix-shell` itself pins glibc via `stdenv.cc.libc`.
- Cargo features: `sanitize` (ASan + UBSan), `sanitize-thread` (TSan);
  mutually exclusive. Stable Rust requires `-C linker=clang` because
  `rust-lld` doesn't translate `-fsanitize=` into the runtime link
  (documented in `README.md`).
- `shell.nix` provides `sccache` + `mold` for local dev; CI ditches
  sccache (GitHub Actions cache backend is too flaky to gate the
  build on) but keeps mold on Linux.

### CI

- `.github/workflows/ci.yml` with 4 jobs:
  - `test (ubuntu-latest)` — fmt/clippy/build/test, mold linker.
  - `test (macos-latest)` — fmt/clippy/build/test.
  - `test (nix-shell)` — full pipeline inside `shell.nix` for dev-env
    fidelity.
  - `sanitize (ubuntu)` — `cargo test --features verovio/sanitize` with
    `clang` as linker for ASan runtime resolution.
- Swatinem/rust-cache for cargo registry + fingerprints.

### Tests, benches, examples, docs

- 44 unit + integration tests across `tests/{api, render, multi_page,
  typed, version}.rs`, plus 1 module-level doctest in `verovio/src/lib.rs`.
- Multi-page coverage uses Verovio's own `doc/importer.mei` at a forced
  narrow page width (`pageWidth: 800, pageHeight: 400`).
- `benches/render.rs` — criterion suite for SVG render alloc-vs-reuse,
  multi-page render, and typed parse paths.
- `examples/render_to_file.rs` — load PAE, render every page to disk
  using the buffer-reuse pattern.
- `examples/playback_simulation.rs` — walk a typed timemap and report
  active element IDs over time.
- `README.md`, `LICENSE` (LGPL-3.0-or-later), `NOTICE` (Verovio +
  vendored deps + SMuFL fonts attribution).

### Verovio surface deliberately NOT exposed

Per the safety contract, the following Verovio APIs are omitted because
they touch process-global state we'd have to mutex-gate without a clear
caller story: `GetHumdrum*` (`Toolkit::m_humdrumBuffer` is a `static`
`char*`), `GetLog` / `GetLogString` (namespace-level log buffer),
`SetLocale` (`std::locale::global`). Adding any of these requires
updating the safety-contract memory first.
