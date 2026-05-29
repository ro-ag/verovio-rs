# verovio-rs

Rust bindings to [Verovio](https://www.verovio.org/), RISM's C++ music
notation engraving library. Loads MusicXML / MEI / Humdrum / ABC / PAE,
produces SVG, and exposes the timemap Verovio uses to sync playback to
notation.

> **Status: pre-1.0, pre-publish.** API surface is small and focused on
> what the parent project [`xpart`](https://github.com/ro-ag/xpart)
> needs. Methods are added when they're consumed, not preemptively. Not
> yet published to crates.io.

## Crates

| Crate            | Description                                                   |
| ---------------- | ------------------------------------------------------------- |
| `verovio`        | Safe wrapper. The crate you depend on.                        |
| `verovio-sys`    | `cxx::bridge` plus the C++ build of vendored Verovio sources. |
| `verovio-data`   | Bundled SMuFL fonts + resource files (Bravura, Leipzig, …).   |

## Quick start

```rust
use verovio::Toolkit;

let mut tk = Toolkit::new();
tk.load_data(r#"
@start:clefs
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:clefs
"#)?;

for page in 1..=tk.page_count() {
    let svg = tk.render_to_svg(page)?;
    // … write svg to disk, or render in a UI
}

// Playhead-sync timemap (JSON):
let timemap = tk.render_to_timemap()?;
```

Buffer-reuse variants (`render_to_svg_into(&mut String)` etc.) are
available on every allocating method for tight render loops.

## Platforms

Linux and macOS. **Windows is intentionally out of scope** and will not
be accepted; both target platforms are POSIX, which keeps `build.rs`, CI,
and the FFI surface much simpler.

## Build requirements

A working C++20 toolchain (clang or gcc 11+) and a Rust 1.85+ stable
toolchain. The Verovio C++ source is vendored as a git submodule and
built from source via `cc::Build` — no `cmake`, no system `verovio`
required.

```sh
git clone --recurse-submodules https://github.com/ro-ag/verovio-rs
cd verovio-rs
cargo test
```

First clean build takes ~6 minutes (295 C++ files in `-O0` + debug
info); subsequent incremental builds are seconds.

### NixOS

`shell.nix` provides the toolchain plus `sccache` (compiler cache) and
`mold` (fast linker). Enter once with `nix-shell`; a clean rebuild after
`cargo clean` then completes in under a minute on a warm sccache.

```sh
nix-shell
cargo test
```

## Thread safety

`Toolkit: Send + !Sync`. Verovio's render and layout methods mutate
internal state even when shaped as `const`; sharing a `&Toolkit` between
threads would be unsound. For concurrent rendering, construct one
`Toolkit` per thread or use a single worker thread fronted by a channel.

The crate deliberately omits a few upstream surfaces that touch
process-global state — Humdrum methods, the log toggle, `SetLocale` —
because those would break the `Send` guarantee. Add them only with
matching mutex/serialization, never as bare bindings.

## License

`verovio-rs` is licensed under **LGPL-3.0-or-later**, matching the
upstream Verovio library. The vendored Verovio source tree is a mix of
LGPL-3.0 (Verovio itself) and permissive licenses for individual
dependencies (pugixml MIT, jsonxx MIT, humlib BSD-2-Clause, midifile
BSD-2-Clause, miniz-cpp MIT, crc public domain). All are compatible with
LGPL-3.0 downstream.

## Acknowledgements

- [Verovio](https://www.verovio.org) — the engraving engine this crate
  wraps. Developed by RISM Digital Center.
- [`verovioxide`](https://github.com/oxur/verovioxide) — independent
  prior-art Rust binding; `verovio-rs/build.rs` borrows its
  tarball-pinning patterns.
- [`cxx`](https://github.com/dtolnay/cxx) — the Rust ↔ C++ binding
  generator this crate is built on.
