# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).
Until 1.0, every minor bump (0.x → 0.y) may carry breaking API changes.

## [Unreleased]

The 0.1.0 work-in-progress. Tracks `main`; not yet published to crates.io.

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
  active element IDs over time (the playhead-sync shape `xpart`
  consumes).
- `README.md`, `LICENSE` (LGPL-3.0-or-later), `NOTICE` (Verovio +
  vendored deps + SMuFL fonts attribution).

### Verovio surface deliberately NOT exposed

Per the safety contract, the following Verovio APIs are omitted because
they touch process-global state we'd have to mutex-gate without a clear
caller story: `GetHumdrum*` (`Toolkit::m_humdrumBuffer` is a `static`
`char*`), `GetLog` / `GetLogString` (namespace-level log buffer),
`SetLocale` (`std::locale::global`). Adding any of these requires
updating the safety-contract memory first.
