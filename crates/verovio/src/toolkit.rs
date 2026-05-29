//! The [`Toolkit`] type itself — the safe wrapper around `vrv::Toolkit`.
//!
//! Every render-family method in this module checks
//! `self.page_count() == 0` before crossing the FFI boundary. Verovio's
//! `Doc::GetVisibleScores` asserts unconditionally on an empty document and
//! SIGABRTs the process; the guard converts that into
//! [`Error::RenderFailed`].

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cxx::UniquePtr;
use verovio_sys::ffi;

use crate::{ElementsAtTime, Error, Result, Timemap};

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
        Self { inner }
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

    /// Current option set as a JSON document.
    pub fn options(&self) -> String {
        ffi::get_options(&self.inner)
    }

    /// Default option values as a JSON document. Useful for discovering the
    /// option schema without mutating the toolkit.
    pub fn default_options(&self) -> String {
        ffi::get_default_options(&self.inner)
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
    pub fn timemap(&mut self) -> Result<Timemap> {
        let json = self.render_to_timemap()?;
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
}

impl Default for Toolkit {
    fn default() -> Self {
        Self::new()
    }
}
