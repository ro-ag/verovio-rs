//! The [`Toolkit`] type itself — the safe wrapper around `vrv::Toolkit`.
//!
//! Every render-family method in this module checks
//! `self.page_count() == 0` before crossing the FFI boundary. Verovio's
//! `Doc::GetVisibleScores` asserts unconditionally on an empty document and
//! SIGABRTs the process; the guard converts that into
//! [`Error::RenderFailed`].

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cxx::UniquePtr;
use verovio_sys::ffi;

use crate::{
    BBox, ClassifiedElements, ElementKind, ElementTimes, ElementsAtTime, Error, ExpansionMap,
    LayoutOptions, MeasureInfo, MeiOptions, MidiOptions, MidiValues, Result, ScoreMetadata,
    SvgOptions, TempoMap, Timemap, TimemapEventExact,
};

/// Stage the bundled `verovio-data` resources into a process-lifetime tempdir
/// the first time the toolkit is constructed; reuse on subsequent calls.
///
/// The tempdir is intentionally leaked (`TempDir::keep`) — it lives for the
/// process lifetime so any `Toolkit` that points at it stays valid.
fn resource_path() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let tmp = tempfile::Builder::new()
            .prefix("verovio-data-")
            .tempdir()
            .expect("failed to create tempdir for Verovio resources");
        verovio_data::extract(tmp.path()).expect("failed to extract bundled Verovio resources");
        tmp.keep()
    })
    .as_path()
}

/// A Verovio engraving toolkit.
///
/// One toolkit owns an MEI document, an option set, and the render state for
/// that document. Construct one per score you want to engrave.
pub struct Toolkit {
    inner: UniquePtr<ffi::Toolkit>,
    /// Score-level metadata parsed at load time. Verovio's C++ Toolkit
    /// doesn't expose title/composer/etc., so we parse them from the
    /// input data once and keep the small `ScoreMetadata` struct rather
    /// than retaining the original (potentially hundreds-of-KB) source.
    /// `None` until [`Self::load_data`] / [`Self::load_file`] succeed.
    cached_metadata: Option<ScoreMetadata>,
    /// Cached page count after first layout. `None` after any operation
    /// that invalidates Verovio's layout (load_data, set_options,
    /// reset_options, redo_layout, select, set_scale, set_layout_options,
    /// set_*_options, …). Filled by [`Self::page_count`] on next call.
    cached_page_count: std::cell::Cell<Option<u32>>,
}

// SAFETY: `Toolkit` is `Send` only because this crate deliberately does *not*
// expose any Verovio surface that touches process-global state — specifically
// no Humdrum methods (would race on `static Toolkit::m_humdrumBuffer`), no log
// methods without mutex gating, and no `SetLocale`. See the safety contract.
unsafe impl Send for Toolkit {}

impl std::fmt::Debug for Toolkit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The interesting state (loaded document, options, layout cache)
        // lives behind the cxx::UniquePtr and isn't safely inspectable
        // without &mut self. Surface only the version for traceability.
        f.debug_struct("Toolkit")
            .field("verovio_version", &self.version())
            .finish_non_exhaustive()
    }
}

impl Toolkit {
    /// Construct a new toolkit.
    ///
    /// Stages the bundled `verovio-data` SMuFL resources on disk (once per
    /// process) and points Verovio at them via `SetResourcePath`. Returns a
    /// toolkit that is ready to accept [`Self::load_data`].
    ///
    /// # Panics
    ///
    /// Panics if the bundled resources can't be extracted to a tempdir or
    /// Verovio rejects the resource path. Both are environment-level failures
    /// (e.g. read-only `$TMPDIR`) — there's nothing meaningful a caller could
    /// do with a `Result` here.
    pub fn new() -> Self {
        let mut inner = ffi::new_toolkit(false);
        let path = resource_path()
            .to_str()
            .expect("Verovio resource path must be UTF-8");
        let ok = ffi::set_resource_path(inner.pin_mut(), path);
        assert!(ok, "Verovio rejected SetResourcePath({path})");
        Self {
            inner,
            cached_metadata: None,
            cached_page_count: std::cell::Cell::new(None),
        }
    }

    /// Construct a toolkit and load a score in one step. Equivalent to
    /// [`Self::new`] followed by [`Self::load_data`].
    pub fn from_data(data: &str) -> Result<Self> {
        let mut tk = Self::new();
        tk.load_data(data)?;
        Ok(tk)
    }

