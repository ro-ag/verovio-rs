//! Safe Rust bindings to [Verovio](https://www.verovio.org/), RISM's C++ music
//! notation engraver.
//!
//! # Quick start
//!
//! ```
//! use verovio::Toolkit;
//!
//! let mut tk = Toolkit::new();
//! tk.load_data(
//!     "@start:s\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:\n@data:'4G/4-\n@end:s\n"
//! )?;
//!
//! for page in 1..=tk.page_count() {
//!     let svg = tk.render_to_svg(page)?;
//!     // … write to disk or render in a UI
//!     # let _ = svg;
//! }
//! # Ok::<(), verovio::Error>(())
//! ```
//!
//! See also [`Toolkit::render_to_svg_into`] for the buffer-reuse variant
//! recommended for tight render loops.
//!
//! # Status
//!
//! API surface for xpart's needs: [`Toolkit::new`], [`Toolkit::load_data`],
//! [`Toolkit::page_count`], the option getters/setters, the rendering surface
//! ([`Toolkit::render_to_svg`], [`Toolkit::render_to_timemap`],
//! [`Toolkit::redo_layout`], [`Toolkit::elements_at_time`]), plus `_into`
//! buffer-reuse variants for every allocating method.
//!
//! # Resource files
//!
//! On first [`Toolkit::new`] call, the [`verovio_data`] crate's bundled SMuFL
//! resources are extracted to a process-lifetime temporary directory and
//! handed to Verovio via `SetResourcePath`. Subsequent toolkit constructions
//! reuse the same extraction. Verovio refuses to parse any input until
//! resources are available.
//!
//! # Thread safety
//!
//! [`Toolkit`] is `Send` but not `Sync`. Verovio's render/layout methods mutate
//! internal state even when shaped as read calls; sharing a `&Toolkit` between
//! threads is unsound. For concurrent rendering, construct one `Toolkit` per
//! thread or use a single worker thread fronted by a channel.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use cxx::UniquePtr;
use serde::{Deserialize, Serialize};
use verovio_sys::ffi;

/// One row of the playback timemap: a moment where elements turn on or off.
///
/// `tstamp` is in **milliseconds**; `qstamp` is in **quarter-note beats**.
/// `on` / `off` are MEI element IDs (the same IDs Verovio embeds as `xml:id`
/// in the SVG output and that [`Toolkit::elements_at_time`] reports).
/// `tempo` is BPM at this moment (present on the first event and any
/// subsequent tempo change).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TimemapEvent {
    /// Timestamp in milliseconds from the start of playback.
    pub tstamp: f64,
    /// Timestamp in quarter-note beats from the start of playback.
    pub qstamp: f64,
    /// Element IDs whose articulations begin at this moment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<String>,
    /// Element IDs whose articulations end at this moment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off: Vec<String>,
    /// Tempo (BPM) effective from this event onward, when Verovio
    /// publishes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tempo: Option<f64>,
}

/// The playhead-sync map for a loaded score: a chronological sequence of
/// note-on / note-off events with tempo metadata.
pub type Timemap = Vec<TimemapEvent>;

/// The elements active at a given playback time, as reported by
/// [`Toolkit::elements_at`].
///
/// All vec fields hold MEI element IDs (matching the `xml:id` attributes
/// in the SVG output). `measure` is the single enclosing measure ID, if
/// any. `page` is the 1-indexed page number Verovio resolved the time to.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct ElementsAtTime {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chords: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rests: Vec<String>,
}

/// Verbosity threshold for Verovio's internal log channel.
///
/// Mirrors the upstream `LogLevel` enum at `include/vrv/toolkitdef.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogLevel {
    /// Suppress all log output. Recommended for embedders that don't want
    /// Verovio writing to stdout.
    Off,
    /// Only errors.
    Error,
    /// Errors and warnings (Verovio's default).
    Warning,
    /// Errors, warnings, and informational messages.
    Info,
    /// Everything, including debug traces.
    Debug,
}

impl LogLevel {
    fn as_c_int(self) -> i32 {
        match self {
            LogLevel::Off => 0,
            LogLevel::Error => 1,
            LogLevel::Warning => 2,
            LogLevel::Info => 3,
            LogLevel::Debug => 4,
        }
    }
}

/// Set the Verovio log threshold globally for this process.
///
/// Verovio's log state is namespace-global (see `vrv::logLevel`), not
/// per-toolkit, so this call is **process-wide** — every existing and
/// future `Toolkit` in the process is affected.
///
/// Internally serialized with a mutex so concurrent threads can call this
/// without racing. The mutex is held only for the duration of the upstream
/// `EnableLog` call.
pub fn set_log_level(level: LogLevel) {
    static LOG_MUTEX: Mutex<()> = Mutex::new(());
    let _guard = LOG_MUTEX
        .lock()
        .expect("verovio log-control mutex poisoned");
    ffi::enable_log(level.as_c_int());
}

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

/// Error returned by safe Toolkit methods that can fail.
#[derive(Debug)]
pub enum Error {
    /// `LoadData` returned `false`. Verovio's internal log holds the reason;
    /// the log surface is not yet exposed (it is process-global and requires
    /// mutex gating per the safety contract).
    LoadFailed,
    /// `SetOptions` returned `false`. The JSON either failed to parse or named
    /// an option Verovio doesn't recognize.
    OptionsRejected,
    /// A render call returned an empty string. Typically means no document is
    /// loaded or the requested page is out of range.
    RenderFailed {
        /// 1-indexed page that was requested. `0` for whole-document renders
        /// (e.g. timemap).
        page: u32,
    },
    /// File-IO failure raised by `Toolkit::load_file` / `Toolkit::from_file`.
    Io(std::io::Error),
    /// JSON parse failure on a typed accessor ([`Toolkit::timemap`],
    /// [`Toolkit::elements_at`]). Indicates a shape mismatch between what
    /// Verovio produced and the Rust struct we expected.
    Json(serde_json::Error),
    /// Base64 decode failure inside [`Toolkit::render_to_midi_bytes`].
    /// Verovio is expected to emit well-formed base64; this would be a
    /// bug upstream rather than a user error.
    Base64(base64::DecodeError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::LoadFailed => f.write_str("Verovio failed to load data"),
            Error::OptionsRejected => f.write_str("Verovio rejected options"),
            Error::RenderFailed { page: 0 } => f.write_str("Verovio render returned empty"),
            Error::RenderFailed { page } => {
                write!(f, "Verovio render returned empty for page {page}")
            }
            Error::Io(e) => write!(f, "I/O error reading score file: {e}"),
            Error::Json(e) => write!(f, "JSON parse error from Verovio output: {e}"),
            Error::Base64(e) => write!(f, "base64 decode error from Verovio MIDI output: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::Base64(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

impl From<base64::DecodeError> for Error {
    fn from(e: base64::DecodeError) -> Self {
        Error::Base64(e)
    }
}

/// Result alias for fallible Toolkit operations.
pub type Result<T> = std::result::Result<T, Error>;

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
    /// ::STANDARD.decode(&midi)`) to get the raw `Vec<u8>` `.mid` payload.
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
    /// `[{tstamp_ms, on:[ids], off:[ids]}, ...]`. Parse with `serde_json`.
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

    /// Render the timemap parsed into typed [`TimemapEvent`]s — the form
    /// `xpart` actually consumes. See [`Self::render_to_timemap`] for the
    /// raw JSON-string variant.
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
