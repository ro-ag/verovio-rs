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
    BBox, ClassifiedElements, ElementKind, ElementsAtTime, Error, ExpansionMap, MeasureInfo,
    MidiOptions, Result, ScoreMetadata, SvgOptions, TempoMap, Timemap, TimemapEventExact,
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
    /// Verbatim copy of the most recent `load_data` input, retained so
    /// [`Self::metadata`] can parse title / composer / etc. out of the
    /// original MEI or MusicXML — Verovio doesn't expose those through
    /// the C++ Toolkit API. Memory cost: one extra `String` per loaded
    /// score (~typical score: a few hundred KB).
    last_loaded: Option<String>,
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
            last_loaded: None,
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
    /// (MEI, MusicXML, Humdrum, ABC, PAE, ...).
    ///
    /// Returns [`Error::LoadFailed`] if Verovio's parser rejects the input.
    pub fn load_data(&mut self, data: &str) -> Result<()> {
        if ffi::load_data(self.inner.pin_mut(), data) {
            self.last_loaded = Some(data.to_string());
            Ok(())
        } else {
            Err(Error::LoadFailed)
        }
    }

    /// Read a score from disk and load it. Format auto-detected from the
    /// file contents (the extension is not consulted).
    ///
    /// Returns [`Error::Io`] on filesystem errors, [`Error::LoadFailed`] if
    /// the parser rejects the content.
    pub fn load_file(&mut self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let data = std::fs::read_to_string(path)?;
        self.load_data(&data)
    }

    /// Number of layout pages for the currently-loaded document.
    ///
    /// Takes `&mut self` because Verovio's `GetPageCount` is non-`const`
    /// upstream — it triggers lazy layout computation on first call.
    /// Returns `0` if no document is loaded or the upstream call returned
    /// a negative value.
    pub fn page_count(&mut self) -> u32 {
        let n = ffi::page_count(self.inner.pin_mut());
        n.max(0) as u32
    }

    /// Whether a document is currently loaded and laid out. Implemented as
    /// `page_count() > 0` — semantic predicate so consumers don't have to
    /// reach for the underlying number to express the same intent.
    pub fn is_loaded(&mut self) -> bool {
        self.page_count() > 0
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

    /// Render the loaded document to MIDI, returned as **base64-encoded**
    /// bytes (Verovio's upstream convention so the binary payload fits in a
    /// `std::string`).
    ///
    /// Decode with any base64 crate (e.g. `base64::engine::general_purpose
    /// ::STANDARD.decode(&midi)`) to get the raw `Vec<u8>` `.mid` payload —
    /// or just call [`Self::render_to_midi_bytes`].
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

    /// Render to MIDI, decoded into raw SMF (Standard MIDI File) bytes — the
    /// form you'd write to a `.mid` file. Convenience over the base64 round
    /// trip of [`Self::render_to_midi`].
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
        ffi::redo_layout(self.inner.pin_mut(), "");
    }

    /// Force a layout pass with a JSON options overlay applied for this
    /// pass only.
    pub fn redo_layout_with_options(&mut self, options: &str) {
        ffi::redo_layout(self.inner.pin_mut(), options);
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
    /// the verbatim input cached by [`Self::load_data`].
    ///
    /// Returns mostly-empty fields when the source format doesn't carry
    /// the corresponding metadata: PAE, ABC, and Humdrum bodies have at
    /// most a title or composer, where MEI / MusicXML carry the full
    /// `<respStmt>` / `<identification>` set.
    ///
    /// Returns [`Error::LoadFailed`] if no document has been loaded yet.
    pub fn metadata(&self) -> Result<ScoreMetadata> {
        let src = self.last_loaded.as_deref().ok_or(Error::LoadFailed)?;
        let trimmed = src.trim_start();
        if looks_like_xml(trimmed) {
            // MusicXML files routinely carry a DOCTYPE declaration;
            // roxmltree refuses those by default for XXE-style safety.
            // Score input is trusted here (the caller already handed it
            // to Verovio), so opt in to DTD parsing.
            let opts = roxmltree::ParsingOptions {
                allow_dtd: true,
                ..roxmltree::ParsingOptions::default()
            };
            let doc = roxmltree::Document::parse_with_options(src, opts)
                .map_err(|e| Error::Xml(e.to_string()))?;
            let root = doc.root_element();
            let root_name = root.tag_name().name();
            if root_name == "mei" {
                Ok(parse_mei_metadata(&doc))
            } else if root_name == "score-partwise" || root_name == "score-timewise" {
                Ok(parse_musicxml_metadata(&doc))
            } else {
                Ok(ScoreMetadata::default())
            }
        } else {
            // PAE / ABC / Humdrum — best-effort first-line scrape.
            Ok(parse_plaintext_metadata(src))
        }
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
    /// [`ElementsAtTime`]. Empty doc returns `Default::default()`.
    pub fn elements_at(&mut self, millis: u32) -> Result<ElementsAtTime> {
        let json = self.elements_at_time(millis);
        Ok(serde_json::from_str(&json)?)
    }

    /// Return the element IDs active at the given playback time, as a JSON
    /// document. The shape upstream is roughly `{notes: [...], page: N}`.
    /// Parse with `serde_json` — or use [`Self::elements_at`] for the typed
    /// equivalent.
    ///
    /// Returns `"{}"` if no document is loaded (Verovio's score-walk would
    /// otherwise assert).
    pub fn elements_at_time(&mut self, millis: u32) -> String {
        if self.page_count() == 0 {
            return "{}".into();
        }
        ffi::get_elements_at_time(self.inner.pin_mut(), millis as i32)
    }

    /// Return the element IDs active at the given playback time, written
    /// into the caller's buffer.
    pub fn elements_at_time_into(&mut self, millis: u32, out: &mut String) {
        out.clear();
        if self.page_count() == 0 {
            out.push_str("{}");
            return;
        }
        let json = ffi::get_elements_at_time(self.inner.pin_mut(), millis as i32);
        out.push_str(&json);
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