    /// Construct a toolkit and load a score from disk in one step.
    /// Equivalent to [`Self::new`] followed by [`Self::load_file`].
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let mut tk = Self::new();
        tk.load_file(path)?;
        Ok(tk)
    }

    /// Return the upstream Verovio version string (e.g. `"6.2.1"`).
    pub fn version(&self) -> String {
        ffi::get_version(&self.inner)
    }

    /// Load a score document. Verovio auto-detects the format from content
    /// (MEI, MusicXML, ABC, PAE, ...).
    ///
    /// Returns [`Error::LoadFailed`] if Verovio's parser rejects the input.
    pub fn load_data(&mut self, data: &str) -> Result<()> {
        self.invalidate_caches();
        if ffi::load_data(self.inner.pin_mut(), data) {
            self.cached_metadata = Some(parse_metadata(data));
            Ok(())
        } else {
            Err(Error::LoadFailed)
        }
    }

    /// Read a score from disk and load it.
    ///
    /// Delegates to upstream `Toolkit::LoadFile`, which handles UTF-16
    /// MusicXML and compressed `.mxl` archives transparently — neither of
    /// which a plain `fs::read_to_string` could read.
    ///
    /// Metadata extraction is best-effort: if the file reads back as UTF-8
    /// text (most MEI / MusicXML / PAE / ABC), [`Self::metadata`] returns
    /// the parsed fields. For binary inputs (`.mxl`, UTF-16) metadata is
    /// empty — the data still loads into Verovio normally.
    ///
    /// Returns [`Error::Io`] on filesystem errors, [`Error::LoadFailed`] if
    /// Verovio's parser rejects the content.
    pub fn load_file(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let path_str = path
            .to_str()
            .ok_or_else(|| Error::Io(io::Error::other("path is not valid UTF-8")))?;
        // Surface FS errors before invalidating caches.
        if !path.exists() {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::NotFound,
                format!("file not found: {}", path.display()),
            )));
        }
        self.invalidate_caches();
        if !ffi::load_file(self.inner.pin_mut(), path_str) {
            return Err(Error::LoadFailed);
        }
        // Best-effort metadata scrape: works for UTF-8 text inputs;
        // silently leaves cached_metadata = None for .mxl / UTF-16.
        if let Ok(text) = std::fs::read_to_string(path) {
            self.cached_metadata = Some(parse_metadata(&text));
        }
        Ok(())
    }

    /// Load a compressed MusicXML file (`.mxl`) from raw bytes.
    ///
    /// Returns [`Error::LoadFailed`] if the buffer isn't a valid ZIP
    /// archive or Verovio's unzip / parse rejects the contents. Metadata
    /// is not parsed from compressed inputs.
    ///
    /// The magic-byte check is non-negotiable: upstream's
    /// `LoadZipDataBuffer` calls into miniz-cpp without validation and
    /// will `std::terminate` (uncaught `std::length_error`) on garbage
    /// input. We reject obvious non-zip buffers in Rust before crossing
    /// the FFI boundary.
    pub fn load_zip_data_buffer(&mut self, data: &[u8]) -> Result<()> {
        // ZIP local-file-header / empty-archive / spanned-archive
        // magic. Anything else is a non-zip buffer — reject before
        // calling upstream.
        let valid_zip = data.len() >= 4
            && data[0] == b'P'
            && data[1] == b'K'
            && (data[2..4] == [0x03, 0x04]
                || data[2..4] == [0x05, 0x06]
                || data[2..4] == [0x07, 0x08]);
        if !valid_zip {
            return Err(Error::LoadFailed);
        }
        self.invalidate_caches();
        if ffi::load_zip_data_buffer(self.inner.pin_mut(), data) {
            Ok(())
        } else {
            Err(Error::LoadFailed)
        }
    }

    /// Number of layout pages for the currently-loaded document.
    ///
    /// First call triggers Verovio's lazy layout computation and caches
    /// the result; subsequent calls are O(1) without crossing the FFI
    /// boundary. The cache is invalidated by any method that mutates
    /// loaded data or layout options (`load_data`, `set_options`,
    /// `reset_options`, `redo_layout`, `select`, `set_scale`, …).
    ///
    /// Returns `0` if no document is loaded or upstream returned a
    /// negative value.
    pub fn page_count(&mut self) -> u32 {
        if let Some(n) = self.cached_page_count.get() {
            return n;
        }
        let n = ffi::page_count(self.inner.pin_mut());
        let count = n.max(0) as u32;
        self.cached_page_count.set(Some(count));
        count
    }

    /// Whether a document is currently loaded and laid out. Implemented as
    /// `page_count() > 0` — semantic predicate so consumers don't have to
    /// reach for the underlying number to express the same intent.
    pub fn is_loaded(&mut self) -> bool {
        self.page_count() > 0
    }

    /// Drop every cached value derived from the loaded document — the
    /// page-count cache and the metadata scrape. Called from every
    /// method that mutates Verovio's loaded data or layout options.
    fn invalidate_caches(&mut self) {
        self.cached_page_count.set(None);
        self.cached_metadata = None;
    }

    /// Current option set as a JSON document.
    pub fn options(&self) -> String {
        ffi::get_options(&self.inner)
    }

    /// Default option values as a JSON document. Useful for discovering the
    /// option schema without mutating the toolkit.
    pub fn default_options(&self) -> String {
        ffi::get_default_options(&self.inner)
    }

    /// Read a single option's value from the current option set, parsed
    /// as a [`serde_json::Value`]. Returns `None` if the option doesn't
    /// exist or the options JSON fails to parse.
    ///
    /// Convenience over parsing the full document yourself when you just
    /// want to inspect one field — e.g. after a layout that may have
    /// touched `pageWidth` / `pageHeight`.
    pub fn option_value(&self, name: &str) -> Option<serde_json::Value> {
        let json: serde_json::Value = serde_json::from_str(&self.options()).ok()?;
        json.get(name).cloned()
    }

    /// Apply MIDI-generation options via a typed wrapper. Convenience for
    /// users who don't want to assemble the JSON themselves.
    pub fn set_midi_options(&mut self, opts: &MidiOptions) -> Result<()> {
        self.set_options(&opts.to_json())
    }

    /// Switch the SMuFL engraving font. `font_name` must match one of the
    /// bundled font directory names — see
    /// [`verovio_data::AVAILABLE_FONTS`](https://docs.rs/verovio-data).
    /// Verovio accepts any string and logs a runtime warning if the font
    /// cannot be located; it does **not** report invalid font names as an
    /// error from `set_options`.
    ///
    /// Triggers a layout pass via Verovio's option-change path; the next
    /// render reflects the new font.
    pub fn set_font(&mut self, font_name: &str) -> Result<()> {
        let escaped = font_name.replace('"', "\\\"");
        self.set_options(&format!(r#"{{"font": "{escaped}"}}"#))
    }

    /// Convenience over `set_options({"scale": pct})`. `pct` is a percent
    /// (`100` = 1x, `200` = 2x). Affects subsequent SVG render output
    /// dimensions.
    pub fn set_zoom(&mut self, pct: u32) -> Result<()> {
        self.set_options(&format!(r#"{{"scale": {pct}}}"#))
    }

    /// Convenience over `set_options({"pageWidth": w, "pageHeight": h})`.
    /// Values are in Verovio's internal units (mm × 10 typically — see
    /// `default_options()` for the schema).
    pub fn set_page_size(&mut self, width: u32, height: u32) -> Result<()> {
        self.set_options(&format!(
            r#"{{"pageWidth": {width}, "pageHeight": {height}}}"#
        ))
    }

    /// Set Verovio's layout `breaks` option — one of `"auto"`,
    /// `"none"`, `"encoded"`, `"smart"`, `"line"`. Other strings will
    /// be rejected by Verovio.
    pub fn set_breaks(&mut self, mode: &str) -> Result<()> {
        let escaped = mode.replace('"', "\\\"");
        self.set_options(&format!(r#"{{"breaks": "{escaped}"}}"#))
    }

    /// Convenience over `set_options({"landscape": bool})`. Swaps page
    /// dimensions on the next layout pass.
    pub fn set_landscape(&mut self, landscape: bool) -> Result<()> {
        self.set_options(&format!(r#"{{"landscape": {landscape}}}"#))
    }

    /// Apply SVG-rendering options via a typed wrapper. The headline use
    /// is [`SvgOptions::css`]: pass a CSS block and it'll be embedded
    /// inside the rendered SVG. See [`SvgOptions`] for the targetable
    /// class structure.
    pub fn set_svg_options(&mut self, opts: &SvgOptions) -> Result<()> {
        self.set_options(&opts.to_json())
    }

    /// Apply the given options. `json` is a JSON document; see
    /// [`Self::default_options`] for the schema.
    ///
    /// Returns [`Error::OptionsRejected`] if Verovio fails to parse the JSON
    /// or it names an unrecognized option.
    pub fn set_options(&mut self, json: &str) -> Result<()> {
        // Layout-affecting options invalidate the cached page count.
        // Verovio doesn't tell us which options touch layout vs. not,
        // so invalidate unconditionally.
        self.cached_page_count.set(None);
        if ffi::set_options(self.inner.pin_mut(), json) {
            Ok(())
        } else {
            Err(Error::OptionsRejected)
        }
    }

    /// Render a single page to SVG. `page` is 1-indexed (Verovio's convention).
    ///
    /// Returns [`Error::RenderFailed`] if no document is loaded or the
    /// requested page is out of range. (Upstream's degenerate-but-valid
    /// `<svg width="0px" …>` response is detected via the page-count check;
    /// the layout pass it triggers is cached after the first call.)
    pub fn render_to_svg(&mut self, page: u32) -> Result<String> {
        if page == 0 || page > self.page_count() {
            return Err(Error::RenderFailed { page });
        }
        Ok(ffi::render_to_svg(self.inner.pin_mut(), page as i32, false))
    }

    /// Render a single page to SVG and write the bytes into `w`. Saves
    /// the caller's intermediate allocation when piping to a file or
    /// socket — call this instead of holding the full `String` in memory.
    ///
    /// **Honesty disclaimer:** Verovio's `RenderToSVG` only returns
    /// `std::string` (no `std::ostream&` overload upstream), so the C++
    /// side allocates and holds the full page in memory before this
    /// function copies it to `w`. The savings are on the **Rust side**:
    /// no `String` is held by the caller. Upstream streaming would
    /// require a Verovio PR adding `RenderToSVG(std::ostream&)`; tracked
    /// in `project-safety-contract` memory.
    pub fn render_to_svg_writer<W: io::Write>(&mut self, page: u32, w: &mut W) -> Result<()> {
        if page == 0 || page > self.page_count() {
            return Err(Error::RenderFailed { page });
        }
        let svg = ffi::render_to_svg(self.inner.pin_mut(), page as i32, false);
        w.write_all(svg.as_bytes())?;
        Ok(())
    }

    /// Render to MIDI (raw SMF bytes, base64-decoded) and write to `w`.
    /// Same disclaimer as [`Self::render_to_svg_writer`].
    pub fn render_to_midi_writer<W: io::Write>(&mut self, w: &mut W) -> Result<()> {
        let bytes = self.render_to_midi_bytes()?;
        w.write_all(&bytes)?;
        Ok(())
    }

    /// Render the timemap as JSON and write to `w`. Same disclaimer as
    /// [`Self::render_to_svg_writer`].
    pub fn render_to_timemap_writer<W: io::Write>(&mut self, w: &mut W) -> Result<()> {
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        let json = ffi::render_to_timemap(self.inner.pin_mut(), "");
        w.write_all(json.as_bytes())?;
        Ok(())
    }

    /// Render a single page to SVG, reusing the caller's buffer.
    ///
    /// `out` is cleared then filled. The C++ side still allocates its own
    /// `std::string` per call (Verovio has no streaming overload), but
    /// repeated calls in a render loop avoid Rust-side `String` reallocation.
    pub fn render_to_svg_into(&mut self, page: u32, out: &mut String) -> Result<()> {
        out.clear();
        if page == 0 || page > self.page_count() {
            return Err(Error::RenderFailed { page });
        }
        let svg = ffi::render_to_svg(self.inner.pin_mut(), page as i32, false);
        out.push_str(&svg);
        Ok(())
    }

    /// Render the loaded document to MIDI as a **base64-encoded** string —
    /// Verovio's upstream convention so the binary payload fits inside a
    /// `std::string`.
    ///
    /// **Most Rust callers want [`Self::render_to_midi_bytes`]** instead —
    /// the raw `.mid` SMF bytes you'd write to a file or hand to a
    /// synthesizer. This base64 form is provided for browser interop and
    /// applications that already pipe the upstream payload through a
    /// base64-aware downstream channel.
    ///
    /// Returns [`Error::RenderFailed`] (with `page: 0`) if no document is
    /// loaded (Verovio's `RenderToMIDI` would otherwise hit an internal
    /// `assert(!m_visibleScores.empty())` and SIGABRT the process — we
    /// gate on `page_count() == 0` first).
    pub fn render_to_midi(&mut self) -> Result<String> {
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        let midi = ffi::render_to_midi(self.inner.pin_mut());
        if midi.is_empty() {
            Err(Error::RenderFailed { page: 0 })
        } else {
            Ok(midi)
        }
    }

    /// Render to MIDI, reusing the caller's buffer. See [`Self::render_to_midi`]
    /// for the encoding contract.
    pub fn render_to_midi_into(&mut self, out: &mut String) -> Result<()> {
        out.clear();
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        let midi = ffi::render_to_midi(self.inner.pin_mut());
        if midi.is_empty() {
            return Err(Error::RenderFailed { page: 0 });
        }
        out.push_str(&midi);
        Ok(())
    }

    /// Render to MIDI as raw SMF (Standard MIDI File) bytes — the form you'd
    /// write to a `.mid` file or hand to a synthesizer like `rustysynth`.
    ///
    /// This is the **recommended primary form** for Rust consumers — the
    /// base64-encoded [`Self::render_to_midi`] only exists because upstream
    /// can't return binary `std::string` cleanly.
    ///
    /// Returns [`Error::RenderFailed`] if no document is loaded, or
    /// [`Error::Base64`] if Verovio's output is malformed (shouldn't happen).
    pub fn render_to_midi_bytes(&mut self) -> Result<Vec<u8>> {
        use base64::Engine as _;
        let b64 = self.render_to_midi()?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64.as_bytes())?;
        Ok(bytes)
    }

    /// Render to MIDI and apply a per-track policy (channel reassignment,
    /// instrument override, volume, mute) to the SMF bytes before returning.
    ///
    /// Verovio emits every staff on MIDI channel 0 by default with no
    /// program-change / volume events. This wraps
    /// [`crate::midi::apply_track_policy`] to give consumers genuine
    /// multi-channel MIDI output — each staff as its own voice with its
    /// own instrument and level.
    ///
    /// See [`MidiTrackPolicy`](crate::midi::MidiTrackPolicy) for the
    /// shape of the policy and [`crate::midi`] for the design rationale.
    pub fn render_to_midi_bytes_with_policy(
        &mut self,
        policy: &crate::midi::MidiTrackPolicy,
    ) -> Result<Vec<u8>> {
        let bytes = self.render_to_midi_bytes()?;
        crate::midi::apply_track_policy(&bytes, policy).ok_or(Error::RenderFailed { page: 0 })
    }

    /// Render the document's playback timemap as a JSON string.
    ///
    /// The timemap is the playhead-sync map xpart needs:
    /// `[{tstamp, qstamp, on:[ids], off:[ids], tempo}, ...]`. Parse with
    /// `serde_json` — or use [`Self::timemap`] for the typed equivalent.
    ///
    /// Returns [`Error::RenderFailed`] (with `page: 0`) if no document is
    /// loaded (Verovio's score-walk would otherwise assert).
    pub fn render_to_timemap(&mut self) -> Result<String> {
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        let json = ffi::render_to_timemap(self.inner.pin_mut(), "");
        if json.is_empty() {
            Err(Error::RenderFailed { page: 0 })
        } else {
            Ok(json)
        }
    }

    /// Render the timemap, reusing the caller's buffer.
    pub fn render_to_timemap_into(&mut self, out: &mut String) -> Result<()> {
        out.clear();
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        let json = ffi::render_to_timemap(self.inner.pin_mut(), "");
        if json.is_empty() {
            return Err(Error::RenderFailed { page: 0 });
        }
        out.push_str(&json);
        Ok(())
    }

    /// Render the timemap with explicit upstream options applied. `json_options`
    /// is the same JSON document Verovio's `Toolkit::RenderToTimemap` accepts —
    /// notably:
    ///
    /// | Option            | Type | Effect                                                |
    /// |-------------------|------|-------------------------------------------------------|
    /// | `useFractions`    | bool | Emit `tstamp` / `qstamp` as exact `[num, den]` pairs  |
    /// |                   |      | instead of f64 milliseconds — see [`Self::timemap_exact`] |
    /// | `includeRests`    | bool | Add `restsOn` / `restsOff` arrays to each event       |
    /// | `includeMeasures` | bool | Add `measureOn` (MEI ID) when crossing a barline      |
    ///
    /// Returns [`Error::RenderFailed`] if no document is loaded.
    pub fn render_to_timemap_with_options(&mut self, json_options: &str) -> Result<String> {
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        let json = ffi::render_to_timemap(self.inner.pin_mut(), json_options);
        if json.is_empty() {
            Err(Error::RenderFailed { page: 0 })
        } else {
            Ok(json)
        }
    }

    /// Force a layout pass on the currently-loaded document.
    ///
    /// Layout happens lazily on the first render call; explicit
    /// `redo_layout` is only needed after option changes that affect layout.
    pub fn redo_layout(&mut self) {
        self.cached_page_count.set(None);
        ffi::redo_layout(self.inner.pin_mut(), "");
    }

    /// Force a layout pass with a JSON options overlay applied for this
    /// pass only.
    pub fn redo_layout_with_options(&mut self, options: &str) {
        self.cached_page_count.set(None);
        ffi::redo_layout(self.inner.pin_mut(), options);
    }

    /// Recalculate only the vertical (pitch) positions of notes on the
    /// current page — cheaper than a full [`Self::redo_layout`].
    ///
    /// Use after option changes that affect note positioning but not
    /// horizontal spacing (e.g. fine-grained accidental tuning).
    pub fn redo_page_pitch_pos_layout(&mut self) {
        ffi::redo_page_pitch_pos_layout(self.inner.pin_mut());
    }

    /// Render the timemap parsed into typed [`TimemapEvent`](crate::TimemapEvent)s
    /// — the form `xpart` actually consumes. See [`Self::render_to_timemap`]
    /// for the raw JSON-string variant.
    ///
    /// **Hot-path note:** call this *once per loaded document* and walk the
    /// returned `Vec` directly in your playback loop. Re-calling on every
    /// frame re-traverses the FFI boundary and re-parses JSON — that's the
    /// ~22 µs/call benched. The vector itself is cheap to walk
    /// (sorted by `tstamp` upstream, so a `partition_point` binary search is
    /// O(log n) for "what's active at time t").
    pub fn timemap(&mut self) -> Result<Timemap> {
        let json = self.render_to_timemap()?;
        Ok(serde_json::from_str(&json)?)
    }

    /// Render the document's expansion map as a JSON string.
    ///
    /// The expansion map encodes how MEI `<expansion>` markers (repeats,
    /// segno/fine, voltas, …) unfold into a linear playback sequence. The
    /// JSON shape is an object: `{ "originalId": ["expandedId1", ...], … }`
    /// where each key is an MEI element ID (usually a `<measure>` ID) and
    /// the value lists the IDs as they appear in playback order — so an
    /// id that's played twice appears twice in the array.
    ///
    /// Returns `"{}"` if the loaded score has no expansion markers (most
    /// short fixtures, all PAE/ABC). Returns
    /// [`Error::RenderFailed`] for an unloaded toolkit (Verovio's
    /// `SetMidiDoc` asserts otherwise).
    pub fn render_to_expansion_map(&mut self) -> Result<String> {
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        let json = ffi::render_to_expansion_map(self.inner.pin_mut());
        Ok(json)
    }

    /// Typed version of [`Self::render_to_expansion_map`] — the JSON object
    /// parsed into a [`ExpansionMap`] (`BTreeMap<String, Vec<String>>`).
    ///
    /// For scores without `<expansion>` markers, returns an empty map.
    pub fn expansion_map(&mut self) -> Result<ExpansionMap> {
        let json = self.render_to_expansion_map()?;
        Ok(serde_json::from_str(&json)?)
    }

    /// Build a side-table mapping every element ID in the rendered SVG to
    /// the **1-indexed staff number** it belongs to. The numbering matches
    /// upstream — staff 1 is the first `<staff n="1">` in the MEI, which
    /// is also Verovio's SMF track 1 (track 0 is meta).
    ///
    /// Pairs with [`Self::render_to_midi_bytes_with_policy`] for genuine
    /// multi-track visual sync: a consumer can color the playing notes
    /// per track by looking up each currently-sounding ID in the returned
    /// table and styling by staff number.
    ///
    /// # How the staff number is derived
    ///
    /// Verovio emits `<g class="staff">` wrappers **per measure**, not
    /// per logical staff — a 2-measure single-staff score gets 2 wrappers,
    /// a 1-measure 2-staff score also gets 2 wrappers. The wrappers
    /// don't carry an `n="…"` attribute either.
    ///
    /// The algorithm restarts the staff counter at every `<g class="measure">`
    /// and assigns 1, 2, … to staff wrappers in document order within that
    /// measure. This matches the source MEI's `<staff n="N">` ordering: the
    /// k-th staff wrapper inside a measure is staff k.
    ///
    /// Cost: renders every page to SVG and parses it. For multi-page
    /// scores this is `pages × (SVG render + XML parse)` — meaningful but
    /// paid once per loaded document and cacheable by the consumer. Don't
    /// call per frame.
    ///
    /// Returns [`Error::Xml`] if any page's SVG fails to parse (would be
    /// a Verovio bug, not a user error).
    pub fn staff_map(&mut self) -> Result<HashMap<String, u32>> {
        let mut out = HashMap::new();
        let pages = self.page_count();
        for page in 1..=pages {
            let svg = self.render_to_svg(page)?;
            let doc = roxmltree::Document::parse(&svg).map_err(|e| Error::Xml(e.to_string()))?;

            for measure in doc.descendants().filter(|n| {
                n.is_element()
                    && n.tag_name().name() == "g"
                    && n.attribute("class") == Some("measure")
            }) {
                let mut staff_idx: u32 = 0;
                for staff in measure.descendants().filter(|n| {
                    n.is_element()
                        && n.tag_name().name() == "g"
                        && n.attribute("class") == Some("staff")
                }) {
                    staff_idx += 1;
                    for desc in staff.descendants() {
                        if let Some(id) = desc.attribute("id") {
                            // or_insert so a tied-over id (appearing in
                            // multiple measures) resolves to the same staff
                            // number both times.
                            out.entry(id.to_string()).or_insert(staff_idx);
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Build a side-table mapping every element ID in the rendered SVG to
    /// its axis-aligned bounding box (in Verovio's SVG viewBox coordinate
    /// system). Powers "click on note to seek" hit testing and
    /// "highlight box around the playing note" overlays.
    ///
    /// # How the bbox is derived
    ///
    /// Walks every page's SVG with a `translate(x, y)` transform stack,
    /// collecting coordinate samples from `<use>` glyph references and
    /// `<path d="M x y L x y">` stem / barline / staff-line strokes. For
    /// each `<g id="...">` boundary, the union of its descendants' sample
    /// points becomes the bbox.
    ///
    /// **Accuracy contract**: the bbox covers anchor points and explicit
    /// path coordinates. SMuFL glyph extents are approximated as a
    /// `200 × 300` unit footprint around each `<use>` anchor (typical
    /// notehead size — Bravura's noteheads are roughly this scale).
    /// Good enough for hit testing and visible highlights; not pixel
    /// perfect for layout debugging (use Verovio's
    /// `svgBoundingBoxes` option for that).
    ///
    /// Cost: renders every page to SVG and parses it. Cacheable by the
    /// consumer.
    pub fn bbox_map(&mut self) -> Result<HashMap<String, BBox>> {
        let mut out: HashMap<String, BBox> = HashMap::new();
        let pages = self.page_count();
        for page in 1..=pages {
            let svg = self.render_to_svg(page)?;
            let doc = roxmltree::Document::parse(&svg).map_err(|e| Error::Xml(e.to_string()))?;
            walk_bbox(doc.root_element(), (0.0, 0.0), page, &mut out);
        }
        Ok(out)
    }

    /// Build a side-table classifying every element ID that appears in the
    /// timemap by structural kind (note / chord / rest / measure). Returned
    /// `HashMap` supports O(1) lookup of "is this id a note?" — useful for
    /// playback drivers that want to filter highlights by element type.
    ///
    /// Pre-compute once per loaded document; the returned table is
    /// invariant under playback (it depends only on the score structure).
    ///
    /// Implementation note: walks the timemap's event tstamps and calls
    /// [`Self::elements_at`] at each (one FFI + JSON parse per event).
    /// Cost is `N events × ~2.4 µs` — ~250 µs for a 100-event score, paid
    /// once per `load_data`.
    pub fn classified_elements(&mut self) -> Result<ClassifiedElements> {
        // Collect tstamps first so the borrow on `self.timemap()` is released
        // before the per-event `self.elements_at(...)` calls.
        let tstamps: Vec<f64> = self.timemap()?.iter().map(|e| e.tstamp).collect();
        let mut out: ClassifiedElements = ClassifiedElements::default();
        for ms in tstamps {
            let els = self.elements_at(ms as u32)?;
            for id in els.notes {
                out.insert(id, ElementKind::Note);
            }
            for id in els.chords {
                out.insert(id, ElementKind::Chord);
            }
            for id in els.rests {
                out.insert(id, ElementKind::Rest);
            }
            if let Some(m) = els.measure {
                out.insert(m, ElementKind::Measure);
            }
        }
        Ok(out)
    }

    /// Extract the measure-level timeline as `Vec<MeasureInfo>` — for each
    /// measure, its MEI ID plus the wall-clock and quarter-beat range it
    /// covers. Powers "Measure N" displays and measure-based loop / seek
    /// UIs in playback consumers.
    ///
    /// Equivalent to
    /// `crate::lookup::measures_from_events(&self.timemap_exact()?)` —
    /// provided here for the common one-shot case. Programs that already
    /// cache `timemap_exact()` should call the pure
    /// [`crate::lookup::measures_from_events`] instead.
    pub fn measures(&mut self) -> Result<Vec<MeasureInfo>> {
        let events = self.timemap_exact()?;
        Ok(crate::lookup::measures_from_events(&events))
    }

    /// MEI ID of the measure enclosing the given wall-clock ms. Returns
    /// `None` if `ms` is before the first measure marker or no document
    /// is loaded.
    ///
    /// One-shot convenience over [`crate::lookup::measure_at_in`] —
    /// repeated calls re-render the timemap each time, so for tight
    /// playback loops cache `timemap_exact()` and use the pure helper.
    pub fn measure_at(&mut self, ms: f64) -> Result<Option<String>> {
        let events = self.timemap_exact()?;
        Ok(crate::lookup::measure_at_in(&events, ms).map(String::from))
    }

    /// Parse score-level metadata (title, composer, lyricist, copyright,
    /// instrument labels) out of the originally loaded MEI or MusicXML.
    /// Verovio's C++ Toolkit doesn't expose these — we parse them from
    /// the input at load time and cache the small [`ScoreMetadata`]
    /// struct rather than retaining the raw source.
    ///
    /// Returns mostly-empty fields when the source format doesn't carry
    /// the corresponding metadata: PAE, ABC, and Humdrum bodies have at
    /// most a title or composer, where MEI / MusicXML carry the full
    /// `<respStmt>` / `<identification>` set. Compressed (`.mxl`) and
    /// UTF-16 inputs loaded via [`Self::load_file`] never carry
    /// scrapeable metadata; the struct is empty in those cases.
    ///
    /// Returns [`Error::LoadFailed`] if no document has been loaded yet.
    pub fn metadata(&self) -> Result<ScoreMetadata> {
        self.cached_metadata.clone().ok_or(Error::LoadFailed)
    }

    /// Extract the tempo changes from the document as a [`TempoMap`] — the
    /// primitive xpart needs to drive playback under arbitrary tempo
    /// overrides (slow practice mode, click-track sync, etc.).
    ///
    /// Equivalent to `TempoMap::from_timemap(&self.timemap()?)` — provided
    /// here for the common case of "give me the tempo info now". For
    /// programs that already cache `timemap()`, calling
    /// `TempoMap::from_timemap(&cached)` is one fewer FFI crossing.
    ///
    /// Returns `None` if the timemap is empty or its first event has no
    /// tempo info (Verovio normally always publishes tempo first).
    pub fn tempo_map(&mut self) -> Result<Option<TempoMap>> {
        let tm = self.timemap()?;
        Ok(TempoMap::from_timemap(&tm))
    }

    /// Render the timemap with maximum precision: exact rational quarter-note
    /// timestamps (`qfrac: [num, den]`), rest events, and measure markers all
    /// included. Equivalent to calling
    /// [`Self::render_to_timemap_with_options`] with
    /// `{"useFractions": true, "includeRests": true, "includeMeasures": true}`
    /// then parsing into [`TimemapEventExact`].
    ///
    /// Use this when you care about accumulated precision (long scores, tight
    /// rhythmic detail like tuplets at fast tempos) — `qfrac` is exact and
    /// never drifts, unlike the f64 `tstamp` in [`Self::timemap`].
    ///
    /// Same hot-path advice applies: call once, walk locally.
    pub fn timemap_exact(&mut self) -> Result<Vec<TimemapEventExact>> {
        let json = self.render_to_timemap_with_options(
            r#"{"useFractions": true, "includeRests": true, "includeMeasures": true}"#,
        )?;
        Ok(serde_json::from_str(&json)?)
    }

    /// Return the elements active at the given playback time as a typed
    /// [`ElementsAtTime`].
    ///
    /// Returns [`Error::RenderFailed`] if no document is loaded.
    pub fn elements_at(&mut self, millis: u32) -> Result<ElementsAtTime> {
        let json = self.elements_at_time(millis)?;
        Ok(serde_json::from_str(&json)?)
    }

    /// Return the element IDs active at the given playback time, as a JSON
    /// document. The shape upstream is roughly `{notes: [...], page: N}`.
    /// Parse with `serde_json` — or use [`Self::elements_at`] for the typed
    /// equivalent.
    ///
    /// Returns [`Error::RenderFailed`] (with `page: 0`) if no document is
    /// loaded — consistent with the rest of the render family. For an
    /// always-Ok variant returning `"{}"` on missing data, fall back to
    /// `.elements_at_time(ms).unwrap_or_else(|_| "{}".into())`.
    pub fn elements_at_time(&mut self, millis: u32) -> Result<String> {
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        Ok(ffi::get_elements_at_time(
            self.inner.pin_mut(),
            millis as i32,
        ))
    }

    /// Return the element IDs active at the given playback time, written
    /// into the caller's buffer.
    pub fn elements_at_time_into(&mut self, millis: u32, out: &mut String) -> Result<()> {
        out.clear();
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        let json = ffi::get_elements_at_time(self.inner.pin_mut(), millis as i32);
        out.push_str(&json);
        Ok(())
    }

    /// Render every page that touches measures `from..=to` (1-indexed,
    /// inclusive). Returns the rendered pages concatenated by `joiner`.
    ///
    /// Implementation: sets Verovio's `measureFrom` / `measureTo` options,
    /// triggers a layout pass, renders the resulting pages, then restores
    /// the previous options and re-lays out. One layout pass + N SVG
    /// renders per call.
    ///
    /// `from == 0` or `from > to` returns `Ok(String::new())`. `to` past
    /// the last measure is silently clamped by Verovio.
    pub fn render_svg_measure_range(&mut self, from: u32, to: u32, joiner: &str) -> Result<String> {
        if from == 0 || from > to {
            return Ok(String::new());
        }
        let saved_opts = self.options();
        let scoped = format!(r#"{{"measureFrom": "{from}", "measureTo": "{to}"}}"#);
        self.set_options(&scoped)?;
        self.redo_layout();
        let mut out = String::new();
        let pages = self.page_count();
        let mut buf = String::new();
        for page in 1..=pages {
            self.render_to_svg_into(page, &mut buf)?;
            if !out.is_empty() {
                out.push_str(joiner);
            }
            out.push_str(&buf);
        }
        // Best-effort restore — never swallow the render error to report
        // a restore error.
        let _ = self.set_options(&saved_opts);
        self.redo_layout();
        Ok(out)
    }

    // -----------------------------------------------------------------
    // Diagnostics
    // -----------------------------------------------------------------

    /// Opaque per-document identifier (`vrv::Doc::GetID`). Useful for
    /// log correlation when running multiple toolkits in one process.
    pub fn id(&mut self) -> String {
        ffi::get_id(self.inner.pin_mut())
    }

    /// Read back the resource path Verovio is currently using — the
    /// directory containing the SMuFL font data (Bravura, Leipzig, …).
    pub fn resource_path(&self) -> String {
        ffi::get_resource_path(&self.inner)
    }

    // -----------------------------------------------------------------
    // Options surface (extended)
    // -----------------------------------------------------------------

    /// Full option schema as a JSON document: every option grouped by
    /// category, with type / default / minimum / maximum where
    /// available. Use for generating CLI help, validating user input
    /// before applying it, or building an options-editor UI.
    pub fn available_options(&self) -> String {
        ffi::get_available_options(&self.inner)
    }

    /// Reset every option to its compile-time default. Invalidates the
    /// cached page count (option changes can affect layout).
    pub fn reset_options(&mut self) {
        self.cached_page_count.set(None);
        ffi::reset_options(self.inner.pin_mut());
    }

    /// Apply a region selection (measure / staff range) for subsequent
    /// renders. See `vrv::Toolkit::Select` upstream for the JSON
    /// schema; pass an empty string or `"{}"` to clear the selection.
    ///
    /// Returns [`Error::OptionsRejected`] if Verovio rejects the JSON
    /// (malformed, or naming an mdiv that doesn't exist).
    pub fn select(&mut self, selection: &str) -> Result<()> {
        self.cached_page_count.set(None);
        if ffi::select(self.inner.pin_mut(), selection) {
            Ok(())
        } else {
            Err(Error::OptionsRejected)
        }
    }

    /// Typed scale setter — same effect as `set_options({"scale": pct})`
    /// without the JSON round-trip. `100` = 1×, `200` = 2×.
    pub fn set_scale(&mut self, pct: u32) -> Result<()> {
        self.cached_page_count.set(None);
        if ffi::set_scale(self.inner.pin_mut(), pct as i32) {
            Ok(())
        } else {
            Err(Error::OptionsRejected)
        }
    }

    /// Current scale percent. `100` = 1×.
    pub fn scale(&mut self) -> u32 {
        let s = ffi::get_scale(self.inner.pin_mut());
        s.max(0) as u32
    }

    /// Force the input format instead of letting Verovio auto-detect.
    /// Accepts the same strings Verovio's `--from` CLI flag does
    /// (`"mei"`, `"musicxml"`, `"abc"`, `"pae"`, `"humdrum"`, …).
    ///
    /// Returns [`Error::OptionsRejected`] for unrecognized format names.
    pub fn set_input_from(&mut self, format: &str) -> Result<()> {
        self.cached_page_count.set(None);
        if ffi::set_input_from(self.inner.pin_mut(), format) {
            Ok(())
        } else {
            Err(Error::OptionsRejected)
        }
    }

    /// Force the output format for subsequent calls that respect the
    /// `outputTo` option. Accepts the same strings as Verovio's
    /// `--to` CLI flag (`"svg"`, `"mei"`, `"mei-basic"`, `"midi"`,
    /// `"pae"`, `"timemap"`, …).
    pub fn set_output_to(&mut self, format: &str) -> Result<()> {
        self.cached_page_count.set(None);
        if ffi::set_output_to(self.inner.pin_mut(), format) {
            Ok(())
        } else {
            Err(Error::OptionsRejected)
        }
    }

    /// Re-seed the `@xml:id` generator. Pass `0` for a time-based
    /// random seed; pass a fixed value for reproducible IDs in
    /// snapshot tests. No-op when Verovio's `--xml-id-checksum` option
    /// is set.
    pub fn reset_xml_id_seed(&mut self, seed: u32) {
        ffi::reset_xml_id_seed(self.inner.pin_mut(), seed as i32);
    }

    /// Apply a batch of layout options via [`LayoutOptions`] — one
    /// `SetOptions` call (= one layout invalidation), versus the N
    /// invalidations you'd pay by chaining `set_font` / `set_zoom` /
    /// `set_page_size` separately.
    ///
    /// Unset (`None`) fields are omitted from the emitted JSON, so
    /// they keep their previous values.
    pub fn set_layout_options(&mut self, opts: &LayoutOptions) -> Result<()> {
        self.set_options(&opts.to_json())
    }

    // -----------------------------------------------------------------
    // Conversion / serialization
    // -----------------------------------------------------------------

    /// Serialize the loaded document as MEI with default options
    /// (score-based, all pages, IDs preserved). Convenient for
    /// converting MusicXML / ABC / PAE / Humdrum input to canonical MEI.
    pub fn to_mei(&mut self) -> Result<String> {
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        Ok(ffi::get_mei(self.inner.pin_mut(), ""))
    }

    /// Serialize the loaded document as MEI with explicit options
    /// (page selection, MEI-Basic restriction, ID cleanup). See
    /// [`MeiOptions`] for the fields.
    pub fn to_mei_with_options(&mut self, opts: &MeiOptions) -> Result<String> {
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        Ok(ffi::get_mei(self.inner.pin_mut(), &opts.to_json()))
    }

    /// Serialize the loaded document as Plaine & Easie code. Only the
    /// top staff / layer is exported (upstream limitation).
    pub fn render_to_pae(&mut self) -> Result<String> {
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        let pae = ffi::render_to_pae(self.inner.pin_mut());
        if pae.is_empty() {
            Err(Error::RenderFailed { page: 0 })
        } else {
            Ok(pae)
        }
    }

    /// Validate Plaine & Easie input — returns the upstream JSON object
    /// describing any warnings or errors per input key. **Discards any
    /// previously loaded document** (upstream behavior; documented on
    /// `vrv::Toolkit::ValidatePAE`).
    ///
    /// Returns the raw JSON string for callers that want to forward it;
    /// pair with `serde_json::from_str` if you want to walk the
    /// structure.
    pub fn validate_pae(&mut self, data: &str) -> String {
        self.invalidate_caches();
        ffi::validate_pae(self.inner.pin_mut(), data)
    }

    /// Extract descriptive features for incipit / structural search,
    /// as a JSON string. `json_options` is upstream's feature-extraction
    /// option schema; pass `"{}"` for defaults.
    pub fn descriptive_features(&mut self, json_options: &str) -> Result<String> {
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        Ok(ffi::get_descriptive_features(
            self.inner.pin_mut(),
            json_options,
        ))
    }

    // -----------------------------------------------------------------
    // Element introspection — the inverse of `elements_at_time`.
    // -----------------------------------------------------------------

    /// Page (1-based) on which the element with `xml_id` is rendered.
    /// Returns `None` if the element doesn't exist in the loaded
    /// document.
    pub fn page_with_element(&mut self, xml_id: &str) -> Option<u32> {
        let n = ffi::get_page_with_element(self.inner.pin_mut(), xml_id);
        if n <= 0 {
            None
        } else {
            Some(n as u32)
        }
    }

    /// Wall-clock millisecond onset of the element with `xml_id`.
    /// Returns `None` if the element doesn't exist or isn't playable
    /// (rest, measure marker, …).
    ///
    /// Internally renders the timemap once per loaded document to
    /// populate Verovio's MIDI doc — required by upstream and
    /// otherwise a documented footgun. Subsequent calls hit the
    /// cached MIDI doc.
    pub fn time_for_element(&mut self, xml_id: &str) -> Option<u32> {
        self.ensure_midi_doc().ok()?;
        let t = ffi::get_time_for_element(self.inner.pin_mut(), xml_id);
        if t < 0 {
            None
        } else {
            Some(t as u32)
        }
    }

    /// MIDI pitch / onset / duration for a note element. Returns `None`
    /// for chord wrappers, rests, missing IDs, or elements that aren't
    /// individual notes — upstream emits an empty JSON object in those
    /// cases.
    ///
    /// Internally renders the timemap once per loaded document.
    pub fn midi_values_for_element(&mut self, xml_id: &str) -> Result<Option<MidiValues>> {
        self.ensure_midi_doc()?;
        let json = ffi::get_midi_values_for_element(self.inner.pin_mut(), xml_id);
        // Upstream returns "{}" for non-notes / missing elements; map
        // that to None so the typed wrapper isn't misleading.
        let v: serde_json::Value = serde_json::from_str(&json)?;
        if v.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            return Ok(None);
        }
        Ok(Some(serde_json::from_value(v)?))
    }

    /// Score-time + wall-clock onset/offset for a note element. Returns
    /// `None` for non-notes or missing IDs (upstream emits `{}`).
    ///
    /// Internally renders the timemap once per loaded document.
    pub fn times_for_element(&mut self, xml_id: &str) -> Result<Option<ElementTimes>> {
        self.ensure_midi_doc()?;
        let json = ffi::get_times_for_element(self.inner.pin_mut(), xml_id);
        let v: serde_json::Value = serde_json::from_str(&json)?;
        if v.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            return Ok(None);
        }
        Ok(Some(serde_json::from_value(v)?))
    }

    /// Force Verovio to populate its internal MIDI document and its
    /// timemap if the loaded score hasn't done so yet. Required before
    /// element-time / element-MIDI-value queries — upstream documents
    /// "RenderToMIDI() must be called prior". We do it lazily here so
    /// callers don't have to remember the dance.
    fn ensure_midi_doc(&mut self) -> Result<()> {
        if self.page_count() == 0 {
            return Err(Error::RenderFailed { page: 0 });
        }
        // RenderToTimemap is cheaper than RenderToMIDI (no SMF
        // serialization) and bootstraps the same internal state.
        // The result is discarded; this exists purely for the side
        // effect on Verovio's MIDI doc.
        let _ = ffi::render_to_timemap(self.inner.pin_mut(), "");
        Ok(())
    }

    /// Every MEI attribute on the element with `xml_id`, as a typed
    /// map. Returns an empty map for unknown IDs. Attribute values are
    /// returned as `serde_json::Value` (most are strings but
    /// numeric-typed attrs come back as numbers).
    pub fn element_attr(&mut self, xml_id: &str) -> Result<HashMap<String, serde_json::Value>> {
        let json = ffi::get_element_attr(self.inner.pin_mut(), xml_id);
        Ok(serde_json::from_str(&json)?)
    }

    /// MEI ID of the notated (original) element when `xml_id` refers
    /// to an expansion clone. Returns `xml_id` itself if the score
    /// has no `<expansion>` markers — this matches upstream's
    /// pass-through behavior.
    pub fn notated_id_for_element(&mut self, xml_id: &str) -> String {
        ffi::get_notated_id_for_element(self.inner.pin_mut(), xml_id)
    }

    /// Every expansion-clone ID that shares the notated `xml_id`.
    /// Returns an empty vec when the score has no expansion map
    /// (upstream emits `[""]` in that case — we strip the sentinel).
    pub fn expansion_ids_for_element(&mut self, xml_id: &str) -> Result<Vec<String>> {
        let json = ffi::get_expansion_ids_for_element(self.inner.pin_mut(), xml_id);
        let ids: Vec<String> = serde_json::from_str(&json)?;
        // Strip the upstream sentinel: a single empty string means
        // "no expansion map at all" rather than "an element with no
        // expansion clones".
        if ids.len() == 1 && ids[0].is_empty() {
            return Ok(Vec::new());
        }
        Ok(ids)
    }
}

impl Default for Toolkit {
    fn default() -> Self {
        Self::new()
    }
}

/// Approximate SMuFL notehead / glyph footprint, in SVG viewBox units —
/// used to give `<use>` references a non-zero bbox even though their
/// glyph extents aren't carried in the rendered SVG.
const GLYPH_HALF_W: f64 = 100.0;
const GLYPH_HALF_H: f64 = 150.0;

/// Recursively walk a Verovio SVG node, accumulating absolute
/// coordinate samples from `<use>` and `<path>` descendants. At each
/// `<g id="...">` boundary, record the bbox of its accumulated samples.
/// Returns the sample list for the caller to fold into its own bbox.
fn walk_bbox<'a>(
    node: roxmltree::Node<'a, 'a>,
    translate: (f64, f64),
    page: u32,
    out: &mut HashMap<String, BBox>,
) -> Vec<(f64, f64)> {
    let mut t = translate;
    if let Some(s) = node.attribute("transform") {
        if let Some((dx, dy)) = parse_translate(s) {
            t.0 += dx;
            t.1 += dy;
        }
    }

    let mut samples: Vec<(f64, f64)> = Vec::new();
    let tag = node.tag_name().name();

    if tag == "use" {
        // Per-element `x`/`y` attributes layered on top of the cumulative
        // `transform`. Verovio emits glyphs almost always with the
        // position in the transform, but the spec permits both.
        let x = node
            .attribute("x")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let y = node
            .attribute("y")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let cx = t.0 + x;
        let cy = t.1 + y;
        samples.push((cx - GLYPH_HALF_W, cy - GLYPH_HALF_H));
        samples.push((cx + GLYPH_HALF_W, cy + GLYPH_HALF_H));
    } else if tag == "path" {
        if let Some(d) = node.attribute("d") {
            extract_path_points(d, t, &mut samples);
        }
    } else if tag == "rect" {
        let x = node
            .attribute("x")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let y = node
            .attribute("y")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let w = node
            .attribute("width")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let h = node
            .attribute("height")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        samples.push((t.0 + x, t.1 + y));
        samples.push((t.0 + x + w, t.1 + y + h));
    }

    for child in node.children().filter(|c| c.is_element()) {
        let child_samples = walk_bbox(child, t, page, out);
        samples.extend(child_samples);
    }

    if let Some(id) = node.attribute("id") {
        if !samples.is_empty() {
            let (min_x, min_y, max_x, max_y) = bounds_of(&samples);
            // `or_insert` — if the same id appears across pages (rare),
            // keep the first occurrence so callers get a stable answer.
            out.entry(id.to_string()).or_insert(BBox {
                x: min_x,
                y: min_y,
                width: max_x - min_x,
                height: max_y - min_y,
                page,
            });
        }
    }
    samples
}

/// Parse a `transform="translate(x, y) …"` attribute, returning the
/// translate component as `(dx, dy)`. Other transform fns (scale,
/// rotate) are ignored — Verovio uses translate for layout positioning
/// and scale for SMuFL glyph sizing (the latter doesn't move element
/// anchors, just resizes the glyph in place).
fn parse_translate(s: &str) -> Option<(f64, f64)> {
    let start = s.find("translate")?;
    let after = &s[start + "translate".len()..];
    let open = after.find('(')?;
    let close = after[open + 1..].find(')')?;
    let inner = &after[open + 1..open + 1 + close];
    let mut parts = inner.split(|c: char| c == ',' || c.is_whitespace());
    let dx: f64 = parts.find(|s| !s.is_empty())?.parse().ok()?;
    let dy: f64 = parts.find(|s| !s.is_empty()).unwrap_or("0").parse().ok()?;
    Some((dx, dy))
}

/// Extract `(x, y)` coordinates from an SVG `d=` path attribute. Only
/// honors `M` / `L` / `m` / `l` (move / line, abs/rel) — sufficient for
/// Verovio's stems, barlines, staff lines, ledger lines, and beams,
/// which are the only paths visible in a layout SVG (glyph paths live
/// inside `<defs>` and don't carry layout coords).
fn extract_path_points(d: &str, translate: (f64, f64), out: &mut Vec<(f64, f64)>) {
    let mut last_abs = (0.0, 0.0);
    let mut iter = d.split_whitespace().peekable();
    while let Some(tok) = iter.next() {
        let bytes = tok.as_bytes();
        if bytes.is_empty() {
            continue;
        }
        let first = bytes[0];
        match first {
            b'M' | b'L' => {
                let rest = &tok[1..];
                let x: f64 = if rest.is_empty() {
                    iter.next().and_then(|s| s.parse().ok()).unwrap_or(0.0)
                } else {
                    rest.parse().unwrap_or(0.0)
                };
                let y: f64 = iter.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                last_abs = (x, y);
                out.push((translate.0 + x, translate.1 + y));
            }
            b'm' | b'l' => {
                let rest = &tok[1..];
                let dx: f64 = if rest.is_empty() {
                    iter.next().and_then(|s| s.parse().ok()).unwrap_or(0.0)
                } else {
                    rest.parse().unwrap_or(0.0)
                };
                let dy: f64 = iter.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                last_abs = (last_abs.0 + dx, last_abs.1 + dy);
                out.push((translate.0 + last_abs.0, translate.1 + last_abs.1));
            }
            _ => {
                // Other path commands (curves, arcs) — Verovio's layout
                // SVG doesn't use these for engraved geometry, so skip.
            }
        }
    }
}

fn looks_like_xml(s: &str) -> bool {
    s.starts_with("<?xml")
        || s.starts_with("<mei")
        || s.starts_with("<score-partwise")
        || s.starts_with("<score-timewise")
        || s.starts_with("<!DOCTYPE")
}

/// Parse score-level metadata from raw input text. Picks the right
/// branch (MEI / MusicXML / plaintext) and falls back to an empty
/// struct on parse failure — metadata is best-effort by design.
fn parse_metadata(src: &str) -> ScoreMetadata {
    let trimmed = src.trim_start();
    if looks_like_xml(trimmed) {
        // MusicXML files routinely carry a DOCTYPE declaration; roxmltree
        // refuses those by default for XXE-style safety. Score input is
        // trusted here (the caller already handed it to Verovio), so opt
        // in to DTD parsing.
        let opts = roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        };
        match roxmltree::Document::parse_with_options(src, opts) {
            Ok(doc) => {
                let root = doc.root_element();
                let root_name = root.tag_name().name();
                if root_name == "mei" {
                    parse_mei_metadata(&doc)
                } else if root_name == "score-partwise" || root_name == "score-timewise" {
                    parse_musicxml_metadata(&doc)
                } else {
                    ScoreMetadata::default()
                }
            }
            Err(_) => ScoreMetadata::default(),
        }
    } else {
        parse_plaintext_metadata(src)
    }
}

fn parse_mei_metadata(doc: &roxmltree::Document) -> ScoreMetadata {
    let mut md = ScoreMetadata::default();
    let root = doc.root_element();
    for desc in root.descendants() {
        if !desc.is_element() {
            continue;
        }
        let name = desc.tag_name().name();
        match name {
            "title" if md.title.is_none() => {
                md.title = text_of(desc);
            }
            "persName" => {
                let role = desc.attribute("role").unwrap_or("");
                match role {
                    "composer" if md.composer.is_none() => md.composer = text_of(desc),
                    "lyricist" | "librettist" if md.lyricist.is_none() => {
                        md.lyricist = text_of(desc)
                    }
                    "arranger" if md.arranger.is_none() => md.arranger = text_of(desc),
                    _ => {}
                }
            }
            "availability" | "useRestrict" if md.copyright.is_none() => {
                md.copyright = text_of(desc);
            }
            "label" => {
                // staffDef labels — captured in document order; the
                // staff number is on the parent staffDef's `n=` attr but
                // not all MEI files carry it, so we accept order order.
                if let Some(parent) = desc.parent() {
                    let pname = parent.tag_name().name();
                    if pname == "staffDef" {
                        if let Some(t) = text_of(desc) {
                            md.instruments.push(t);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    md
}

fn parse_musicxml_metadata(doc: &roxmltree::Document) -> ScoreMetadata {
    let mut md = ScoreMetadata::default();
    let root = doc.root_element();
    for desc in root.descendants() {
        if !desc.is_element() {
            continue;
        }
        let name = desc.tag_name().name();
        match name {
            "work-title" if md.title.is_none() => {
                md.title = text_of(desc);
            }
            "creator" => {
                let typ = desc.attribute("type").unwrap_or("");
                match typ {
                    "composer" if md.composer.is_none() => md.composer = text_of(desc),
                    "lyricist" | "poet" if md.lyricist.is_none() => md.lyricist = text_of(desc),
                    "arranger" if md.arranger.is_none() => md.arranger = text_of(desc),
                    _ => {}
                }
            }
            "rights" if md.copyright.is_none() => {
                md.copyright = text_of(desc);
            }
            "part-name" => {
                if let Some(t) = text_of(desc) {
                    md.instruments.push(t);
                }
            }
            _ => {}
        }
    }
    md
}

fn parse_plaintext_metadata(src: &str) -> ScoreMetadata {
    let mut md = ScoreMetadata::default();
    // PAE: `@start:<label>` lines aren't titles per se — skip them.
    // ABC: `T:Title`, `C:Composer`, `Z:Copyright`, `V:Voice` headers.
    for line in src.lines().take(40) {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("T:") {
            if md.title.is_none() {
                md.title = Some(rest.trim().to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("C:") {
            if md.composer.is_none() {
                md.composer = Some(rest.trim().to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("Z:") {
            if md.copyright.is_none() {
                md.copyright = Some(rest.trim().to_string());
            }
        }
    }
    md
}

fn text_of(node: roxmltree::Node) -> Option<String> {
    let mut buf = String::new();
    // Walk only text nodes — `Node::text()` on an element returns its
    // first text child, so iterating *all* descendants (including the
    // wrapping element) would yield duplicates.
    for d in node.descendants() {
        if d.node_type() == roxmltree::NodeType::Text {
            if let Some(t) = d.text() {
                buf.push_str(t);
            }
        }
    }
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn bounds_of(points: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (x, y) in points {
        if *x < min_x {
            min_x = *x;
        }
        if *y < min_y {
            min_y = *y;
        }
        if *x > max_x {
            max_x = *x;
        }
        if *y > max_y {
            max_y = *y;
        }
    }
    (min_x, min_y, max_x, max_y)
}
